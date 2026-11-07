//! Centre de notifications : la cloche en haut de l'écran.
//!
//! Tout ce que Heim a à dire y reste, même sans téléphone abonné : mises à jour, plugins
//! arrêtés, mémoire / carte SD / température de la box, appareils (piles, muets, connexions),
//! alarme, automatismes (« prévenir »), présence, coupures de courant, et les alertes de
//! consommation choisies par l'utilisateur (« la prise du chauffe-eau dépasse 2 000 W depuis
//! 10 min »).
//!
//! Une note peut porter une **clé** : c'est alors une situation qui dure (« mémoire à 92 % ») ;
//! la reposer la met à jour au lieu d'en ajouter une, et elle passe « réglée » quand la
//! situation cesse. Chaque compte a sa date de lecture : les notes plus récentes sont « non
//! lues » (le chiffre sur la cloche).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event};
use crate::notify::Level;
use crate::settings::Settings;
use crate::store::{StoreHandle, Table};

pub const SETTINGS_KEY: &str = "center";
const SEEN_KEY: &str = "center.seen";
const MAX_NOTES: usize = 300;
const KEEP_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Update,
    Plugin,
    System,
    Device,
    Automation,
    Security,
    Presence,
    Energy,
    #[default]
    Message,
}

impl Kind {
    pub fn parse(s: &str) -> Kind {
        match s {
            "update" => Kind::Update,
            "plugin" => Kind::Plugin,
            "system" => Kind::System,
            "device" => Kind::Device,
            "automation" => Kind::Automation,
            "security" => Kind::Security,
            "presence" => Kind::Presence,
            "energy" => Kind::Energy,
            _ => Kind::Message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub kind: Kind,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    /// Où aller en touchant la note (« #reglages/systeme »).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    pub first: DateTime<Utc>,
    /// Dernière fois qu'elle a été posée (ou mise à jour) : sert au « non lu ».
    pub at: DateTime<Utc>,
    /// Situation terminée (« revenu à la normale »).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Réservée aux admins (mises à jour, plugins, box).
    #[serde(default)]
    pub admin: bool,
    /// Réservée à ces comptes (message adressé à quelqu'un) ; vide : tout le monde.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// Une note à poser.
#[derive(Debug, Clone, Default)]
pub struct NewNote {
    pub key: Option<String>,
    pub kind: Kind,
    pub level: Level,
    pub title: Option<String>,
    pub message: String,
    pub open: Option<String>,
    pub admin: bool,
    pub only: Vec<String>,
}

/// Qui regarde : son compte (aucun quand Heim est sans compte) et s'il est admin.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub account: Option<String>,
    pub admin: bool,
}

impl Viewer {
    fn key(&self) -> String {
        self.account.clone().unwrap_or_else(|| "_open".into())
    }

    fn sees(&self, n: &Note) -> bool {
        if self.admin || self.account.is_none() {
            return true;
        }
        !n.admin && (n.only.is_empty() || self.account.as_ref().is_some_and(|a| n.only.contains(a)))
    }
}

fn d_mem_warn() -> u8 {
    85
}
fn d_mem_crit() -> u8 {
    95
}
fn d_temp_warn() -> u8 {
    80
}
fn d_temp_crit() -> u8 {
    85
}
fn yes() -> bool {
    true
}
fn d_for() -> u32 {
    5
}

/// Une alerte de consommation (ou de n'importe quelle mesure) : au-dessus de `above` depuis
/// `for_min` minutes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnergyRule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub entity_id: String,
    pub above: f64,
    #[serde(default = "d_for")]
    pub for_min: u32,
    /// Prévenir aussi les téléphones.
    #[serde(default = "yes")]
    pub push: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CenterSettings {
    #[serde(default = "d_mem_warn")]
    pub memory_warn: u8,
    #[serde(default = "d_mem_crit")]
    pub memory_crit: u8,
    #[serde(default = "d_mem_warn")]
    pub disk_warn: u8,
    #[serde(default = "d_mem_crit")]
    pub disk_crit: u8,
    #[serde(default = "d_temp_warn")]
    pub temp_warn: u8,
    #[serde(default = "d_temp_crit")]
    pub temp_crit: u8,
    /// Les situations critiques de la box partent aussi sur les téléphones des admins.
    #[serde(default = "yes")]
    pub push_critical: bool,
    #[serde(default)]
    pub energy: Vec<EnergyRule>,
}

impl Default for CenterSettings {
    fn default() -> Self {
        CenterSettings {
            memory_warn: d_mem_warn(),
            memory_crit: d_mem_crit(),
            disk_warn: d_mem_warn(),
            disk_crit: d_mem_crit(),
            temp_warn: d_temp_warn(),
            temp_crit: d_temp_crit(),
            push_critical: true,
            energy: Vec::new(),
        }
    }
}

impl CenterSettings {
    pub fn validate(&self) -> Result<(), String> {
        let pair = |w: u8, c: u8, what: &str| {
            if !(10..=100).contains(&w) || !(10..=100).contains(&c) || w > c {
                Err(format!(
                    "{what} : seuils invalides (alerte ≤ critique, entre 10 et 100)"
                ))
            } else {
                Ok(())
            }
        };
        pair(self.memory_warn, self.memory_crit, "mémoire")?;
        pair(self.disk_warn, self.disk_crit, "carte SD")?;
        pair(self.temp_warn, self.temp_crit, "température")?;
        if self.energy.len() > 50 {
            return Err("50 alertes de consommation au plus".into());
        }
        for r in &self.energy {
            if !r.entity_id.contains('.') {
                return Err("alerte de consommation : choisissez une mesure".into());
            }
            if r.for_min > 24 * 60 {
                return Err("alerte de consommation : durée trop longue (24 h au plus)".into());
            }
        }
        Ok(())
    }
}

struct Inner {
    notes: VecDeque<Note>,
    next: u64,
}

pub struct Center {
    inner: Mutex<Inner>,
    store: StoreHandle,
    bus: Bus,
    settings: Arc<Settings>,
}

impl Center {
    pub fn new(
        mut notes: Vec<Note>,
        store: StoreHandle,
        bus: Bus,
        settings: Arc<Settings>,
    ) -> Arc<Center> {
        notes.sort_by_key(|n| std::cmp::Reverse(n.at));
        let next = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let c = Arc::new(Center {
            inner: Mutex::new(Inner {
                notes: notes.into(),
                next,
            }),
            store,
            bus,
            settings,
        });
        c.prune();
        c
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.bus.publish(Event::CenterChanged);
    }

    /// Pose une note ; avec une clé déjà active, la met à jour (et la remonte en tête).
    pub fn post(&self, new: NewNote) -> Note {
        let now = Utc::now();
        let note = {
            let mut inner = self.lock();
            let existing = new.key.as_ref().and_then(|k| {
                inner
                    .notes
                    .iter()
                    .position(|n| n.key.as_ref() == Some(k) && n.resolved.is_none())
            });
            let note = match existing {
                Some(i) => {
                    let mut n = inner.notes.remove(i).expect("présente");
                    n.level = new.level;
                    n.title = new.title;
                    n.message = new.message;
                    n.open = new.open.or(n.open);
                    n.at = now;
                    n
                }
                None => {
                    let id = inner.next;
                    inner.next += 1;
                    Note {
                        id,
                        key: new.key,
                        kind: new.kind,
                        level: new.level,
                        title: new.title,
                        message: new.message,
                        open: new.open,
                        first: now,
                        at: now,
                        resolved: None,
                        resolution: None,
                        admin: new.admin,
                        only: new.only,
                    }
                }
            };
            inner.notes.push_front(note.clone());
            note
        };
        self.store.put(Table::Notes, &note.id.to_string(), &note);
        self.prune();
        self.changed();
        note
    }

    /// La situation `key` a cessé.
    pub fn resolve(&self, key: &str, resolution: Option<String>) {
        let note = {
            let mut inner = self.lock();
            let Some(n) = inner
                .notes
                .iter_mut()
                .find(|n| n.key.as_deref() == Some(key) && n.resolved.is_none())
            else {
                return;
            };
            n.resolved = Some(Utc::now());
            n.resolution = resolution;
            n.clone()
        };
        self.store.put(Table::Notes, &note.id.to_string(), &note);
        self.changed();
    }

    pub fn is_active(&self, key: &str) -> bool {
        self.lock()
            .notes
            .iter()
            .any(|n| n.key.as_deref() == Some(key) && n.resolved.is_none())
    }

    fn prune(&self) {
        let limit = Utc::now() - TimeDelta::days(KEEP_DAYS);
        let removed: Vec<u64> = {
            let mut inner = self.lock();
            let mut removed = Vec::new();
            let mut kept = VecDeque::with_capacity(inner.notes.len());
            for n in inner.notes.drain(..) {
                // Une situation encore en cours n'est jamais oubliée.
                let old = n.at < limit && n.resolved.is_some();
                if old || (kept.len() >= MAX_NOTES && n.resolved.is_some()) {
                    removed.push(n.id);
                } else {
                    kept.push_back(n);
                }
            }
            inner.notes = kept;
            removed
        };
        for id in removed {
            self.store.delete(Table::Notes, &id.to_string());
        }
    }

    fn seen_map(&self) -> HashMap<String, DateTime<Utc>> {
        self.settings.get(SEEN_KEY).unwrap_or_default()
    }

    pub fn seen_at(&self, viewer: &Viewer) -> Option<DateTime<Utc>> {
        self.seen_map().get(&viewer.key()).copied()
    }

    pub fn list(&self, viewer: &Viewer) -> Vec<Note> {
        self.lock()
            .notes
            .iter()
            .filter(|n| viewer.sees(n))
            .cloned()
            .collect()
    }

    pub fn unread(&self, viewer: &Viewer) -> usize {
        let seen = self.seen_at(viewer);
        self.lock()
            .notes
            .iter()
            .filter(|n| viewer.sees(n) && seen.is_none_or(|s| n.at > s))
            .count()
    }

    pub fn mark_seen(&self, viewer: &Viewer) {
        let mut map = self.seen_map();
        map.insert(viewer.key(), Utc::now());
        let _ = self.settings.set(SEEN_KEY, &map);
        self.changed();
    }

    /// Retire une note (ou toutes les notes réglées / d'information, `id` absent).
    pub fn dismiss(&self, id: Option<u64>, viewer: &Viewer) -> usize {
        let removed: Vec<u64> = {
            let mut inner = self.lock();
            let before: Vec<u64> = inner.notes.iter().map(|n| n.id).collect();
            inner.notes.retain(|n| {
                let target = match id {
                    Some(id) => n.id == id,
                    // « Tout effacer » garde les situations en cours (clé non réglée).
                    None => n.key.is_none() || n.resolved.is_some(),
                };
                !(target && viewer.sees(n))
            });
            let after: Vec<u64> = inner.notes.iter().map(|n| n.id).collect();
            before.into_iter().filter(|i| !after.contains(i)).collect()
        };
        for i in &removed {
            self.store.delete(Table::Notes, &i.to_string());
        }
        if !removed.is_empty() {
            self.changed();
        }
        removed.len()
    }
}

// ---- Surveillance : box, plugins, consommation -------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Band {
    Ok,
    Warn,
    Crit,
}

/// Le niveau d'une mesure, avec 5 points d'écart pour revenir (pas de va-et-vient).
fn band(value: f64, warn: u8, crit: u8, current: Band) -> Band {
    let (w, c) = (warn as f64, crit as f64);
    if value >= c {
        Band::Crit
    } else if value >= w {
        if current == Band::Crit && value >= c - 5.0 {
            Band::Crit
        } else {
            Band::Warn
        }
    } else if current != Band::Ok && value >= w - 5.0 {
        Band::Warn.min(current)
    } else {
        Band::Ok
    }
}

fn mb(bytes: u64) -> String {
    format!("{} Mo", bytes / (1024 * 1024))
}

pub struct Watch {
    pub center: Arc<Center>,
    pub notify: Arc<crate::notify::Notifier>,
    pub settings: Arc<Settings>,
    pub sampler: Arc<crate::sysstats::Sampler>,
    pub plugins: Arc<crate::plugins::Plugins>,
    pub registry: crate::registry::RegistryHandle,
    pub accounts: Arc<crate::accounts::Accounts>,
    pub bus: Bus,
}

impl Watch {
    fn conf(&self) -> CenterSettings {
        self.settings.get(SETTINGS_KEY).unwrap_or_default()
    }

    /// Pose la note ; si elle est critique (et que c'est voulu), prévient aussi les admins.
    fn raise(&self, note: NewNote, push: bool) {
        if push {
            let to = self
                .accounts
                .list()
                .into_iter()
                .filter(|u| u.role == crate::accounts::Role::Admin)
                .map(|u| format!("person:{}", u.id))
                .collect();
            // Le notifieur la pose lui-même dans le centre (avec la clé) : pas de doublon.
            let sent = self.notify.send(crate::notify::NotifyRequest {
                title: note.title.clone(),
                message: note.message.clone(),
                level: note.level,
                to,
                kind: Some(note.kind),
                key: note.key.clone(),
                open: note.open.clone(),
                admin: note.admin,
                ..Default::default()
            });
            if sent.is_ok() {
                return;
            }
        }
        self.center.post(note);
    }
}

pub fn spawn(w: Watch) {
    let w = Arc::new(w);
    // La box et les plugins : chaque minute.
    let box_w = w.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(90)).await;
        let mut bands: HashMap<&'static str, Band> = HashMap::new();
        let mut restarts: HashMap<String, u32> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            check_box(&box_w, &mut bands);
            check_plugins(&box_w, &mut restarts);
        }
    });
    // Les alertes de consommation : à chaque nouvelle valeur, et chaque minute pour les durées.
    tokio::spawn(async move {
        let mut rx = w.bus.subscribe();
        let mut over: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut values: HashMap<String, f64> = HashMap::new();
        for e in w.registry.snapshot().await {
            if let Some(v) = e.state.and_then(|s| s.value.as_f64()) {
                values.insert(e.meta.entity_id, v);
            }
        }
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, new, .. }) => {
                        if let Some(v) = new.value.as_f64() {
                            values.insert(entity_id, v);
                        } else {
                            values.remove(&entity_id);
                        }
                    }
                    Err(RecvError::Closed) => return,
                    _ => continue,
                },
                _ = tick.tick() => {}
            }
            check_energy(&w, &values, &mut over).await;
        }
    });
}

fn check_box(w: &Watch, bands: &mut HashMap<&'static str, Band>) {
    let c = w.conf();
    let snap = w.sampler.last();
    let top = snap
        .processes
        .iter()
        .max_by_key(|p| p.memory)
        .map(|p| format!(" Le plus gourmand : {} ({}).", p.name, mb(p.memory)))
        .unwrap_or_default();
    let checks: [(&'static str, Option<f64>, u8, u8); 3] = [
        (
            "memory",
            snap.memory.as_ref().map(|m| m.percent),
            c.memory_warn,
            c.memory_crit,
        ),
        (
            "disk",
            snap.disk.as_ref().map(|d| d.percent),
            c.disk_warn,
            c.disk_crit,
        ),
        ("temperature", snap.temperature, c.temp_warn, c.temp_crit),
    ];
    for (what, value, warn, crit) in checks {
        let Some(v) = value else { continue };
        let was = bands.get(what).copied().unwrap_or(Band::Ok);
        let now = band(v, warn, crit, was);
        bands.insert(what, now);
        let key = format!("system.{what}");
        if now == Band::Ok && !w.center.is_active(&key) {
            continue;
        }
        if now == was && now != Band::Ok && w.center.is_active(&key) {
            continue;
        }
        if now == Band::Ok {
            let back = match what {
                "temperature" => format!("Revenue à {v:.0} °C."),
                _ => format!("Revenue à {v:.0} %."),
            };
            w.center.resolve(&key, Some(back));
            continue;
        }
        let (title, message) = match what {
            "memory" => (
                "Mémoire de la box presque pleine",
                format!("Mémoire utilisée à {v:.0} %.{top}"),
            ),
            "disk" => (
                "Carte SD presque pleine",
                format!(
                    "Carte SD remplie à {v:.0} %{}.",
                    snap.disk
                        .as_ref()
                        .map(|d| format!(" (reste {})", mb(d.total.saturating_sub(d.used))))
                        .unwrap_or_default()
                ),
            ),
            _ => (
                "La box chauffe",
                format!("Processeur à {v:.0} °C : vérifiez l'aération du boîtier."),
            ),
        };
        let crit_now = now == Band::Crit;
        // Seulement en montant : redescendre de critique à alerte ne renvoie rien au téléphone.
        let push = crit_now && was != Band::Crit && c.push_critical;
        w.raise(
            NewNote {
                key: Some(key),
                kind: Kind::System,
                level: if crit_now {
                    Level::Urgent
                } else {
                    Level::Important
                },
                title: Some(title.into()),
                message,
                open: Some("#reglages/systeme".into()),
                admin: true,
                only: Vec::new(),
            },
            push,
        );
    }
}

fn check_plugins(w: &Watch, seen: &mut HashMap<String, u32>) {
    let installed: crate::plugins::PluginsSettings = w
        .settings
        .get(crate::plugins::SETTINGS_KEY)
        .unwrap_or_default();
    for (id, p) in &installed.installed {
        let Some(st) = w.plugins.state(id) else {
            continue;
        };
        let before = seen.insert(id.clone(), st.restarts);
        if before.is_some_and(|b| st.restarts > b) {
            let name = p
                .manifest
                .as_ref()
                .map(|m| m.display_name("fr"))
                .unwrap_or_else(|| id.clone());
            let why = st
                .last_exit
                .clone()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default();
            w.center.post(NewNote {
                key: Some(format!("plugin.{id}")),
                kind: Kind::Plugin,
                level: Level::Important,
                title: Some(format!("Plugin « {name} » arrêté")),
                message: format!(
                    "Il s'est arrêté{why} et Heim l'a relancé ({} fois depuis le démarrage).",
                    st.restarts
                ),
                open: Some("#reglages/integrations".into()),
                admin: true,
                only: Vec::new(),
            });
        }
    }
}

async fn check_energy(
    w: &Watch,
    values: &HashMap<String, f64>,
    over: &mut HashMap<String, DateTime<Utc>>,
) {
    let c = w.conf();
    let now = Utc::now();
    over.retain(|id, _| c.energy.iter().any(|r| &r.id == id));
    for r in &c.energy {
        let key = format!("energy.{}", r.id);
        let v = values.get(&r.entity_id).copied();
        let above = v.is_some_and(|v| v > r.above);
        if !above {
            over.remove(&r.id);
            if w.center.is_active(&key) {
                w.center.resolve(
                    &key,
                    Some(match v {
                        Some(v) => format!("Revenu à {v}."),
                        None => "Plus de mesure.".into(),
                    }),
                );
            }
            continue;
        }
        let since = *over.entry(r.id.clone()).or_insert(now);
        if now - since < TimeDelta::minutes(r.for_min as i64) || w.center.is_active(&key) {
            continue;
        }
        let entity = w.registry.get(&r.entity_id).await;
        let unit = entity
            .as_ref()
            .and_then(|e| {
                e.meta
                    .capabilities
                    .get("unit")
                    .and_then(|u| u.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let name = if r.name.trim().is_empty() {
            entity
                .map(|e| e.meta.name)
                .unwrap_or_else(|| r.entity_id.clone())
        } else {
            r.name.clone()
        };
        let duration = if r.for_min > 0 {
            format!(" depuis {} min", r.for_min)
        } else {
            String::new()
        };
        w.raise(
            NewNote {
                key: Some(key),
                kind: Kind::Energy,
                level: Level::Important,
                title: Some(format!("Consommation : {name}")),
                message: format!(
                    "{} {unit} (seuil {} {unit}){duration}.",
                    v.unwrap_or_default(),
                    r.above
                )
                .replace("  ", " "),
                open: None,
                admin: false,
                only: Vec::new(),
            },
            r.push,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_have_hysteresis() {
        assert_eq!(band(50.0, 85, 95, Band::Ok), Band::Ok);
        assert_eq!(band(86.0, 85, 95, Band::Ok), Band::Warn);
        assert_eq!(band(96.0, 85, 95, Band::Warn), Band::Crit);
        // Redescendre un peu sous le seuil ne fait pas encore « revenu à la normale ».
        assert_eq!(band(93.0, 85, 95, Band::Crit), Band::Crit);
        assert_eq!(band(82.0, 85, 95, Band::Warn), Band::Warn);
        assert_eq!(band(79.0, 85, 95, Band::Warn), Band::Ok);
    }

    #[test]
    fn viewers_and_settings() {
        let n = |admin: bool, only: Vec<String>| Note {
            id: 1,
            key: None,
            kind: Kind::System,
            level: Level::Info,
            title: None,
            message: "x".into(),
            open: None,
            first: Utc::now(),
            at: Utc::now(),
            resolved: None,
            resolution: None,
            admin,
            only,
        };
        let member = Viewer {
            account: Some("alex".into()),
            admin: false,
        };
        assert!(!member.sees(&n(true, vec![])));
        assert!(member.sees(&n(false, vec![])));
        assert!(member.sees(&n(false, vec!["alex".into()])));
        assert!(!member.sees(&n(false, vec!["sam".into()])));
        let open = Viewer {
            account: None,
            admin: false,
        };
        assert!(open.sees(&n(true, vec!["sam".into()])));
        let mut s = CenterSettings::default();
        assert!(s.validate().is_ok());
        s.memory_warn = 99;
        assert!(s.validate().is_err());
    }
}
