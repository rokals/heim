//! Notifications.
//!
//! Chaque téléphone ou écran qui ouvre l'application devient une *cible* tout seul.
//! Envoyer « à tout le monde sauf un » s'écrit simplement :
//! `{"message": "...", "except": ["telephone_alex"]}` — sans groupe, sans YAML.
//!
//! Sélecteurs acceptés dans `to` et `except` :
//! - `all` (ou une liste `to` vide) : toutes les cibles ;
//! - `person:<nom>` : tous les appareils d'une personne ;
//! - `<id>` : une cible précise.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::{Bus, Event};
use crate::model::slugify;
use crate::store::{StoreHandle, Table};

const INBOX_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetKind {
    /// L'application Heim (PWA) ouverte sur un téléphone, une tablette, un écran mural.
    /// Avec `push`, elle reçoit aussi les notifications quand elle est fermée.
    App {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        push: Option<crate::push::Subscription>,
    },
    /// Un service tiers appelé en HTTP POST (ntfy, Gotify, Discord, n8n...).
    Webhook { url: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotifyTarget {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    #[serde(flatten)]
    pub kind: TargetKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Info,
    Important,
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    pub level: Level,
    /// Action proposée, libre : ouvrir une carte, activer une scène...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyRequest {
    #[serde(default)]
    pub title: Option<String>,
    pub message: String,
    #[serde(default)]
    pub level: Level,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub except: Vec<String>,
    #[serde(default)]
    pub action: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendReport {
    pub id: u64,
    pub recipients: Vec<String>,
    /// Applications connectées à cet instant (les autres la verront dans leur boîte à la reconnexion).
    pub delivered_live: Vec<String>,
    pub webhooks: Vec<String>,
    /// Téléphones prévenus par notification push (même application fermée).
    pub pushed: Vec<String>,
}

struct InboxEntry {
    notification: Arc<Notification>,
    recipients: BTreeSet<String>,
}

#[derive(Default)]
struct Inner {
    targets: BTreeMap<String, NotifyTarget>,
    online: HashMap<String, usize>,
    inbox: VecDeque<InboxEntry>,
}

pub struct Notifier {
    inner: Mutex<Inner>,
    vapid: Mutex<Option<crate::push::Vapid>>,
    next_id: AtomicU64,
    bus: Bus,
    store: StoreHandle,
    #[cfg(feature = "webhook")]
    http: reqwest::Client,
}

impl Notifier {
    pub fn new(targets: Vec<NotifyTarget>, bus: Bus, store: StoreHandle) -> Arc<Self> {
        let inner = Inner {
            targets: targets.into_iter().map(|t| (t.id.clone(), t)).collect(),
            ..Default::default()
        };
        Arc::new(Self {
            inner: Mutex::new(inner),
            vapid: Mutex::new(None),
            next_id: AtomicU64::new(Utc::now().timestamp_millis() as u64),
            bus,
            store,
            #[cfg(feature = "webhook")]
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("client HTTP"),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // Un verrou empoisonné ne doit jamais faire tomber les notifications.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn targets(&self) -> Vec<NotifyTarget> {
        self.lock().targets.values().cloned().collect()
    }

    pub fn upsert_target(
        &self,
        raw_id: &str,
        name: String,
        person: Option<String>,
        kind: TargetKind,
    ) -> Result<NotifyTarget, String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("la cible doit avoir un nom".into());
        }
        let id = slugify(if raw_id.is_empty() { &name } else { raw_id });
        if id.is_empty() || id == "all" {
            return Err("identifiant de cible invalide".into());
        }
        if let TargetKind::Webhook { url } = &kind
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err("l'URL du webhook doit commencer par http:// ou https://".into());
        }
        let person = person.map(|p| slugify(&p)).filter(|p| !p.is_empty());
        let target = NotifyTarget {
            id: id.clone(),
            name,
            person,
            kind,
        };
        self.store.put(Table::NotifyTargets, &id, &target);
        self.lock().targets.insert(id, target.clone());
        Ok(target)
    }

    pub fn delete_target(&self, id: &str) -> bool {
        let existed = self.lock().targets.remove(id).is_some();
        if existed {
            self.store.delete(Table::NotifyTargets, id);
        }
        existed
    }

    /// Une application s'annonce. Si elle est nouvelle, elle devient une cible automatiquement.
    /// `person` : le compte connecté sur cet appareil. L'appareil lui est rattaché, pour que
    /// « person:alex » atteigne tous ses téléphones.
    pub fn app_connected(
        &self,
        raw_id: &str,
        name: Option<String>,
        person: Option<String>,
    ) -> Option<String> {
        let id = slugify(raw_id);
        if id.is_empty() {
            return None;
        }
        let existing = self.lock().targets.get(&id).cloned();
        match existing {
            None => {
                let name = name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| id.clone());
                if let Err(e) =
                    self.upsert_target(&id, name, person, TargetKind::App { push: None })
                {
                    tracing::warn!("notifications : cible {id} refusée : {e}");
                    return None;
                }
                tracing::info!("notifications : nouvel appareil enregistré ({id})");
            }
            Some(t) if person.is_some() && t.person != person => {
                let _ = self.upsert_target(&id, t.name, person, t.kind);
            }
            Some(_) => {}
        }
        *self.lock().online.entry(id.clone()).or_insert(0) += 1;
        Some(id)
    }

    pub fn app_disconnected(&self, id: &str) {
        let mut inner = self.lock();
        if let Some(count) = inner.online.get_mut(id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                inner.online.remove(id);
            }
        }
    }

    fn expand(targets: &BTreeMap<String, NotifyTarget>, selector: &str) -> Vec<String> {
        let selector = selector.trim();
        if selector == "all" {
            targets.keys().cloned().collect()
        } else if let Some(person) = selector.strip_prefix("person:") {
            let person = slugify(person);
            targets
                .values()
                .filter(|t| t.person.as_deref() == Some(person.as_str()))
                .map(|t| t.id.clone())
                .collect()
        } else {
            let id = slugify(selector);
            targets
                .contains_key(&id)
                .then_some(id)
                .into_iter()
                .collect()
        }
    }

    pub fn resolve(&self, to: &[String], except: &[String]) -> Vec<String> {
        let inner = self.lock();
        let mut chosen: BTreeSet<String> = if to.is_empty() {
            inner.targets.keys().cloned().collect()
        } else {
            to.iter()
                .flat_map(|s| Self::expand(&inner.targets, s))
                .collect()
        };
        for s in except {
            for id in Self::expand(&inner.targets, s) {
                chosen.remove(&id);
            }
        }
        chosen.into_iter().collect()
    }

    pub fn send(&self, req: NotifyRequest) -> Result<SendReport, String> {
        let message = req.message.trim().to_string();
        if message.is_empty() {
            return Err("le message est vide".into());
        }
        let recipients = self.resolve(&req.to, &req.except);
        if recipients.is_empty() {
            return Err("aucun destinataire ne correspond".into());
        }
        let notification = Arc::new(Notification {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            title: req.title.filter(|t| !t.trim().is_empty()),
            message,
            level: req.level,
            action: req.action,
            created: Utc::now(),
        });

        let mut delivered_live = Vec::new();
        let mut webhooks = Vec::new();
        let mut pushes = Vec::new();
        {
            let mut inner = self.lock();
            if inner.inbox.len() >= INBOX_SIZE {
                inner.inbox.pop_front();
            }
            inner.inbox.push_back(InboxEntry {
                notification: notification.clone(),
                recipients: recipients.iter().cloned().collect(),
            });
            for id in &recipients {
                match inner.targets.get(id).map(|t| &t.kind) {
                    Some(TargetKind::App { push }) => {
                        if inner.online.contains_key(id) {
                            delivered_live.push(id.clone());
                        }
                        if let Some(sub) = push {
                            pushes.push((id.clone(), sub.clone()));
                        }
                    }
                    Some(TargetKind::Webhook { url }) => webhooks.push((id.clone(), url.clone())),
                    None => {}
                }
            }
        }

        for id in &recipients {
            self.bus.publish(Event::Notification {
                target_id: id.clone(),
                notification: notification.clone(),
            });
        }
        let webhook_ids = webhooks.iter().map(|(id, _)| id.clone()).collect();
        for (id, url) in webhooks {
            self.post_webhook(id, url, notification.clone());
        }
        let pushed: Vec<String> = pushes.iter().map(|(id, _)| id.clone()).collect();
        if !pushes.is_empty() {
            self.push_all(pushes, &notification);
        }
        Ok(SendReport {
            id: notification.id,
            recipients,
            delivered_live,
            webhooks: webhook_ids,
            pushed,
        })
    }

    pub fn set_vapid(&self, vapid: crate::push::Vapid) {
        *self.vapid.lock().unwrap_or_else(|e| e.into_inner()) = Some(vapid);
    }

    pub fn vapid_public(&self) -> Option<String> {
        self.vapid
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|v| v.public.clone())
    }

    /// Le téléphone accepte les notifications push : on range son abonnement avec sa cible.
    pub fn subscribe(
        &self,
        target_id: &str,
        sub: crate::push::Subscription,
    ) -> Result<NotifyTarget, String> {
        sub.validate()?;
        let id = slugify(target_id);
        let target = self
            .lock()
            .targets
            .get(&id)
            .cloned()
            .ok_or("appareil inconnu : rechargez la page")?;
        if !matches!(target.kind, TargetKind::App { .. }) {
            return Err("seule l'application peut recevoir des notifications push".into());
        }
        self.upsert_target(
            &id,
            target.name,
            target.person,
            TargetKind::App { push: Some(sub) },
        )
    }

    /// À qui est rattaché cet appareil (None : à personne, ou appareil inconnu).
    pub fn target_owner(&self, target_id: &str) -> Option<String> {
        self.lock()
            .targets
            .get(&slugify(target_id))
            .and_then(|t| t.person.clone())
    }

    pub fn unsubscribe(&self, target_id: &str) {
        let id = slugify(target_id);
        let target = self.lock().targets.get(&id).cloned();
        if let Some(t) = target
            && matches!(t.kind, TargetKind::App { push: Some(_) })
        {
            let _ = self.upsert_target(&id, t.name, t.person, TargetKind::App { push: None });
        }
    }

    #[cfg(feature = "webhook")]
    fn push_all(&self, pushes: Vec<(String, crate::push::Subscription)>, n: &Notification) {
        let Some(vapid) = self.vapid.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            return;
        };
        let level = match n.level {
            Level::Info => "info",
            Level::Important => "important",
            Level::Urgent => "urgent",
        };
        let url = match n
            .action
            .as_ref()
            .and_then(|a| a.get("open"))
            .and_then(Value::as_str)
        {
            Some("security") => "/",
            Some(other) if other.starts_with('#') => other,
            _ => "/",
        };
        // Sur l'écran verrouillé, dans la langue de la maison.
        let lang = crate::i18n::house_lang();
        let title = n
            .title
            .as_deref()
            .map(|t| crate::i18n::tr(&lang, t).into_owned());
        let payload = crate::push::payload(
            title.as_deref().unwrap_or("Heim"),
            &crate::i18n::tr(&lang, &n.message),
            level,
            url,
            None,
        );
        let urgent = n.level != Level::Info;
        for (id, sub) in pushes {
            let http = self.http.clone();
            let vapid = vapid.clone();
            let payload = payload.clone();
            let gone_tx = self.bus.clone();
            tokio::spawn(async move {
                match crate::push::send(&http, &vapid, &sub, &payload, urgent).await {
                    Ok(()) => {}
                    Err(crate::push::PushError::Gone) => {
                        tracing::info!("push : abonnement de {id} expiré, oublié");
                        gone_tx.publish(Event::PushExpired { target_id: id });
                    }
                    Err(crate::push::PushError::Other(e)) => tracing::warn!("push {id} : {e}"),
                }
            });
        }
    }

    #[cfg(not(feature = "webhook"))]
    fn push_all(&self, _pushes: Vec<(String, crate::push::Subscription)>, _n: &Notification) {}

    /// Envoie un essai à un seul appareil et attend la réponse du service push.
    #[cfg(feature = "webhook")]
    pub async fn push_test(&self, target_id: &str) -> Result<(), String> {
        let id = slugify(target_id);
        let sub = match self.lock().targets.get(&id).map(|t| t.kind.clone()) {
            Some(TargetKind::App { push: Some(sub) }) => sub,
            _ => return Err("cet appareil n'est pas abonné aux notifications".into()),
        };
        let vapid = self
            .vapid
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or("clés push absentes")?;
        let payload = crate::push::payload(
            "Heim",
            &crate::i18n::tr(
                &crate::i18n::house_lang(),
                "Les notifications marchent sur cet appareil.",
            ),
            "info",
            "/",
            Some("test"),
        );
        match crate::push::send(&self.http, &vapid, &sub, &payload, false).await {
            Ok(()) => Ok(()),
            Err(crate::push::PushError::Gone) => {
                self.unsubscribe(&id);
                Err("l'abonnement n'est plus valable : réactivez les notifications".into())
            }
            Err(crate::push::PushError::Other(e)) => Err(e),
        }
    }

    #[cfg(not(feature = "webhook"))]
    pub async fn push_test(&self, _target_id: &str) -> Result<(), String> {
        Err("Heim compilé sans accès HTTP".into())
    }

    #[cfg(feature = "webhook")]
    fn post_webhook(&self, id: String, url: String, notification: Arc<Notification>) {
        let http = self.http.clone();
        tokio::spawn(async move {
            match http.post(&url).json(&*notification).send().await {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => tracing::warn!("webhook {id} : réponse {}", r.status()),
                Err(e) => tracing::warn!("webhook {id} : {e}"),
            }
        });
    }

    #[cfg(not(feature = "webhook"))]
    fn post_webhook(&self, id: String, _url: String, _n: Arc<Notification>) {
        tracing::warn!("webhook {id} ignoré : Heim compilé sans la fonctionnalité « webhook »");
    }

    /// Notifications destinées à une cible, plus récentes que `after_id`.
    pub fn inbox(&self, target: &str, after_id: u64) -> Vec<Arc<Notification>> {
        let target = slugify(target);
        self.lock()
            .inbox
            .iter()
            .filter(|e| e.notification.id > after_id && e.recipients.contains(&target))
            .map(|e| e.notification.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notifier(name: &str) -> Arc<Notifier> {
        let dir = std::env::temp_dir().join(format!("heim-notify-{name}-{}", std::process::id()));
        let (store, _) = crate::store::open(&dir.join("t.db")).unwrap();
        let n = Notifier::new(Vec::new(), Bus::new(16), store);
        n.upsert_target(
            "tel_alex",
            "Téléphone Alex".into(),
            Some("alex".into()),
            TargetKind::App { push: None },
        )
        .unwrap();
        n.upsert_target(
            "tablette_alex",
            "Tablette".into(),
            Some("alex".into()),
            TargetKind::App { push: None },
        )
        .unwrap();
        n.upsert_target(
            "tel_camille",
            "Téléphone Camille".into(),
            Some("camille".into()),
            TargetKind::App { push: None },
        )
        .unwrap();
        n.upsert_target(
            "ecran_cuisine",
            "Écran cuisine".into(),
            None,
            TargetKind::App { push: None },
        )
        .unwrap();
        n
    }

    #[tokio::test]
    async fn all_but_one() {
        let n = notifier("a");
        let r = n.resolve(&[], &["ecran_cuisine".into()]);
        assert_eq!(r, vec!["tablette_alex", "tel_alex", "tel_camille"]);
    }

    #[tokio::test]
    async fn person_selector() {
        let n = notifier("b");
        assert_eq!(
            n.resolve(&["person:alex".into()], &[]),
            vec!["tablette_alex", "tel_alex"]
        );
        assert_eq!(
            n.resolve(&["all".into()], &["person:alex".into()]),
            vec!["ecran_cuisine", "tel_camille"]
        );
    }

    #[tokio::test]
    async fn send_and_inbox() {
        let n = notifier("c");
        let report = n
            .send(NotifyRequest {
                title: None,
                message: "Linge terminé".into(),
                level: Level::Info,
                to: vec![],
                except: vec!["tel_camille".into()],
                action: None,
            })
            .unwrap();
        assert_eq!(report.recipients.len(), 3);
        assert_eq!(n.inbox("tel_alex", 0).len(), 1);
        assert!(n.inbox("tel_camille", 0).is_empty());
        assert!(
            n.send(NotifyRequest {
                title: None,
                message: "  ".into(),
                level: Level::Info,
                to: vec![],
                except: vec![],
                action: None
            })
            .is_err()
        );
    }
}
