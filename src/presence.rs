//! Présence par le Wi-Fi : qui est à la maison ?
//!
//! Heim regarde si le téléphone de chacun est connecté au réseau de la maison. Pas de GPS,
//! rien à installer sur le téléphone. Chaque personne devient une entité `person.<id>`
//! (« home » / « away ») utilisable dans les automatismes.
//!
//! Comment : Heim envoie un petit paquet aux adresses du réseau local pour que le système
//! apprenne qui répond (table ARP), puis lit cette table. Un iPhone en veille répond mal :
//! on ne le déclare parti qu'après plusieurs minutes sans nouvelles (10 par défaut).
//!
//! Sur iPhone : Réglages > Wi-Fi > (i) > « Adresse Wi-Fi privée » sur « Fixe » (iOS 18),
//! sinon l'adresse du téléphone change de temps en temps et Heim ne le reconnaît plus.

use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::accounts::{Accounts, Role};
use crate::bus::{Bus, Event};
use crate::model::{Attributes, Category, Domain, EntityMeta};
use crate::notify::{Level, Notifier, NotifyRequest};
use crate::registry::RegistryHandle;
use crate::security::{Mode, Phase, SecurityHandle};
use crate::settings::Settings;

pub const CONFIG_KEY: &str = "presence";
pub const INTEGRATION: &str = "presence";
const TICK: Duration = Duration::from_secs(30);
const SWEEP_EVERY: u32 = 10; // tours de 30 s : un balayage complet toutes les 5 min

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EveryoneLeft {
    Nothing,
    /// Prévenir les membres : « Tout le monde est parti, activer l'alarme ? »
    #[default]
    Suggest,
    /// Activer l'alarme Absent toute seule (si tout est fermé).
    Arm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phone {
    /// Adresse matérielle Wi-Fi, en minuscules : `aa:bb:cc:dd:ee:ff`.
    pub mac: String,
    pub user_id: String,
    #[serde(default)]
    pub label: Option<String>,
}

fn d_away() -> u32 {
    10
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "d_away")]
    pub away_after_min: u32,
    #[serde(default)]
    pub everyone_left: EveryoneLeft,
    #[serde(default)]
    pub phones: Vec<Phone>,
    /// Pour les tests : délai en secondes à la place des minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub away_after_s: Option<u32>,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("réglages par défaut")
    }
}

impl PresenceConfig {
    /// Les adresses telles qu'on les compare (minuscules, deux-points), même si la base
    /// contient autre chose.
    fn normalized(mut self) -> Self {
        for p in &mut self.phones {
            if let Some(m) = normalize_mac(&p.mac) {
                p.mac = m;
            }
        }
        self
    }

    pub fn validate(&mut self) -> Result<(), String> {
        if !(1..=120).contains(&self.away_after_min) {
            return Err("délai d'absence : entre 1 et 120 minutes".into());
        }
        for p in &mut self.phones {
            p.mac =
                normalize_mac(&p.mac).ok_or_else(|| format!("adresse « {} » invalide", p.mac))?;
        }
        Ok(())
    }

    fn away_after(&self) -> chrono::TimeDelta {
        match self.away_after_s {
            Some(s) => chrono::TimeDelta::seconds(s as i64),
            None => chrono::TimeDelta::minutes(self.away_after_min as i64),
        }
    }
}

pub fn normalize_mac(raw: &str) -> Option<String> {
    let hex: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12
        || raw
            .chars()
            .any(|c| !(c.is_ascii_hexdigit() || ":-. ".contains(c)))
    {
        return None;
    }
    let hex = hex.to_ascii_lowercase();
    if hex == "000000000000" || hex == "ffffffffffff" {
        return None;
    }
    Some(
        (0..6)
            .map(|i| &hex[i * 2..i * 2 + 2])
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Adresse « privée » (tirée au hasard par le téléphone) : bit « administré localement ».
pub fn is_private_mac(mac: &str) -> bool {
    u8::from_str_radix(&mac[..2], 16).is_ok_and(|b| b & 0x02 != 0)
}

// ---- Lecture du voisinage réseau ----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Neighbor {
    pub ip: Ipv4Addr,
    pub mac: String,
    /// `Some(true)` : a répondu à l'instant ; `Some(false)` : connu mais ancien ; `None` : on ne sait pas.
    pub fresh: Option<bool>,
}

/// `ip -j neigh` (Linux, Raspberry Pi OS).
pub fn parse_ip_neigh_json(text: &str) -> Vec<Neighbor> {
    let Ok(list) = serde_json::from_str::<Vec<Value>>(text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|n| {
            let ip: Ipv4Addr = n.get("dst")?.as_str()?.parse().ok()?;
            let mac = normalize_mac(n.get("lladdr")?.as_str()?)?;
            let states: Vec<&str> = n
                .get("state")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if states.iter().any(|s| matches!(*s, "FAILED" | "INCOMPLETE")) {
                return None;
            }
            let fresh = states.iter().any(|s| matches!(*s, "REACHABLE" | "DELAY"));
            Some(Neighbor {
                ip,
                mac,
                fresh: Some(fresh),
            })
        })
        .collect()
}

/// `/proc/net/arp` (Linux sans iproute2).
pub fn parse_proc_arp(text: &str) -> Vec<Neighbor> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            if cols.len() < 4 || cols[2] != "0x2" {
                return None;
            }
            Some(Neighbor {
                ip: cols[0].parse().ok()?,
                mac: normalize_mac(cols[3])?,
                fresh: None,
            })
        })
        .collect()
}

/// `arp -a` (Windows : « 192.168.1.20   aa-bb-cc-dd-ee-ff   dynamique »).
pub fn parse_arp_a(text: &str) -> Vec<Neighbor> {
    text.lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            let ip: Ipv4Addr = cols
                .first()?
                .trim_matches(|c| c == '(' || c == ')')
                .parse()
                .ok()?;
            let mac = cols.iter().skip(1).find_map(|c| normalize_mac(c))?;
            if ip.octets()[3] == 255 || ip.is_multicast() {
                return None;
            }
            Some(Neighbor {
                ip,
                mac,
                fresh: None,
            })
        })
        .collect()
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// La table des voisins réseau, quelle que soit la plateforme. Bloquant.
pub fn read_neighbors() -> Vec<Neighbor> {
    if cfg!(target_os = "linux") {
        if let Some(text) = run("ip", &["-j", "neigh", "show"]) {
            let v = parse_ip_neigh_json(&text);
            if !v.is_empty() {
                return v;
            }
        }
        if let Ok(text) = std::fs::read_to_string("/proc/net/arp") {
            return parse_proc_arp(&text);
        }
    }
    run("arp", &["-a"])
        .map(|t| parse_arp_a(&t))
        .unwrap_or_default()
}

/// Un ping d'une seconde (commande système : pas besoin de droits particuliers). Bloquant.
pub fn ping(ip: Ipv4Addr) -> bool {
    let ip = ip.to_string();
    let args: Vec<&str> = if cfg!(windows) {
        vec!["-n", "1", "-w", "1000", &ip]
    } else {
        vec!["-c", "1", "-W", "1", &ip]
    };
    std::process::Command::new("ping")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Adresse locale de Heim (aucun paquet n'est envoyé pour la connaître).
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Un paquet UDP vers le port « discard » : le système doit d'abord trouver l'appareil
/// (requête ARP), ce qui remplit la table des voisins. Le paquet lui-même est ignoré.
fn poke(ips: impl IntoIterator<Item = Ipv4Addr>) {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return;
    };
    for ip in ips {
        let _ = socket.send_to(b"heim", (ip, 9));
    }
}

fn sweep_targets(me: Ipv4Addr) -> Vec<Ipv4Addr> {
    let [a, b, c, _] = me.octets();
    (1..=254u8)
        .map(|d| Ipv4Addr::new(a, b, c, d))
        .filter(|ip| *ip != me)
        .collect()
}

// ---- Moteur --------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Seen {
    pub ip: Option<Ipv4Addr>,
    pub last_seen: Option<DateTime<Utc>>,
    pub first_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NeighborView {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub private: bool,
    pub fresh: Option<bool>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub phone_of: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonView {
    pub user_id: String,
    pub name: String,
    pub home: Option<bool>,
    pub last_seen: Option<DateTime<Utc>>,
    pub phones: usize,
}

enum Msg {
    Seen(String, Option<Ipv4Addr>),
    Scan(oneshot::Sender<Vec<NeighborView>>),
    People(oneshot::Sender<Vec<PersonView>>),
    Evaluate,
}

#[derive(Clone)]
pub struct PresenceHandle {
    tx: mpsc::Sender<Msg>,
    settings: Arc<Settings>,
}

impl PresenceHandle {
    pub fn config(&self) -> PresenceConfig {
        self.settings.get(CONFIG_KEY).unwrap_or_default()
    }

    /// Signale qu'un appareil vient d'être vu (utilisé par le balayage, et par les tests).
    pub async fn seen(&self, mac: &str, ip: Option<Ipv4Addr>) {
        if let Some(mac) = normalize_mac(mac) {
            let _ = self.tx.send(Msg::Seen(mac, ip)).await;
        }
    }

    /// Recalcule qui est là maintenant (sans attendre le prochain tour).
    pub async fn evaluate(&self) {
        let _ = self.tx.send(Msg::Evaluate).await;
    }

    /// Balaye le réseau maintenant et renvoie les appareils trouvés (pour choisir son téléphone).
    pub async fn scan(&self) -> Vec<NeighborView> {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Msg::Scan(reply)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn people(&self) -> Vec<PersonView> {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Msg::People(reply)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }
}

pub struct Deps {
    pub registry: RegistryHandle,
    pub notify: Arc<Notifier>,
    pub settings: Arc<Settings>,
    pub accounts: Arc<Accounts>,
    pub security: SecurityHandle,
    pub bus: Bus,
}

pub fn spawn(deps: Deps) -> PresenceHandle {
    let (tx, rx) = mpsc::channel(64);
    let handle = PresenceHandle {
        tx: tx.clone(),
        settings: deps.settings.clone(),
    };
    let engine = Engine {
        config: deps
            .settings
            .get::<PresenceConfig>(CONFIG_KEY)
            .unwrap_or_default()
            .normalized(),
        seen: HashMap::new(),
        home: BTreeMap::new(),
        deps,
        tx,
        started: Utc::now(),
    };
    tokio::spawn(engine.run(rx));
    handle
}

struct Engine {
    config: PresenceConfig,
    seen: HashMap<String, Seen>,
    /// Dernier état publié par personne.
    home: BTreeMap<String, bool>,
    deps: Deps,
    tx: mpsc::Sender<Msg>,
    started: DateTime<Utc>,
}

impl Engine {
    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        // On reprend l'état rangé en base : un redémarrage ne fait partir personne.
        for e in self.deps.registry.snapshot().await {
            if e.meta.domain == Domain::Person
                && let Some(st) = e.state
            {
                let id = e.meta.entity_id.trim_start_matches("person.").to_string();
                self.home.insert(id, st.value == json!("home"));
            }
        }
        self.sync_entities().await;
        let mut bus_rx = self.deps.bus.subscribe();
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut round: u32 = 0;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if self.config.enabled {
                        self.scan_round(round.is_multiple_of(SWEEP_EVERY));
                        round = round.wrapping_add(1);
                    }
                    self.evaluate().await;
                }
                msg = rx.recv() => match msg {
                    Some(Msg::Seen(mac, ip)) => {
                        let now = Utc::now();
                        let entry = self.seen.entry(mac).or_insert(Seen { ip, last_seen: None, first_seen: now });
                        entry.last_seen = Some(now);
                        if ip.is_some() { entry.ip = ip; }
                        self.evaluate().await;
                    }
                    Some(Msg::Evaluate) => self.evaluate().await,
                    Some(Msg::People(reply)) => { let _ = reply.send(self.people()); }
                    Some(Msg::Scan(reply)) => {
                        let tx = self.tx.clone();
                        let found = tokio::task::spawn_blocking(|| {
                            if let Some(me) = local_ipv4() { poke(sweep_targets(me)); }
                            std::thread::sleep(Duration::from_secs(3));
                            read_neighbors()
                        }).await.unwrap_or_default();
                        let now = Utc::now();
                        for n in &found {
                            let e = self.seen.entry(n.mac.clone()).or_insert(Seen { ip: Some(n.ip), last_seen: None, first_seen: now });
                            e.ip = Some(n.ip);
                            if n.fresh == Some(true) { e.last_seen = Some(now); }
                        }
                        let _ = reply.send(self.view(&found));
                        let _ = tx.try_send(Msg::Evaluate);
                    }
                    None => break,
                },
                ev = bus_rx.recv() => match ev {
                    Ok(Event::SettingsChanged { key }) if key == CONFIG_KEY => {
                        self.config = self.deps.settings.get::<PresenceConfig>(CONFIG_KEY).unwrap_or_default().normalized();
                        self.sync_entities().await;
                        self.evaluate().await;
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }
    }

    /// Un tour : réveiller les téléphones suivis (et tout le réseau de temps en temps), puis lire.
    fn scan_round(&self, sweep: bool) {
        let tracked: Vec<(String, Option<Ipv4Addr>)> = self
            .config
            .phones
            .iter()
            .map(|p| (p.mac.clone(), self.seen.get(&p.mac).and_then(|s| s.ip)))
            .collect();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let me = local_ipv4();
            let known: Vec<Ipv4Addr> = tracked.iter().filter_map(|(_, ip)| *ip).collect();
            if sweep || known.len() < tracked.len() {
                if let Some(me) = me {
                    poke(sweep_targets(me));
                }
            } else {
                poke(known.iter().copied());
            }
            // Le temps que les réponses arrivent (sonde ARP du système : quelques secondes).
            std::thread::sleep(Duration::from_secs(5));
            let neighbors = read_neighbors();
            for (mac, last_ip) in &tracked {
                let found = neighbors.iter().find(|n| &n.mac == mac);
                let ip = found.map(|n| n.ip).or(*last_ip);
                let fresh = match found.and_then(|n| n.fresh) {
                    Some(true) => true,
                    // Pas d'état fiable (Windows, /proc/net/arp) ou entrée ancienne : on demande.
                    _ => ip.is_some_and(ping),
                };
                if fresh {
                    let _ = tx.blocking_send(Msg::Seen(mac.clone(), ip));
                }
            }
        });
    }

    fn view(&self, found: &[Neighbor]) -> Vec<NeighborView> {
        let mut v: Vec<NeighborView> = found
            .iter()
            .map(|n| {
                let s = self.seen.get(&n.mac);
                NeighborView {
                    ip: n.ip,
                    mac: n.mac.clone(),
                    private: is_private_mac(&n.mac),
                    fresh: n.fresh,
                    first_seen: s.map(|s| s.first_seen).unwrap_or_else(Utc::now),
                    last_seen: s.and_then(|s| s.last_seen),
                    phone_of: self
                        .config
                        .phones
                        .iter()
                        .find(|p| p.mac == n.mac)
                        .map(|p| p.user_id.clone()),
                }
            })
            .collect();
        // Les derniers arrivés en premier : allumer le Wi-Fi du téléphone le fait remonter.
        v.sort_by(|a, b| b.first_seen.cmp(&a.first_seen).then(a.ip.cmp(&b.ip)));
        v
    }

    fn people(&self) -> Vec<PersonView> {
        let mut by_user: BTreeMap<String, Vec<&Phone>> = BTreeMap::new();
        for p in &self.config.phones {
            by_user.entry(p.user_id.clone()).or_default().push(p);
        }
        by_user
            .into_iter()
            .map(|(user_id, phones)| PersonView {
                name: self
                    .deps
                    .accounts
                    .get(&user_id)
                    .map(|u| u.name)
                    .unwrap_or_else(|| user_id.clone()),
                home: self.home.get(&user_id).copied(),
                last_seen: phones
                    .iter()
                    .filter_map(|p| self.seen.get(&p.mac).and_then(|s| s.last_seen))
                    .max(),
                phones: phones.len(),
                user_id,
            })
            .collect()
    }

    /// Une entité par personne suivie ; celles qui ne le sont plus disparaissent.
    async fn sync_entities(&mut self) {
        let wanted: Vec<String> = if self.config.enabled {
            let mut ids: Vec<String> = self
                .config
                .phones
                .iter()
                .map(|p| p.user_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids
        } else {
            Vec::new()
        };
        for e in self.deps.registry.snapshot().await {
            if e.meta.integration == INTEGRATION {
                let id = e.meta.entity_id.trim_start_matches("person.");
                if !wanted.iter().any(|w| w == id) {
                    self.deps.registry.remove(&e.meta.entity_id).await;
                    self.home.remove(id);
                }
            }
        }
        for user_id in wanted {
            let name = self
                .deps
                .accounts
                .get(&user_id)
                .map(|u| u.name)
                .unwrap_or_else(|| user_id.clone());
            self.deps
                .registry
                .upsert(EntityMeta {
                    entity_id: format!("person.{user_id}"),
                    domain: Domain::Person,
                    name,
                    integration: INTEGRATION.into(),
                    device_id: None,
                    room: None,
                    plan: None,
                    capabilities: Attributes::new(),
                    user_edited: false,
                    category: Category::Primary,
                    hidden: false,
                })
                .await;
        }
    }

    async fn evaluate(&mut self) {
        if !self.config.enabled || self.config.phones.is_empty() {
            return;
        }
        let now = Utc::now();
        let away_after = self.config.away_after();
        let mut by_user: BTreeMap<String, Option<DateTime<Utc>>> = BTreeMap::new();
        for p in &self.config.phones {
            let last = self.seen.get(&p.mac).and_then(|s| s.last_seen);
            let e = by_user.entry(p.user_id.clone()).or_insert(None);
            *e = (*e).max(last);
        }
        let mut arrived = Vec::new();
        let mut left = Vec::new();
        for (user_id, last) in &by_user {
            let home_now = last.is_some_and(|t| now - t < away_after);
            let before = self.home.get(user_id).copied();
            // Au démarrage, sans nouvelles : on attend le délai avant de déclarer parti.
            if before.is_none() && !home_now && now - self.started < away_after {
                continue;
            }
            if before != Some(home_now) {
                self.home.insert(user_id.clone(), home_now);
                let mut attrs = Attributes::new();
                if let Some(t) = last {
                    attrs.insert("last_seen".into(), json!(t));
                }
                self.deps
                    .registry
                    .report(
                        format!("person.{user_id}"),
                        json!(if home_now { "home" } else { "away" }),
                        attrs,
                        true,
                    )
                    .await;
                if before.is_some() {
                    if home_now {
                        arrived.push(user_id.clone());
                    } else {
                        left.push(user_id.clone());
                    }
                }
            }
        }
        if !left.is_empty() && !self.home.is_empty() && self.home.values().all(|h| !h) {
            self.everyone_left().await;
        }
        for user_id in arrived {
            self.someone_arrived(&user_id).await;
        }
    }

    fn name(&self, user_id: &str) -> String {
        self.deps
            .accounts
            .get(user_id)
            .map(|u| u.name)
            .unwrap_or_else(|| user_id.to_string())
    }

    /// Les comptes qui peuvent agir sur l'alarme (membres et admins).
    fn members(&self) -> Vec<String> {
        self.deps
            .accounts
            .list()
            .into_iter()
            .filter(|u| u.role >= Role::Member)
            .map(|u| format!("person:{}", u.id))
            .collect()
    }

    fn notify(&self, title: &str, message: String) {
        let to = self.members();
        let _ = self.deps.notify.send(NotifyRequest {
            title: Some(title.into()),
            message,
            level: Level::Important,
            // Sans compte, tous les appareils.
            to,
            except: Vec::new(),
            action: Some(json!({ "open": "security" })),
        });
    }

    async fn everyone_left(&mut self) {
        let status = self.deps.security.status().await;
        if status.phase != Phase::Disarmed {
            return;
        }
        let configured = crate::security::is_configured(&self.deps.security.config());
        match self.config.everyone_left {
            EveryoneLeft::Nothing => {}
            _ if !configured => {}
            EveryoneLeft::Suggest => self.notify(
                "Tout le monde est parti",
                "La maison est vide. Touchez pour activer l'alarme.".into(),
            ),
            EveryoneLeft::Arm => match self.deps.security.arm(Mode::Away, false, None).await {
                Ok(_) => self.notify(
                    "Alarme activée",
                    "Tout le monde est parti : l'alarme passe en mode Absent.".into(),
                ),
                Err(e) => self.notify(
                    "Tout le monde est parti",
                    format!(
                        "L'alarme n'a pas pu s'activer ({}). Touchez pour vérifier.",
                        e.message()
                    ),
                ),
            },
        }
    }

    /// Pas de désactivation automatique : un téléphone volé ne doit pas ouvrir la maison.
    async fn someone_arrived(&self, user_id: &str) {
        let status = self.deps.security.status().await;
        if matches!(status.phase, Phase::Armed | Phase::Arming) && status.mode == Some(Mode::Away) {
            self.notify(
                "Retour à la maison",
                format!(
                    "{} est rentré(e). L'alarme est toujours activée.",
                    self.name(user_id)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_are_normalized() {
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-0F").as_deref(),
            Some("aa:bb:cc:dd:ee:0f")
        );
        assert_eq!(
            normalize_mac("aabb.ccdd.ee0f").as_deref(),
            Some("aa:bb:cc:dd:ee:0f")
        );
        assert!(normalize_mac("ff:ff:ff:ff:ff:ff").is_none());
        assert!(normalize_mac("pas une adresse").is_none());
        assert!(is_private_mac("da:a1:19:00:00:01"));
        assert!(!is_private_mac("b8:27:eb:00:00:01"));
    }

    #[test]
    fn parses_neighbor_tables() {
        let json = r#"[
            {"dst":"192.168.1.20","dev":"wlan0","lladdr":"da:a1:19:12:34:56","state":["REACHABLE"]},
            {"dst":"192.168.1.21","dev":"wlan0","lladdr":"b8:27:eb:00:00:01","state":["STALE"]},
            {"dst":"192.168.1.22","dev":"wlan0","state":["FAILED"]},
            {"dst":"fe80::1","dev":"wlan0","lladdr":"00:11:22:33:44:55","state":["REACHABLE"]}
        ]"#;
        let n = parse_ip_neigh_json(json);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].fresh, Some(true));
        assert_eq!(n[1].fresh, Some(false));

        let proc = "IP address       HW type     Flags       HW address            Mask     Device\n\
                    192.168.1.20     0x1         0x2         da:a1:19:12:34:56     *        wlan0\n\
                    192.168.1.23     0x1         0x0         00:00:00:00:00:00     *        wlan0\n";
        assert_eq!(parse_proc_arp(proc).len(), 1);

        let win = "Interface : 192.168.1.10 --- 0x7\n  Adresse Internet      Adresse physique      Type\n  192.168.1.1           74-ac-b9-11-22-33     dynamique\n  192.168.1.255         ff-ff-ff-ff-ff-ff     statique\n  224.0.0.22            01-00-5e-00-00-16     statique\n";
        let w = parse_arp_a(win);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].mac, "74:ac:b9:11:22:33");
    }
}
