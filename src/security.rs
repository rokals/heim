//! Alarme, mode simple.
//!
//! Chaque capteur reçoit un rôle :
//! - **périmètre** (portes, fenêtres) : surveillé en Absent et en Nuit ;
//! - **intérieur** (détecteurs de mouvement) : surveillé seulement en Absent ;
//! - **24 h/24** (fumée, fuite, arrachement) : déclenche toujours, même alarme coupée.
//!
//! Une porte marquée « entrée » laisse le temps de désactiver (délai d'entrée) ; pendant ce
//! délai les détecteurs intérieurs sont ignorés, le temps de traverser l'entrée.
//! L'état est rangé en base : un redémarrage ne désarme pas la maison.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::bus::{Bus, Event};
use crate::model::{Action, EntityId, slugify};
use crate::notify::{Level, Notifier, NotifyRequest};
use crate::registry::RegistryHandle;
use crate::settings::Settings;

pub const CONFIG_KEY: &str = "security";
const STATE_KEY: &str = "security.state";
const LOG_KEY: &str = "security.log";
const LOG_MAX: usize = 200;
const SNAPSHOTS_MAX: usize = 30;
const SOURCE: &str = "security";

// ---- Réglages ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Perimeter,
    Interior,
    H24,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorRole {
    pub entity_id: EntityId,
    pub role: Role,
    /// Porte d'entrée : laisse le délai d'entrée avant de sonner.
    #[serde(default)]
    pub entry: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub name: String,
    /// Adresse d'une image JPEG fixe (`http://user:mdp@192.168.1.20/snapshot.jpg`).
    pub snapshot_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeHash {
    pub salt: String,
    pub hash: String,
}

fn d_exit() -> u32 {
    60
}
fn d_entry() -> u32 {
    30
}
fn d_siren() -> u32 {
    180
}
fn d_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub sensors: Vec<SensorRole>,
    #[serde(default)]
    pub sirens: Vec<EntityId>,
    /// Lumières allumées en cas d'intrusion.
    #[serde(default)]
    pub lights: Vec<EntityId>,
    #[serde(default)]
    pub cameras: Vec<Camera>,
    #[serde(default = "d_exit")]
    pub exit_delay_s: u32,
    /// En Nuit on est déjà dedans : pas de délai de sortie par défaut.
    #[serde(default)]
    pub night_exit_delay_s: u32,
    #[serde(default = "d_entry")]
    pub entry_delay_s: u32,
    #[serde(default = "d_siren")]
    pub siren_duration_s: u32,
    /// Les capteurs 24 h/24 font aussi sonner les sirènes (sinon : notification seule).
    #[serde(default = "d_true")]
    pub h24_sirens: bool,
    /// Demander le code aussi pour activer (sinon seulement pour désactiver).
    #[serde(default)]
    pub code_to_arm: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeHash>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("réglages par défaut")
    }
}

impl SecurityConfig {
    /// Version envoyée à l'interface : le code ne sort jamais, même haché.
    pub fn public(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("code");
            o.insert("has_code".into(), json!(self.code.is_some()));
        }
        v
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.exit_delay_s > 600 || self.night_exit_delay_s > 600 || self.entry_delay_s > 600 {
            return Err("les délais sont limités à 10 minutes".into());
        }
        if !(5..=900).contains(&self.siren_duration_s) {
            return Err("durée des sirènes : entre 5 secondes et 15 minutes".into());
        }
        for c in &self.cameras {
            if c.name.trim().is_empty() {
                return Err("chaque caméra doit avoir un nom".into());
            }
            if !(c.snapshot_url.starts_with("http://") || c.snapshot_url.starts_with("https://")) {
                return Err(format!(
                    "caméra « {} » : l'adresse doit commencer par http:// ou https://",
                    c.name
                ));
            }
        }
        Ok(())
    }

    fn role_of(&self, entity_id: &str) -> Option<&SensorRole> {
        self.sensors.iter().find(|s| s.entity_id == entity_id)
    }
}

/// L'alarme a au moins un capteur à surveiller (sinon on ne propose pas de l'activer).
pub fn is_configured(cfg: &SecurityConfig) -> bool {
    cfg.sensors
        .iter()
        .any(|s| matches!(s.role, Role::Perimeter | Role::Interior))
}

pub fn hash_code(code: &str, salt: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("heim:{salt}:{code}").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_code(code: &str) -> Result<CodeHash, String> {
    let code = code.trim();
    if code.len() < 4 || code.len() > 12 {
        return Err("le code doit faire entre 4 et 12 caractères".into());
    }
    use sha2::Digest;
    let seed = format!(
        "{:?}{}{:p}",
        std::time::SystemTime::now(),
        std::process::id(),
        &code
    );
    let salt: String = sha2::Sha256::digest(seed.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(CodeHash {
        hash: hash_code(code, &salt),
        salt,
    })
}

// ---- État -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Away,
    Night,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Away => "Absent",
            Mode::Night => "Nuit",
        }
    }
    fn watches(self, role: Role) -> bool {
        match role {
            Role::Perimeter | Role::H24 => true,
            Role::Interior => self == Mode::Away,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Disarmed,
    /// Délai de sortie en cours.
    Arming,
    Armed,
    /// Délai d'entrée en cours : il reste `until` pour désactiver.
    Pending,
    Triggered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cause {
    pub entity_id: EntityId,
    pub name: String,
    #[serde(default)]
    pub h24: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub phase: Phase,
    #[serde(default)]
    pub mode: Option<Mode>,
    pub since: DateTime<Utc>,
    /// Fin du délai de sortie ou d'entrée.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    /// Capteurs ignorés pour cette activation (restés ouverts, injoignables).
    #[serde(default)]
    pub bypassed: Vec<EntityId>,
    #[serde(default)]
    pub cause: Option<Cause>,
    /// Les sirènes sonnent jusqu'à cette heure.
    #[serde(default)]
    pub sirens_until: Option<DateTime<Utc>>,
}

impl Status {
    fn disarmed() -> Self {
        Status {
            phase: Phase::Disarmed,
            mode: None,
            since: Utc::now(),
            until: None,
            bypassed: Vec::new(),
            cause: None,
            sirens_until: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at: DateTime<Utc>,
    /// armed, disarmed, arming, pending, triggered, h24, offline, bypassed, wrong_code, snapshot, test
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SensorInfo {
    pub entity_id: EntityId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ArmError {
    /// Des capteurs sont ouverts ou injoignables : on peut forcer.
    NotReady {
        open: Vec<SensorInfo>,
        unavailable: Vec<SensorInfo>,
    },
    Code {
        message: String,
    },
    Invalid {
        message: String,
    },
}

impl ArmError {
    pub fn message(&self) -> String {
        match self {
            ArmError::NotReady { open, unavailable } => {
                let mut parts = Vec::new();
                if !open.is_empty() {
                    parts.push(format!(
                        "ouvert : {}",
                        open.iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !unavailable.is_empty() {
                    parts.push(format!(
                        "injoignable : {}",
                        unavailable
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                parts.join(" ; ")
            }
            ArmError::Code { message } | ArmError::Invalid { message } => message.clone(),
        }
    }
}

// ---- Poignée ----------------------------------------------------------------------

enum Msg {
    Arm {
        mode: Mode,
        force: bool,
        code: Option<String>,
        reply: oneshot::Sender<Result<Status, ArmError>>,
    },
    Disarm {
        code: Option<String>,
        reply: oneshot::Sender<Result<Status, String>>,
    },
    Status(oneshot::Sender<Status>),
    TestSirens {
        seconds: u32,
        reply: oneshot::Sender<Result<usize, String>>,
    },
    Snapshot {
        camera: Option<String>,
        reply: oneshot::Sender<Result<Vec<String>, String>>,
    },
}

#[derive(Clone)]
pub struct SecurityHandle {
    tx: mpsc::Sender<Msg>,
    settings: Arc<Settings>,
    snapshots_dir: PathBuf,
}

impl SecurityHandle {
    pub async fn arm(
        &self,
        mode: Mode,
        force: bool,
        code: Option<String>,
    ) -> Result<Status, ArmError> {
        let (reply, rx) = oneshot::channel();
        let gone = || ArmError::Invalid {
            message: "alarme indisponible".into(),
        };
        self.tx
            .send(Msg::Arm {
                mode,
                force,
                code,
                reply,
            })
            .await
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    pub async fn disarm(&self, code: Option<String>) -> Result<Status, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::Disarm { code, reply })
            .await
            .map_err(|_| "alarme indisponible".to_string())?;
        rx.await.map_err(|_| "alarme indisponible".to_string())?
    }

    pub async fn status(&self) -> Status {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Msg::Status(reply)).await.is_err() {
            return Status::disarmed();
        }
        rx.await.unwrap_or_else(|_| Status::disarmed())
    }

    pub async fn test_sirens(&self, seconds: u32) -> Result<usize, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::TestSirens { seconds, reply })
            .await
            .map_err(|_| "alarme indisponible".to_string())?;
        rx.await.map_err(|_| "alarme indisponible".to_string())?
    }

    /// Prend une photo avec une caméra (ou toutes) et renvoie les noms de fichiers.
    pub async fn snapshot(&self, camera: Option<String>) -> Result<Vec<String>, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::Snapshot { camera, reply })
            .await
            .map_err(|_| "alarme indisponible".to_string())?;
        rx.await.map_err(|_| "alarme indisponible".to_string())?
    }

    pub fn config(&self) -> SecurityConfig {
        self.settings.get(CONFIG_KEY).unwrap_or_default()
    }

    pub fn log(&self) -> Vec<LogEntry> {
        self.settings.get(LOG_KEY).unwrap_or_default()
    }

    pub fn snapshots_dir(&self) -> &PathBuf {
        &self.snapshots_dir
    }

    /// Les photos, de la plus récente à la plus ancienne.
    pub fn snapshots(&self) -> Vec<String> {
        list_snapshots(&self.snapshots_dir)
    }
}

fn list_snapshots(dir: &PathBuf) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".jpg"))
                .collect()
        })
        .unwrap_or_default();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
}

/// Nom de photo acceptable dans une URL (pas de `..`, pas de `/`).
pub fn valid_snapshot_name(name: &str) -> bool {
    name.ends_with(".jpg")
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

pub struct Deps {
    pub registry: RegistryHandle,
    pub notify: Arc<Notifier>,
    pub settings: Arc<Settings>,
    pub bus: Bus,
    pub data_dir: PathBuf,
}

pub fn spawn(deps: Deps) -> SecurityHandle {
    let (tx, rx) = mpsc::channel(32);
    let snapshots_dir = deps.data_dir.join("snapshots");
    let handle = SecurityHandle {
        tx,
        settings: deps.settings.clone(),
        snapshots_dir: snapshots_dir.clone(),
    };
    let mut status: Status = deps
        .settings
        .get(STATE_KEY)
        .unwrap_or_else(Status::disarmed);
    // Les sirènes ne reprennent pas après un redémarrage.
    status.sirens_until = None;
    let engine = Engine {
        config: deps.settings.get(CONFIG_KEY).unwrap_or_default(),
        status,
        registry: deps.registry,
        notify: deps.notify,
        settings: deps.settings,
        bus: deps.bus,
        snapshots_dir,
        failures: VecDeque::new(),
        offline_warned: HashSet::new(),
        #[cfg(feature = "webhook")]
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default(),
    };
    tokio::spawn(engine.run(rx));
    handle
}

// ---- Moteur -----------------------------------------------------------------------

struct Engine {
    config: SecurityConfig,
    status: Status,
    registry: RegistryHandle,
    notify: Arc<Notifier>,
    settings: Arc<Settings>,
    bus: Bus,
    snapshots_dir: PathBuf,
    /// Mauvais codes récents (anti-essais à la chaîne).
    failures: VecDeque<Instant>,
    offline_warned: HashSet<EntityId>,
    #[cfg(feature = "webhook")]
    http: reqwest::Client,
}

fn instant_at(at: DateTime<Utc>) -> tokio::time::Instant {
    let left = (at - Utc::now()).to_std().unwrap_or_default();
    tokio::time::Instant::now() + left
}

/// « Porte entrée » plutôt que « Porte entrée contact » : le nom de l'appareil parle mieux.
fn display_name(meta: &crate::model::EntityMeta) -> String {
    if meta.user_edited {
        return meta.name.clone();
    }
    meta.capabilities
        .get("device_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| meta.name.clone())
}

fn is_on(value: &Value) -> bool {
    matches!(value.as_str(), Some("on" | "open" | "detected")) || value == &json!(true)
}

impl Engine {
    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        let mut bus_rx = self.bus.subscribe();
        // Délai dépassé pendant que Heim était arrêté.
        self.on_deadline().await;
        loop {
            let deadline = self.next_deadline();
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => break,
                },
                ev = bus_rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, old, new }) => {
                        self.on_state(&entity_id, old.as_ref(), &new).await;
                    }
                    Ok(Event::SettingsChanged { key }) if key == CONFIG_KEY => {
                        self.config = self.settings.get(CONFIG_KEY).unwrap_or_default();
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = async {
                    match deadline {
                        Some(d) => tokio::time::sleep_until(d).await,
                        None => std::future::pending().await,
                    }
                } => self.on_deadline().await,
            }
        }
    }

    fn next_deadline(&self) -> Option<tokio::time::Instant> {
        let delay = match self.status.phase {
            Phase::Arming | Phase::Pending => self.status.until,
            _ => None,
        };
        [delay, self.status.sirens_until]
            .into_iter()
            .flatten()
            .min()
            .map(instant_at)
    }

    async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Arm {
                mode,
                force,
                code,
                reply,
            } => {
                let r = self.arm(mode, force, code).await;
                let _ = reply.send(r);
            }
            Msg::Disarm { code, reply } => {
                let r = self.disarm(code).await;
                let _ = reply.send(r);
            }
            Msg::Status(reply) => {
                let _ = reply.send(self.status.clone());
            }
            Msg::TestSirens { seconds, reply } => {
                let sirens = self.config.sirens.clone();
                if sirens.is_empty() {
                    let _ = reply.send(Err("aucune sirène choisie".into()));
                    return;
                }
                for s in &sirens {
                    self.turn(s, true).await;
                }
                self.log("test", format!("Essai des sirènes ({seconds} s)"), None);
                let registry = self.registry.clone();
                let list = sirens.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(seconds.clamp(1, 30) as u64)).await;
                    for s in &list {
                        let _ = registry.command(s, Action::TurnOff, SOURCE).await;
                    }
                });
                let _ = reply.send(Ok(sirens.len()));
            }
            Msg::Snapshot { camera, reply } => {
                let cams: Vec<Camera> = self
                    .config
                    .cameras
                    .iter()
                    .filter(|c| camera.as_ref().is_none_or(|n| &c.name == n))
                    .cloned()
                    .collect();
                if cams.is_empty() {
                    let _ = reply.send(Err("aucune caméra".into()));
                    return;
                }
                let mut names = Vec::new();
                let mut errors = Vec::new();
                for c in &cams {
                    match self.capture(c).await {
                        Ok(n) => names.push(n),
                        Err(e) => errors.push(format!("{} : {e}", c.name)),
                    }
                }
                let _ = reply.send(if names.is_empty() {
                    Err(errors.join(" ; "))
                } else {
                    Ok(names)
                });
            }
        }
    }

    fn check_code(&mut self, code: Option<&str>) -> Result<(), String> {
        let Some(expected) = &self.config.code else {
            return Ok(());
        };
        let now = Instant::now();
        while self
            .failures
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(300))
        {
            self.failures.pop_front();
        }
        if self.failures.len() >= 5
            && self
                .failures
                .back()
                .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(60))
        {
            return Err("trop d'essais : patientez une minute".into());
        }
        let given = code.unwrap_or_default().trim();
        if !given.is_empty() && hash_code(given, &expected.salt) == expected.hash {
            self.failures.clear();
            return Ok(());
        }
        self.failures.push_back(now);
        self.log("wrong_code", "Code erroné".into(), None);
        Err(if given.is_empty() {
            "code demandé".into()
        } else {
            "code erroné".into()
        })
    }

    async fn arm(
        &mut self,
        mode: Mode,
        force: bool,
        code: Option<String>,
    ) -> Result<Status, ArmError> {
        if self.config.code_to_arm {
            self.check_code(code.as_deref())
                .map_err(|message| ArmError::Code { message })?;
        }
        let watched: Vec<&SensorRole> = self
            .config
            .sensors
            .iter()
            .filter(|s| mode.watches(s.role) && s.role != Role::H24)
            .collect();
        if watched.is_empty() {
            return Err(ArmError::Invalid {
                message: format!(
                    "aucun capteur surveillé en mode {} : choisissez-les dans Réglages > Sécurité",
                    mode.label()
                ),
            });
        }
        let mut open = Vec::new();
        let mut unavailable = Vec::new();
        for s in watched {
            let entity = self.registry.get(&s.entity_id).await;
            let name = entity
                .as_ref()
                .map(|e| display_name(&e.meta))
                .unwrap_or_else(|| s.entity_id.clone());
            let info = SensorInfo {
                entity_id: s.entity_id.clone(),
                name,
            };
            match entity.and_then(|e| e.state) {
                Some(st) if !st.available => unavailable.push(info),
                None => unavailable.push(info),
                // Un détecteur de mouvement qui voit encore quelqu'un n'empêche pas de partir.
                Some(st) if is_on(&st.value) && s.role == Role::Perimeter => open.push(info),
                _ => {}
            }
        }
        if (!open.is_empty() || !unavailable.is_empty()) && !force {
            return Err(ArmError::NotReady { open, unavailable });
        }
        let bypassed: Vec<EntityId> = open
            .iter()
            .chain(unavailable.iter())
            .map(|s| s.entity_id.clone())
            .collect();
        let delay = match mode {
            Mode::Away => self.config.exit_delay_s,
            Mode::Night => self.config.night_exit_delay_s,
        };
        let now = Utc::now();
        // Si l'alarme sonnait, on coupe d'abord.
        self.silence().await;
        self.offline_warned.clear();
        self.status = Status {
            phase: if delay > 0 {
                Phase::Arming
            } else {
                Phase::Armed
            },
            mode: Some(mode),
            since: now,
            until: (delay > 0).then(|| now + chrono::TimeDelta::seconds(delay as i64)),
            bypassed,
            cause: None,
            sirens_until: None,
        };
        let mut msg = if delay > 0 {
            format!("Activation {} dans {delay} s", mode.label())
        } else {
            format!("Alarme activée : {}", mode.label())
        };
        if !open.is_empty() || !unavailable.is_empty() {
            let names: Vec<&str> = open
                .iter()
                .chain(unavailable.iter())
                .map(|s| s.name.as_str())
                .collect();
            msg.push_str(&format!(" (ignorés : {})", names.join(", ")));
        }
        self.log(if delay > 0 { "arming" } else { "armed" }, msg, None);
        self.save();
        Ok(self.status.clone())
    }

    async fn disarm(&mut self, code: Option<String>) -> Result<Status, String> {
        if self.status.phase == Phase::Disarmed
            && self.status.sirens_until.is_none()
            && self.status.cause.is_none()
        {
            return Ok(self.status.clone());
        }
        self.check_code(code.as_deref())?;
        let was = self.status.phase;
        self.silence().await;
        self.status = Status::disarmed();
        self.log(
            "disarmed",
            if was == Phase::Triggered {
                "Alarme arrêtée".into()
            } else {
                "Alarme désactivée".into()
            },
            None,
        );
        self.save();
        Ok(self.status.clone())
    }

    async fn on_deadline(&mut self) {
        let now = Utc::now();
        if self.status.sirens_until.is_some_and(|t| t <= now) {
            self.status.sirens_until = None;
            for s in self.config.sirens.clone() {
                self.turn(&s, false).await;
            }
            self.save();
        }
        let due = self.status.until.is_some_and(|t| t <= now);
        match self.status.phase {
            Phase::Arming if due => {
                // Ce qui est resté ouvert à la fin du délai de sortie est ignoré, et on le dit.
                let mode = self.status.mode.unwrap_or(Mode::Away);
                let mut left_open = Vec::new();
                for s in self.config.sensors.clone() {
                    if s.role != Role::Perimeter
                        || !mode.watches(s.role)
                        || self.status.bypassed.contains(&s.entity_id)
                    {
                        continue;
                    }
                    if let Some(e) = self.registry.get(&s.entity_id).await
                        && e.state.as_ref().is_some_and(|st| is_on(&st.value))
                    {
                        left_open.push(display_name(&e.meta));
                        self.status.bypassed.push(s.entity_id.clone());
                    }
                }
                self.status.phase = Phase::Armed;
                self.status.until = None;
                self.status.since = now;
                self.log("armed", format!("Alarme activée : {}", mode.label()), None);
                if !left_open.is_empty() {
                    let names = left_open.join(", ");
                    self.log("bypassed", format!("Resté ouvert, ignoré : {names}"), None);
                    self.send(
                        "Alarme activée",
                        format!("Attention, resté ouvert (non surveillé) : {names}"),
                        Level::Important,
                    );
                }
                self.save();
            }
            Phase::Pending if due => {
                let cause = self.status.cause.clone();
                self.trigger(cause.unwrap_or(Cause {
                    entity_id: String::new(),
                    name: "délai d'entrée écoulé".into(),
                    h24: false,
                }))
                .await;
            }
            _ => {}
        }
    }

    async fn on_state(
        &mut self,
        entity_id: &str,
        old: Option<&crate::model::State>,
        new: &crate::model::State,
    ) {
        let Some(role) = self.config.role_of(entity_id).cloned() else {
            return;
        };
        let active = matches!(
            self.status.phase,
            Phase::Arming | Phase::Armed | Phase::Pending
        );
        let mode = self.status.mode.unwrap_or(Mode::Away);
        let watched = role.role == Role::H24
            || (active
                && mode.watches(role.role)
                && !self.status.bypassed.contains(&role.entity_id));

        // Supervision : un capteur surveillé ne répond plus.
        let was_available = old.is_none_or(|o| o.available);
        if was_available && !new.available {
            if watched
                && (active || role.role == Role::H24)
                && self.offline_warned.insert(role.entity_id.clone())
            {
                let n = self.name_of(entity_id).await;
                self.log("offline", format!("{n} ne répond plus"), None);
                self.send(
                    "Alarme : capteur muet",
                    format!("{n} ne répond plus : il n'est plus surveillé."),
                    Level::Important,
                );
            }
            return;
        }
        if new.available {
            self.offline_warned.remove(&role.entity_id);
        }

        let rising = is_on(&new.value) && !old.is_some_and(|o| is_on(&o.value));
        if !rising || !watched {
            return;
        }
        let n = self.name_of(entity_id).await;
        let cause = Cause {
            entity_id: role.entity_id.clone(),
            name: n.clone(),
            h24: role.role == Role::H24,
        };
        if role.role == Role::H24 {
            self.log("h24", format!("Alerte 24 h/24 : {n}"), None);
            self.trigger(cause).await;
            return;
        }
        match self.status.phase {
            // On sort : portes et mouvements attendus.
            Phase::Arming => {}
            Phase::Armed => {
                if role.entry && self.config.entry_delay_s > 0 {
                    let now = Utc::now();
                    self.status.phase = Phase::Pending;
                    self.status.until =
                        Some(now + chrono::TimeDelta::seconds(self.config.entry_delay_s as i64));
                    self.status.cause = Some(cause);
                    self.log(
                        "pending",
                        format!("{n} : {} s pour désactiver", self.config.entry_delay_s),
                        None,
                    );
                    self.save();
                } else {
                    self.trigger(cause).await;
                }
            }
            // Pendant le délai d'entrée, on traverse l'intérieur : seul le périmètre compte.
            Phase::Pending if role.role == Role::Perimeter && !role.entry => {
                self.trigger(cause).await;
            }
            _ => {}
        }
    }

    async fn name_of(&self, entity_id: &str) -> String {
        self.registry
            .get(entity_id)
            .await
            .map(|e| display_name(&e.meta))
            .unwrap_or_else(|| entity_id.to_string())
    }

    async fn trigger(&mut self, cause: Cause) {
        let now = Utc::now();
        let h24 = cause.h24;
        let first = self.status.phase != Phase::Triggered;
        // Une alerte 24 h/24 alarme coupée sonne sans « armer » la maison.
        if !(h24 && self.status.phase == Phase::Disarmed) {
            self.status.phase = Phase::Triggered;
            self.status.until = None;
            self.status.since = now;
        }
        self.status.cause = Some(cause.clone());
        if !h24 || self.config.h24_sirens {
            self.status.sirens_until =
                Some(now + chrono::TimeDelta::seconds(self.config.siren_duration_s as i64));
            for s in self.config.sirens.clone() {
                self.turn(&s, true).await;
            }
        }
        if !h24 && first {
            for l in self.config.lights.clone() {
                let _ = self
                    .registry
                    .command(
                        &l,
                        Action::TurnOn {
                            brightness: Some(255),
                            color_temp: None,
                        },
                        SOURCE,
                    )
                    .await;
            }
        }
        let (title, message) = if h24 {
            (
                "Alerte !".to_string(),
                format!("{} s'est déclenché.", cause.name),
            )
        } else {
            (
                "Alarme déclenchée".to_string(),
                format!(
                    "Intrusion : {} (mode {}).",
                    cause.name,
                    self.status.mode.unwrap_or(Mode::Away).label()
                ),
            )
        };
        self.log(if h24 { "h24" } else { "triggered" }, message.clone(), None);
        self.send(&title, message, Level::Urgent);
        self.save();
        if !self.config.cameras.is_empty() {
            for c in self.config.cameras.clone() {
                if let Err(e) = self.capture(&c).await {
                    self.log(
                        "snapshot",
                        format!("{} : photo impossible ({e})", c.name),
                        None,
                    );
                }
            }
        }
    }

    async fn silence(&mut self) {
        if self.status.sirens_until.take().is_some() || self.status.phase == Phase::Triggered {
            for s in self.config.sirens.clone() {
                self.turn(&s, false).await;
            }
        }
    }

    async fn turn(&self, entity_id: &str, on: bool) {
        let action = if on {
            Action::TurnOn {
                brightness: None,
                color_temp: None,
            }
        } else {
            Action::TurnOff
        };
        if let Err(e) = self.registry.command(entity_id, action, SOURCE).await {
            tracing::warn!("alarme : {entity_id} ne répond pas à la commande : {e:?}");
        }
    }

    #[cfg(feature = "webhook")]
    async fn capture(&mut self, camera: &Camera) -> Result<String, String> {
        let resp = self
            .http
            .get(&camera.snapshot_url)
            .send()
            .await
            .map_err(|e| format!("injoignable ({e})"))?;
        if !resp.status().is_success() {
            return Err(format!("réponse {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
            return Err("ce n'est pas une image JPEG".into());
        }
        std::fs::create_dir_all(&self.snapshots_dir).map_err(|e| e.to_string())?;
        let name = format!(
            "{}-{}.jpg",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            slugify(&camera.name)
        );
        std::fs::write(self.snapshots_dir.join(&name), &bytes).map_err(|e| e.to_string())?;
        for old in list_snapshots(&self.snapshots_dir)
            .into_iter()
            .skip(SNAPSHOTS_MAX)
        {
            let _ = std::fs::remove_file(self.snapshots_dir.join(old));
        }
        self.log(
            "snapshot",
            format!("Photo : {}", camera.name),
            Some(name.clone()),
        );
        Ok(name)
    }

    #[cfg(not(feature = "webhook"))]
    async fn capture(&mut self, _camera: &Camera) -> Result<String, String> {
        Err("Heim a été compilé sans accès HTTP (fonction webhook)".into())
    }

    fn send(&self, title: &str, message: String, level: Level) {
        let _ = self.notify.send(NotifyRequest {
            title: Some(title.to_string()),
            message,
            level,
            to: Vec::new(),
            except: Vec::new(),
            action: Some(json!({ "open": "security" })),
        });
    }

    fn log(&self, kind: &str, message: String, snapshot: Option<String>) {
        tracing::info!("alarme : {message}");
        let mut log: Vec<LogEntry> = self.settings.get(LOG_KEY).unwrap_or_default();
        log.insert(
            0,
            LogEntry {
                at: Utc::now(),
                kind: kind.into(),
                message,
                snapshot,
            },
        );
        log.truncate(LOG_MAX);
        let _ = self.settings.set(LOG_KEY, &log);
    }

    fn save(&self) {
        let _ = self.settings.set(STATE_KEY, &self.status);
        self.bus.publish(Event::Security {
            status: serde_json::to_value(&self.status).unwrap_or_default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_hash_roundtrip() {
        let c = new_code("1234").unwrap();
        assert_eq!(hash_code("1234", &c.salt), c.hash);
        assert_ne!(hash_code("1235", &c.salt), c.hash);
        assert!(new_code("12").is_err());
    }

    #[test]
    fn modes_watch_the_right_roles() {
        assert!(Mode::Away.watches(Role::Interior));
        assert!(!Mode::Night.watches(Role::Interior));
        assert!(Mode::Night.watches(Role::Perimeter));
    }

    #[test]
    fn public_config_hides_code() {
        let cfg = SecurityConfig {
            code: Some(new_code("0000").unwrap()),
            ..Default::default()
        };
        let v = cfg.public();
        assert!(v.get("code").is_none());
        assert_eq!(v["has_code"], json!(true));
        assert_eq!(v["exit_delay_s"], json!(60));
        assert_eq!(v["entry_delay_s"], json!(30));
    }

    #[test]
    fn snapshot_names_are_safe() {
        assert!(valid_snapshot_name("20261024-101500-entree.jpg"));
        assert!(!valid_snapshot_name("../heim.db"));
        assert!(!valid_snapshot_name("a/b.jpg"));
    }
}
