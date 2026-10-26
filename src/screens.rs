//! Écrans : chaque téléphone, tablette ou ordinateur qui ouvre Heim apparaît ici dès qu'il se
//! connecte. L'admin peut, depuis son téléphone, imposer un tableau à une tablette, bloquer
//! la modification, la passer en mode tablette murale… et la tablette suit en direct.
//!
//! L'identifiant d'un écran est choisi par le navigateur : ce n'est pas une protection. Les
//! vraies limites (tableau imposé, modification, lecture seule) se règlent sur le compte avec
//! lequel l'écran est connecté ; celles de l'écran s'y ajoutent, côté interface.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::accounts::Appearance;
use crate::bus::{Bus, Event};
use crate::settings::Settings;

pub const KEY: &str = "screens";
const MAX_SCREENS: usize = 60;
/// On réécrit « vu le » au plus toutes les 6 h : la carte SD n'a pas à le savoir à la minute.
const SEEN_WRITE_EVERY: TimeDelta = TimeDelta::hours(6);

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: String,
    pub name: String,
    /// Ce que le navigateur dit être (« iPad », « Tablette Android »…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Le compte connecté la dernière fois.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    /// Tableau qui s'ouvre sur cet écran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    /// Pas de changement de tableau sur cet écran.
    #[serde(default)]
    pub lock_board: bool,
    #[serde(default = "yes")]
    pub can_edit: bool,
    #[serde(default = "yes")]
    pub can_add: bool,
    /// Tablette murale : sans barres, plein écran.
    #[serde(default)]
    pub kiosk: bool,
    /// Garder l'écran allumé (adresse HTTPS nécessaire sur la plupart des navigateurs).
    #[serde(default)]
    pub keep_awake: bool,
    /// Revenir à son tableau après tant de minutes sans toucher l'écran (0 : jamais).
    #[serde(default)]
    pub return_min: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_tabs: Vec<String>,
    /// Apparence propre à cet écran (sinon celle du compte).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
}

impl Screen {
    fn new(id: &str, name: String, kind: Option<String>) -> Self {
        Screen {
            id: id.to_string(),
            name,
            kind,
            user: None,
            last_seen: None,
            board: None,
            lock_board: false,
            can_edit: true,
            can_add: true,
            kiosk: false,
            keep_awake: false,
            return_min: 0,
            hidden_tabs: Vec::new(),
            appearance: None,
        }
    }
}

/// Ce que l'admin peut changer sur un écran.
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenPatch {
    pub name: Option<String>,
    #[serde(default, with = "double_option")]
    pub board: Option<Option<String>>,
    pub lock_board: Option<bool>,
    pub can_edit: Option<bool>,
    pub can_add: Option<bool>,
    pub kiosk: Option<bool>,
    pub keep_awake: Option<bool>,
    pub return_min: Option<u32>,
    pub hidden_tabs: Option<Vec<String>>,
    #[serde(default, with = "double_option")]
    pub appearance: Option<Option<Appearance>>,
}

/// `absent` = ne pas toucher, `null` = effacer, valeur = remplacer.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// Un identifiant d'écran raisonnable (celui du navigateur : « app_x7k3ab »).
pub fn clean_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then(|| id.to_string())
}

pub struct Screens {
    list: Mutex<BTreeMap<String, Screen>>,
    online: Mutex<HashMap<String, usize>>,
    settings: Arc<Settings>,
    bus: Bus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenView {
    #[serde(flatten)]
    pub screen: Screen,
    pub online: bool,
}

impl Screens {
    pub fn new(settings: Arc<Settings>, bus: Bus) -> Arc<Self> {
        let list: BTreeMap<String, Screen> = settings.get(KEY).unwrap_or_default();
        Arc::new(Screens {
            list: Mutex::new(list),
            online: Mutex::new(HashMap::new()),
            settings,
            bus,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Screen>> {
        self.list.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, list: &BTreeMap<String, Screen>) {
        if let Err(e) = self.settings.set(KEY, list) {
            tracing::warn!("écrans : {e}");
        }
    }

    fn changed(&self, id: &str, identify: bool) {
        self.bus.publish(Event::ScreenChanged {
            id: id.to_string(),
            identify,
        });
    }

    /// Un écran vient de se connecter (WebSocket) : on le note, on le crée la première fois.
    pub fn connected(
        &self,
        id: &str,
        name: Option<String>,
        kind: Option<String>,
        user: Option<String>,
    ) {
        let Some(id) = clean_id(id) else { return };
        *self
            .online
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.clone())
            .or_default() += 1;
        let now = Utc::now();
        let mut list = self.lock();
        if !list.contains_key(&id) && list.len() >= MAX_SCREENS {
            // On oublie l'écran vu il y a le plus longtemps.
            if let Some(old) = list
                .values()
                .min_by_key(|s| s.last_seen)
                .map(|s| s.id.clone())
            {
                list.remove(&old);
            }
        }
        let fresh = !list.contains_key(&id);
        let label = name
            .map(|n| n.trim().chars().take(40).collect::<String>())
            .filter(|n| !n.is_empty());
        let s = list.entry(id.clone()).or_insert_with(|| {
            Screen::new(
                &id,
                label.clone().unwrap_or_else(|| "Écran".into()),
                kind.clone(),
            )
        });
        let write = fresh
            || s.user != user
            || s.kind != kind
            || s.last_seen.is_none_or(|t| now - t > SEEN_WRITE_EVERY);
        s.user = user;
        s.kind = kind;
        s.last_seen = Some(now);
        if write {
            let snapshot = list.clone();
            drop(list);
            self.save(&snapshot);
        }
        self.changed(&id, false);
    }

    pub fn disconnected(&self, id: &str) {
        let Some(id) = clean_id(id) else { return };
        {
            let mut online = self.online.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(n) = online.get_mut(&id) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    online.remove(&id);
                }
            }
        }
        if let Some(s) = self.lock().get_mut(&id) {
            s.last_seen = Some(Utc::now());
        }
        self.changed(&id, false);
    }

    pub fn get(&self, id: &str) -> Option<Screen> {
        self.lock().get(id).cloned()
    }

    pub fn list(&self) -> Vec<ScreenView> {
        let online = self
            .online
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut v: Vec<ScreenView> = self
            .lock()
            .values()
            .map(|s| ScreenView {
                online: online.contains_key(&s.id),
                screen: s.clone(),
            })
            .collect();
        v.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(b.screen.last_seen.cmp(&a.screen.last_seen))
        });
        v
    }

    pub fn update(&self, id: &str, p: ScreenPatch) -> Result<Screen, String> {
        let mut list = self.lock();
        let s = list.get_mut(id).ok_or("écran introuvable")?;
        if let Some(n) = p
            .name
            .map(|n| n.trim().chars().take(40).collect::<String>())
        {
            if n.is_empty() {
                return Err("donnez un nom à cet écran".into());
            }
            s.name = n;
        }
        if let Some(b) = p.board {
            s.board = b.filter(|b| !b.is_empty());
        }
        if let Some(v) = p.lock_board {
            s.lock_board = v;
        }
        if let Some(v) = p.can_edit {
            s.can_edit = v;
        }
        if let Some(v) = p.can_add {
            s.can_add = v;
        }
        if let Some(v) = p.kiosk {
            s.kiosk = v;
        }
        if let Some(v) = p.keep_awake {
            s.keep_awake = v;
        }
        if let Some(v) = p.return_min {
            s.return_min = v.min(240);
        }
        if let Some(mut t) = p.hidden_tabs {
            t.retain(|t| ["appareils", "scenes", "automatismes", "reglages"].contains(&t.as_str()));
            s.hidden_tabs = t;
        }
        if let Some(a) = p.appearance {
            s.appearance = a;
        }
        let out = s.clone();
        let snapshot = list.clone();
        drop(list);
        self.save(&snapshot);
        self.changed(id, false);
        Ok(out)
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut list = self.lock();
        let found = list.remove(id).is_some();
        if found {
            let snapshot = list.clone();
            drop(list);
            self.save(&snapshot);
            self.changed(id, false);
        }
        found
    }

    /// « Identifier » : l'écran affiche son nom en grand quelques secondes.
    pub fn identify(&self, id: &str) -> bool {
        let found = self.lock().contains_key(id);
        if found {
            self.changed(id, true);
        }
        found
    }

    /// Quand un tableau disparaît, les écrans qui l'ouvraient reviennent au premier.
    pub fn forget_board(&self, board: &str) {
        let mut list = self.lock();
        let mut touched = false;
        for s in list.values_mut() {
            if s.board.as_deref() == Some(board) {
                s.board = None;
                touched = true;
            }
        }
        if touched {
            let snapshot = list.clone();
            drop(list);
            self.save(&snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screens(name: &str) -> Arc<Screens> {
        let dir = std::env::temp_dir().join(format!("heim-scr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (store, _) = crate::store::open(&dir.join("t.db")).unwrap();
        let bus = Bus::new(16);
        Screens::new(Settings::new(Vec::new(), store, bus.clone()), bus)
    }

    #[test]
    fn screen_appears_and_can_be_set_up() {
        let s = screens("setup");
        s.connected(
            "app_x7k3",
            Some("iPad".into()),
            Some("iPad".into()),
            Some("Alex".into()),
        );
        assert!(s.list()[0].online);
        assert_eq!(s.list()[0].screen.name, "iPad");
        let patch: ScreenPatch = serde_json::from_value(serde_json::json!({
            "name": "Tablette cuisine", "board": "cuisine", "lock_board": true, "kiosk": true,
            "hidden_tabs": ["reglages", "n'importe"]
        }))
        .unwrap();
        let out = s.update("app_x7k3", patch).unwrap();
        assert_eq!(out.board.as_deref(), Some("cuisine"));
        assert_eq!(out.hidden_tabs, vec!["reglages".to_string()]);
        s.disconnected("app_x7k3");
        assert!(!s.list()[0].online);
        // Effacer le tableau imposé avec null.
        let patch: ScreenPatch =
            serde_json::from_value(serde_json::json!({ "board": null })).unwrap();
        assert_eq!(s.update("app_x7k3", patch).unwrap().board, None);
        s.forget_board("cuisine");
        assert!(s.remove("app_x7k3"));
        assert!(s.list().is_empty());
    }

    #[test]
    fn odd_ids_are_ignored() {
        let s = screens("ids");
        s.connected("../../etc", None, None, None);
        s.connected("", None, None, None);
        assert!(s.list().is_empty());
    }
}
