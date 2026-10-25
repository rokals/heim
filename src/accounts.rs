//! Comptes et sessions.
//!
//! Trois rôles :
//! - **Admin** : règle tout ;
//! - **Membre** : utilise tout, alarme comprise, sans toucher aux réglages ;
//! - **Invité** : lumières, prises, scènes et minuteurs. Pas l'alarme, pas les sirènes.
//!
//! Tant qu'aucun compte n'existe, Heim reste ouvert comme avant (pratique au premier
//! démarrage). Dès qu'un admin est créé, il faut se connecter.
//!
//! Les mots de passe sont hachés (PBKDF2-SHA256, sel aléatoire). Une session est un jeton
//! aléatoire de 256 bits ; seule son empreinte est rangée en base.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::slugify;
use crate::store::{StoreHandle, Table};

const PBKDF2_ROUNDS: u32 = 100_000;
const SESSIONS_PER_USER: usize = 20;
const SESSION_DAYS: i64 = 365;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Guest,
    Member,
    Admin,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Guest => "Invité",
            Role::Member => "Membre",
            Role::Admin => "Admin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Password {
    salt: String,
    hash: String,
    rounds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredUser {
    id: String,
    name: String,
    role: Role,
    password: Password,
    created: DateTime<Utc>,
}

/// Ce que l'API montre d'un compte (jamais le mot de passe).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub created: DateTime<Utc>,
}

impl From<&StoredUser> for User {
    fn from(u: &StoredUser) -> Self {
        User {
            id: u.id.clone(),
            name: u.name.clone(),
            role: u.role,
            created: u.created,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Session {
    /// Empreinte SHA-256 du jeton : une base volée ne donne pas accès.
    id: String,
    user_id: String,
    created: DateTime<Utc>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created: DateTime<Utc>,
    pub label: Option<String>,
}

#[derive(Default)]
struct Inner {
    users: BTreeMap<String, StoredUser>,
    sessions: HashMap<String, Session>,
    failures: VecDeque<Instant>,
}

pub struct Accounts {
    inner: Mutex<Inner>,
    store: StoreHandle,
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("source d'aléa du système");
    hex(&buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest(token: &str) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(token.as_bytes()))
}

fn hash_password(password: &str, salt: &str, rounds: u32) -> String {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt.as_bytes(), rounds, &mut out);
    hex(&out)
}

/// Comparaison en temps constant, pour ne rien laisser deviner au chronomètre.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn check_password_rules(password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("le mot de passe doit faire au moins 8 caractères".into());
    }
    if password.len() > 200 {
        return Err("mot de passe trop long".into());
    }
    Ok(())
}

impl Accounts {
    pub fn new(
        users: Vec<StoredUserRow>,
        sessions: Vec<SessionRow>,
        store: StoreHandle,
    ) -> Arc<Self> {
        let inner = Inner {
            users: users.into_iter().map(|u| (u.0.id.clone(), u.0)).collect(),
            sessions: sessions
                .into_iter()
                .map(|s| (s.0.id.clone(), s.0))
                .collect(),
            failures: VecDeque::new(),
        };
        Arc::new(Accounts {
            inner: Mutex::new(inner),
            store,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Aucun compte : Heim est ouvert à tout le réseau local.
    pub fn is_open(&self) -> bool {
        self.lock().users.is_empty()
    }

    pub fn list(&self) -> Vec<User> {
        let mut v: Vec<User> = self.lock().users.values().map(User::from).collect();
        v.sort_by(|a, b| b.role.cmp(&a.role).then_with(|| a.name.cmp(&b.name)));
        v
    }

    pub fn get(&self, id: &str) -> Option<User> {
        self.lock().users.get(id).map(User::from)
    }

    /// Les prénoms affichés sur l'écran de connexion (comme une box TV : on choisit son nom).
    pub fn names(&self) -> Vec<(String, String)> {
        self.list().into_iter().map(|u| (u.id, u.name)).collect()
    }

    pub fn create(&self, name: &str, role: Role, password: &str) -> Result<User, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("le prénom doit faire entre 1 et 40 caractères".into());
        }
        check_password_rules(password)?;
        let id = slugify(name);
        if id.is_empty() {
            return Err("prénom invalide".into());
        }
        let mut inner = self.lock();
        if inner.users.contains_key(&id) {
            return Err(format!("un compte « {name} » existe déjà"));
        }
        // Le premier compte est forcément admin : sinon personne ne pourrait plus rien régler.
        let role = if inner.users.is_empty() {
            Role::Admin
        } else {
            role
        };
        let salt = random_hex(16);
        let user = StoredUser {
            id: id.clone(),
            name: name.to_string(),
            role,
            password: Password {
                hash: hash_password(password, &salt, PBKDF2_ROUNDS),
                salt,
                rounds: PBKDF2_ROUNDS,
            },
            created: Utc::now(),
        };
        self.store.put(Table::Users, &id, &user);
        let view = User::from(&user);
        inner.users.insert(id, user);
        Ok(view)
    }

    fn admins(inner: &Inner) -> usize {
        inner
            .users
            .values()
            .filter(|u| u.role == Role::Admin)
            .count()
    }

    pub fn update(
        &self,
        id: &str,
        name: Option<String>,
        role: Option<Role>,
        password: Option<String>,
    ) -> Result<User, String> {
        let mut inner = self.lock();
        let admins = Self::admins(&inner);
        let user = inner.users.get_mut(id).ok_or("compte introuvable")?;
        if let Some(role) = role {
            if user.role == Role::Admin && role != Role::Admin && admins <= 1 {
                return Err("il faut garder au moins un admin".into());
            }
            user.role = role;
        }
        if let Some(name) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            user.name = name;
        }
        let mut revoke = false;
        if let Some(pw) = password {
            check_password_rules(&pw)?;
            let salt = random_hex(16);
            user.password = Password {
                hash: hash_password(&pw, &salt, PBKDF2_ROUNDS),
                salt,
                rounds: PBKDF2_ROUNDS,
            };
            revoke = true;
        }
        let user = user.clone();
        self.store.put(Table::Users, id, &user);
        if revoke {
            // Nouveau mot de passe : les autres appareils doivent se reconnecter.
            let ids: Vec<String> = inner
                .sessions
                .values()
                .filter(|s| s.user_id == id)
                .map(|s| s.id.clone())
                .collect();
            for sid in ids {
                inner.sessions.remove(&sid);
                self.store.delete(Table::Sessions, &sid);
            }
        }
        Ok(User::from(&user))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut inner = self.lock();
        let Some(user) = inner.users.get(id) else {
            return Err("compte introuvable".into());
        };
        if user.role == Role::Admin && Self::admins(&inner) <= 1 {
            return Err("impossible de supprimer le dernier admin".into());
        }
        inner.users.remove(id);
        self.store.delete(Table::Users, id);
        let ids: Vec<String> = inner
            .sessions
            .values()
            .filter(|s| s.user_id == id)
            .map(|s| s.id.clone())
            .collect();
        for sid in ids {
            inner.sessions.remove(&sid);
            self.store.delete(Table::Sessions, &sid);
        }
        Ok(())
    }

    /// Vérifie le mot de passe et ouvre une session. Renvoie le jeton à mettre dans le cookie.
    /// Volontairement lent (PBKDF2) : à appeler depuis `spawn_blocking`.
    pub fn login(
        &self,
        name: &str,
        password: &str,
        label: Option<String>,
    ) -> Result<(String, User), String> {
        let id = slugify(name);
        let stored = {
            let mut inner = self.lock();
            let now = Instant::now();
            while inner
                .failures
                .front()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(300))
            {
                inner.failures.pop_front();
            }
            if inner.failures.len() >= 10 {
                return Err("trop d'essais : patientez quelques minutes".into());
            }
            inner.users.get(&id).cloned()
        };
        let ok = stored.as_ref().is_some_and(|u| {
            same(
                &hash_password(password, &u.password.salt, u.password.rounds),
                &u.password.hash,
            )
        });
        let Some(user) = stored.filter(|_| ok) else {
            self.lock().failures.push_back(Instant::now());
            return Err("prénom ou mot de passe incorrect".into());
        };
        Ok((self.open_session(&user.id, label), User::from(&user)))
    }

    /// Vérifie le mot de passe d'un compte, sans ouvrir de session (changement de mot de passe).
    pub fn verify(&self, id: &str, password: &str) -> bool {
        let Some(u) = self.lock().users.get(id).cloned() else {
            return false;
        };
        same(
            &hash_password(password, &u.password.salt, u.password.rounds),
            &u.password.hash,
        )
    }

    /// Ouvre une session sans mot de passe (juste après la création du premier compte).
    pub fn open_session(&self, user_id: &str, label: Option<String>) -> String {
        let token = random_hex(32);
        let session = Session {
            id: digest(&token),
            user_id: user_id.to_string(),
            created: Utc::now(),
            label: label.map(|l| l.chars().take(80).collect()),
        };
        let mut inner = self.lock();
        // Pas plus de 20 appareils connectés par compte : on oublie les plus anciens.
        let mut mine: Vec<(DateTime<Utc>, String)> = inner
            .sessions
            .values()
            .filter(|s| s.user_id == user_id)
            .map(|s| (s.created, s.id.clone()))
            .collect();
        mine.sort();
        while mine.len() >= SESSIONS_PER_USER {
            let (_, old) = mine.remove(0);
            inner.sessions.remove(&old);
            self.store.delete(Table::Sessions, &old);
        }
        self.store.put(Table::Sessions, &session.id, &session);
        inner.sessions.insert(session.id.clone(), session);
        token
    }

    pub fn session_user(&self, token: &str) -> Option<User> {
        if token.len() != 64 {
            return None;
        }
        let inner = self.lock();
        let session = inner.sessions.get(&digest(token))?;
        // Une session dure un an (comme le cookie) : au-delà, il faut se reconnecter.
        if Utc::now() - session.created > chrono::TimeDelta::days(SESSION_DAYS) {
            return None;
        }
        inner.users.get(&session.user_id).map(User::from)
    }

    pub fn logout(&self, token: &str) {
        let id = digest(token);
        if self.lock().sessions.remove(&id).is_some() {
            self.store.delete(Table::Sessions, &id);
        }
    }

    pub fn sessions_of(&self, user_id: &str) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self
            .lock()
            .sessions
            .values()
            .filter(|s| s.user_id == user_id)
            .map(|s| SessionInfo {
                id: s.id[..12].to_string(),
                created: s.created,
                label: s.label.clone(),
            })
            .collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.created));
        v
    }
}

/// Lignes lues en base (types opaques pour le reste du programme).
#[derive(Deserialize)]
#[serde(transparent)]
pub struct StoredUserRow(StoredUser);

#[derive(Deserialize)]
#[serde(transparent)]
pub struct SessionRow(Session);

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts(name: &str) -> Arc<Accounts> {
        let dir = std::env::temp_dir().join(format!("heim-acc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (store, _) = crate::store::open(&dir.join("t.db")).unwrap();
        Accounts::new(Vec::new(), Vec::new(), store)
    }

    #[test]
    fn first_account_is_admin_and_login_works() {
        let a = accounts("first");
        assert!(a.is_open());
        let u = a.create("Alex", Role::Guest, "secret123").unwrap();
        assert_eq!(u.role, Role::Admin);
        assert!(!a.is_open());
        assert!(a.login("alex", "mauvais", None).is_err());
        let (token, user) = a.login("Alex", "secret123", Some("iPhone".into())).unwrap();
        assert_eq!(user.id, "alex");
        assert_eq!(a.session_user(&token).unwrap().name, "Alex");
        a.logout(&token);
        assert!(a.session_user(&token).is_none());
    }

    #[test]
    fn keeps_one_admin_and_password_change_logs_out() {
        let a = accounts("admin");
        a.create("Alex", Role::Admin, "secret123").unwrap();
        a.create("Camille", Role::Member, "motdepasse").unwrap();
        assert!(a.update("alex", None, Some(Role::Member), None).is_err());
        assert!(a.delete("alex").is_err());
        let (token, _) = a.login("camille", "motdepasse", None).unwrap();
        a.update("camille", None, None, Some("nouveau123".into()))
            .unwrap();
        assert!(a.session_user(&token).is_none());
        assert!(a.create("Zoé", Role::Guest, "123").is_err());
        assert!(a.create("camille", Role::Guest, "123456").is_err());
    }

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Admin > Role::Member && Role::Member > Role::Guest);
    }
}
