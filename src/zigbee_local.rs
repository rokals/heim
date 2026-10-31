//! Zigbee2MQTT sur la box elle-même.
//!
//! L'image SD installe Zigbee2MQTT (`/opt/zigbee2mqtt`) et son service systemd, éteint tant que
//! rien n'est réglé. Heim s'occupe du reste : il repère la clé USB, écrit la configuration de
//! Zigbee2MQTT (branchée sur le broker intégré), démarre le service et le relance après un
//! changement. On peut aussi reprendre un Zigbee2MQTT existant (celui de Home Assistant) :
//! sa configuration, sa base et la sauvegarde de la clé, pour ne rien réappairer.
//!
//! Hors du Pi (un PC, Windows), `HEIM_Z2M_CMD` fait tourner Zigbee2MQTT comme un programme
//! lancé par Heim : `HEIM_Z2M_CMD="node C:\zigbee2mqtt\index.js"`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use yaml_serde::{Mapping, Value as Yaml};

pub const SETTINGS_KEY: &str = "zigbee_local";
pub const SERVICE: &str = "zigbee2mqtt.service";
pub const FRONTEND_PORT: u16 = 8080;
const ADAPTERS: &[&str] = &[
    "ember", "zstack", "deconz", "zigate", "zboss", "zoh", "ezsp",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Le chemin de la clé (`/dev/serial/by-id/…`) ; vide : la première trouvée.
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default)]
    pub adapter: Option<String>,
    /// L'interface web de Zigbee2MQTT, sur le port 8080, protégée par un jeton.
    #[serde(default)]
    pub frontend: bool,
    #[serde(default)]
    pub frontend_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dongle {
    pub path: String,
    pub name: String,
    /// Le pilote que Zigbee2MQTT doit utiliser, quand on le reconnaît au nom.
    pub adapter: Option<&'static str>,
    pub model: Option<&'static str>,
    /// L'autre pilote possible quand le nom ne suffit pas (clé reflashée…). Heim l'essaie
    /// tout seul si le premier ne répond pas.
    pub alternate: Option<&'static str>,
}

/// Reconnaît les clés courantes à leur nom USB (`/dev/serial/by-id/…`).
pub fn guess(name: &str) -> (Option<&'static str>, Option<&'static str>) {
    let n = name.to_lowercase();
    let has = |s: &str| n.contains(s);
    if has("dongle_plus_v2") || has("dongle-e") || has("zbdongle-e") {
        return (Some("ember"), Some("Sonoff ZBDongle-E"));
    }
    if has("mg24") || has("dongle_max") || has("dongle_lite") {
        return (Some("ember"), Some("Sonoff Dongle (EFR32MG24)"));
    }
    if has("sonoff_zigbee_3.0_usb_dongle_plus") || has("zbdongle-p") {
        // Ce nom USB est aussi porté par des clés à puce Silicon Labs ou reflashées en
        // EmberZNet : zstack d'abord, ember en second essai (voir `alternate`).
        return (Some("zstack"), Some("Sonoff Zigbee 3.0 Dongle Plus"));
    }
    if has("skyconnect") || has("zbt-1") || has("connect_zbt") {
        return (Some("ember"), Some("Home Assistant Connect ZBT-1"));
    }
    if has("conbee") || has("raspbee") || has("dresden_elektronik") {
        return (Some("deconz"), Some("ConBee / RaspBee"));
    }
    if has("zigate") {
        return (Some("zigate"), Some("ZiGate"));
    }
    if has("slzb-07") || has("slzb-06m") {
        return (Some("ember"), Some("SMLIGHT (EFR32)"));
    }
    if has("slzb") || has("texas_instruments") || has("cc2652") || has("cc1352") || has("cc2531") {
        return (Some("zstack"), Some("Texas Instruments (CC2652…)"));
    }
    if has("silicon_labs") || has("efr32") || has("ember") {
        return (Some("ember"), None);
    }
    (None, None)
}

/// Le pilote à essayer si celui deviné au nom ne répond pas.
pub fn alternate(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    match guess(name).0 {
        Some("zstack") if n.contains("sonoff") || n.contains("itead") => Some("ember"),
        _ => None,
    }
}

/// Les clés série branchées. Sous Linux, `/dev/serial/by-id` donne des noms parlants.
pub fn scan(by_id: &Path) -> Vec<Dongle> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(by_id) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let (adapter, model) = guess(&name);
            let alternate = alternate(&name);
            out.push(Dongle {
                path: e.path().to_string_lossy().to_string(),
                name,
                adapter,
                model,
                alternate,
            });
        }
    }
    if out.is_empty()
        && let Ok(entries) = std::fs::read_dir("/dev")
    {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("ttyACM") || name.starts_with("ttyUSB") {
                out.push(Dongle {
                    path: format!("/dev/{name}"),
                    name,
                    adapter: None,
                    model: None,
                    alternate: None,
                });
            }
        }
    }
    // Les clés reconnues d'abord.
    out.sort_by_key(|d| (d.adapter.is_none(), d.name.clone()));
    out
}

fn key(s: &str) -> Yaml {
    Yaml::String(s.to_string())
}

fn section<'a>(root: &'a mut Mapping, name: &str) -> &'a mut Mapping {
    let entry = root
        .entry(key(name))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()));
    if !entry.is_mapping() {
        *entry = Yaml::Mapping(Mapping::new());
    }
    entry.as_mapping_mut().expect("mapping")
}

/// La configuration de Zigbee2MQTT, telle que Heim la veut : branchée sur son broker, la clé
/// choisie, les journaux dans un dossier fixe. Ce qui existe déjà (clé réseau, appareils,
/// groupes, canal…) est gardé tel quel.
pub fn render_config(
    existing: Option<&str>,
    s: &LocalSettings,
    port: Option<&str>,
    broker_port: u16,
    data_dir: &Path,
) -> Result<String, String> {
    let mut root = match existing {
        Some(text) if !text.trim().is_empty() => match yaml_serde::from_str::<Yaml>(text) {
            Ok(Yaml::Mapping(m)) => m,
            Ok(_) => return Err("configuration.yaml : ce n'est pas une configuration".into()),
            Err(e) => return Err(format!("configuration.yaml illisible : {e}")),
        },
        _ => Mapping::new(),
    };
    let fresh = root.is_empty();
    if fresh {
        // Zigbee2MQTT 2.x : configuration en version 5, clés réseau tirées au premier démarrage.
        root.insert(key("version"), Yaml::Number(5.into()));
    }
    {
        let mqtt = section(&mut root, "mqtt");
        mqtt.insert(
            key("server"),
            Yaml::String(format!("mqtt://127.0.0.1:{broker_port}")),
        );
        // Le broker de Heim accepte la box sans mot de passe.
        mqtt.remove(key("user"));
        mqtt.remove(key("password"));
        mqtt.remove(key("ca"));
        mqtt.remove(key("key"));
        mqtt.remove(key("cert"));
        mqtt.remove(key("client_id"));
        if !mqtt.contains_key(key("base_topic")) {
            mqtt.insert(key("base_topic"), key("zigbee2mqtt"));
        }
        mqtt.insert(key("version"), Yaml::Number(4.into()));
    }
    {
        let serial = section(&mut root, "serial");
        match port {
            Some(p) => {
                serial.insert(key("port"), key(p));
            }
            None => {
                serial.remove(key("port"));
            }
        }
        match &s.adapter {
            Some(a) if ADAPTERS.contains(&a.as_str()) => {
                serial.insert(key("adapter"), key(a));
            }
            Some(_) => return Err("pilote de clé inconnu".into()),
            None => {}
        }
    }
    {
        let frontend = section(&mut root, "frontend");
        frontend.insert(key("enabled"), Yaml::Bool(s.frontend));
        frontend.insert(key("port"), Yaml::Number(FRONTEND_PORT.into()));
        match (&s.frontend_token, s.frontend) {
            (Some(t), true) => {
                frontend.insert(key("auth_token"), key(t));
            }
            _ => {
                frontend.remove(key("auth_token"));
            }
        }
    }
    // Heim remplace Home Assistant : pas de messages de découverte pour lui.
    section(&mut root, "homeassistant").insert(key("enabled"), Yaml::Bool(false));
    section(&mut root, "availability").insert(key("enabled"), Yaml::Bool(true));
    {
        let adv = section(&mut root, "advanced");
        if fresh {
            for k in ["network_key", "pan_id", "ext_pan_id"] {
                adv.insert(key(k), key("GENERATE"));
            }
        }
        if !adv.contains_key(key("log_level")) {
            adv.insert(key("log_level"), key("info"));
        }
        adv.insert(
            key("log_output"),
            Yaml::Sequence(vec![key("console"), key("file")]),
        );
        adv.insert(
            key("log_directory"),
            key(&data_dir.join("log").to_string_lossy()),
        );
        adv.insert(key("log_rotation"), Yaml::Bool(false));
    }
    // Sans assistant de Zigbee2MQTT : c'est Heim qui règle.
    root.remove(key("onboarding"));
    yaml_serde::to_string(&Yaml::Mapping(root)).map_err(|e| e.to_string())
}

/// Lit le réglage du premier niveau d'une configuration (pour l'interface).
fn peek(text: &str) -> Value {
    let Ok(Yaml::Mapping(m)) = yaml_serde::from_str::<Yaml>(text) else {
        return Value::Null;
    };
    let get = |a: &str, b: &str| {
        m.get(key(a))
            .and_then(|v| v.get(key(b)))
            .map(|v| match v {
                Yaml::String(s) => Value::String(s.clone()),
                Yaml::Number(n) => n.as_i64().map(Value::from).unwrap_or(Value::Null),
                Yaml::Bool(b) => Value::Bool(*b),
                _ => Value::Null,
            })
            .unwrap_or(Value::Null)
    };
    let devices = m
        .get(key("devices"))
        .and_then(Yaml::as_mapping)
        .map(|d| d.len())
        .unwrap_or(0);
    json!({
        "base_topic": get("mqtt", "base_topic"),
        "port": get("serial", "port"),
        "adapter": get("serial", "adapter"),
        "channel": get("advanced", "channel"),
        "pan_id": get("advanced", "pan_id"),
        "devices": devices,
    })
}

/// Comment Zigbee2MQTT est lancé.
#[derive(Debug, Clone, PartialEq)]
pub enum Runner {
    /// Le service systemd de l'image SD.
    Systemd,
    /// Un programme lancé et surveillé par Heim (PC, Windows).
    Child(String),
    /// Pas installé.
    Missing,
}

struct ChildState {
    handle: Option<tokio::task::JoinHandle<()>>,
    running: bool,
    last_exit: Option<String>,
}

pub struct ZigbeeLocal {
    /// Le dossier des données de Zigbee2MQTT (configuration, base, journaux).
    pub data: PathBuf,
    pub runner: Runner,
    install: Option<PathBuf>,
    child: Mutex<ChildState>,
    by_id: PathBuf,
    /// Pilotes déjà essayés sans succès, par clé (pour ne pas boucler).
    tried: Mutex<std::collections::HashSet<(String, String)>>,
}

impl ZigbeeLocal {
    pub fn detect(heim_data: &Path) -> Arc<Self> {
        let data = std::env::var_os("HEIM_Z2M_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| heim_data.join("zigbee2mqtt"));
        let install = std::env::var_os("HEIM_Z2M_DIR")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("/opt/zigbee2mqtt")))
            .filter(|p| {
                p.join("index.js").exists() || p.join("node_modules/zigbee2mqtt/index.js").exists()
            });
        let runner = match std::env::var("HEIM_Z2M_CMD") {
            Ok(cmd) if !cmd.trim().is_empty() => Runner::Child(cmd),
            _ if Path::new("/etc/systemd/system").join(SERVICE).exists()
                || Path::new("/lib/systemd/system").join(SERVICE).exists() =>
            {
                Runner::Systemd
            }
            _ => Runner::Missing,
        };
        let mut me = Self::with(data, runner, install);
        if let Some(dir) = std::env::var_os("HEIM_SERIAL_DIR")
            && let Some(z) = Arc::get_mut(&mut me)
        {
            z.by_id = PathBuf::from(dir);
        }
        me
    }

    pub fn with(data: PathBuf, runner: Runner, install: Option<PathBuf>) -> Arc<Self> {
        Arc::new(ZigbeeLocal {
            data,
            runner,
            install,
            child: Mutex::new(ChildState {
                handle: None,
                running: false,
                last_exit: None,
            }),
            by_id: PathBuf::from("/dev/serial/by-id"),
            tried: Mutex::new(Default::default()),
        })
    }

    /// Note qu'un pilote n'a pas marché sur cette clé.
    pub fn mark_tried(&self, port: &str, adapter: &str) {
        self.tried
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((port.to_string(), adapter.to_string()));
    }

    pub fn was_tried(&self, port: &str, adapter: &str) -> bool {
        self.tried
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&(port.to_string(), adapter.to_string()))
    }

    pub fn installed(&self) -> bool {
        self.runner != Runner::Missing
    }

    fn config_path(&self) -> PathBuf {
        self.data.join("configuration.yaml")
    }

    fn disabled_flag(&self) -> PathBuf {
        self.data.join(".heim-disabled")
    }

    pub fn dongles(&self) -> Vec<Dongle> {
        scan(&self.by_id)
    }

    pub fn version(&self) -> Option<String> {
        let dir = self.install.as_ref()?;
        [
            dir.join("node_modules/zigbee2mqtt/package.json"),
            dir.join("package.json"),
        ]
        .into_iter()
        .filter_map(|p| serde_json::from_slice::<Value>(&std::fs::read(p).ok()?).ok())
        .find(|v| v.get("name").and_then(Value::as_str) == Some("zigbee2mqtt"))?
        .get("version")?
        .as_str()
        .map(str::to_string)
    }

    /// Écrit la configuration. Renvoie le port de la clé retenu.
    pub fn write_config(
        &self,
        s: &LocalSettings,
        broker_port: u16,
    ) -> Result<Option<String>, String> {
        std::fs::create_dir_all(&self.data).map_err(|e| e.to_string())?;
        let dongles = self.dongles();
        // Un chemin enregistré mais absent (clé débranchée, autre clé) : la clé branchée prend
        // le relais, sinon Zigbee2MQTT s'arrêterait sur « port introuvable ».
        let port = s
            .port
            .clone()
            .filter(|p| !p.trim().is_empty())
            .filter(|p| {
                dongles.is_empty() || dongles.iter().any(|d| &d.path == p) || Path::new(p).exists()
            })
            .or_else(|| dongles.first().map(|d| d.path.clone()));
        let existing = std::fs::read_to_string(self.config_path()).ok();
        let mut wanted = s.clone();
        // Le pilote : celui choisi, sinon celui déjà dans la configuration (reprise d'une
        // sauvegarde, essai précédent qui a marché), sinon celui deviné au nom de la clé.
        if wanted.adapter.is_none() {
            wanted.adapter = existing
                .as_deref()
                .and_then(|t| {
                    peek(t)
                        .get("adapter")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .filter(|a| ADAPTERS.contains(&a.as_str()));
        }
        if wanted.adapter.is_none()
            && let Some(p) = &port
        {
            wanted.adapter = guess(p).0.map(str::to_string);
        }
        let text = render_config(
            existing.as_deref(),
            &wanted,
            port.as_deref(),
            broker_port,
            &self.data,
        )?;
        if existing.as_deref() != Some(text.as_str()) {
            let tmp = self.data.join("configuration.yaml.tmp");
            std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, self.config_path()).map_err(|e| e.to_string())?;
        }
        Ok(port)
    }

    /// Reprend les fichiers d'un autre Zigbee2MQTT. Les anciens sont gardés à côté.
    pub fn import(
        &self,
        configuration: &str,
        database: Option<&[u8]>,
        backup: Option<&str>,
    ) -> Result<Value, String> {
        match yaml_serde::from_str::<Yaml>(configuration) {
            Ok(Yaml::Mapping(_)) => {}
            _ => return Err("configuration.yaml illisible".into()),
        }
        if let Some(b) = backup
            && serde_json::from_str::<Value>(b).is_err()
        {
            return Err("coordinator_backup.json illisible".into());
        }
        if let Some(db) = database
            && !db.is_empty()
            && !db.trim_ascii_start().starts_with(b"{")
        {
            return Err("database.db illisible (une ligne JSON par appareil attendue)".into());
        }
        std::fs::create_dir_all(&self.data).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        for name in [
            "configuration.yaml",
            "database.db",
            "coordinator_backup.json",
            "state.json",
        ] {
            let p = self.data.join(name);
            if p.exists() {
                let _ = std::fs::rename(&p, self.data.join(format!("{name}.avant-{stamp}")));
            }
        }
        std::fs::write(self.config_path(), configuration).map_err(|e| e.to_string())?;
        if let Some(db) = database {
            std::fs::write(self.data.join("database.db"), db).map_err(|e| e.to_string())?;
        }
        if let Some(b) = backup {
            std::fs::write(self.data.join("coordinator_backup.json"), b)
                .map_err(|e| e.to_string())?;
        }
        Ok(self.config_summary())
    }

    /// Les autres fichiers d'une sauvegarde (secret.yaml, state.json…), à côté de la configuration.
    pub fn import_extra(&self, files: &[(String, Vec<u8>)]) -> Result<usize, String> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut n = 0;
        for (name, data) in files {
            let plain = name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
            let kind = [".yaml", ".yml", ".json", ".db"]
                .iter()
                .any(|x| name.ends_with(x));
            if !plain
                || !kind
                || name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "configuration.yaml" | "database.db" | "coordinator_backup.json"
                )
            {
                continue;
            }
            let p = self.data.join(name);
            if p.exists() && name != "state.json" {
                let _ = std::fs::rename(&p, self.data.join(format!("{name}.avant-{stamp}")));
            }
            std::fs::write(&p, data).map_err(|e| e.to_string())?;
            n += 1;
        }
        Ok(n)
    }

    pub fn config_summary(&self) -> Value {
        let Ok(text) = std::fs::read_to_string(self.config_path()) else {
            return Value::Null;
        };
        let mut summary = peek(&text);
        // Les appareils sans nom donné n'ont pas d'entrée dans la configuration : on compte
        // aussi ceux de la base (hors coordinateur).
        if let Ok(db) = std::fs::read(self.data.join("database.db")) {
            let n = String::from_utf8_lossy(&db)
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|d| d.get("type").and_then(Value::as_str) != Some("Coordinator"))
                .count() as u64;
            if n > summary["devices"].as_u64().unwrap_or(0) {
                summary["devices"] = n.into();
            }
        }
        summary
    }

    /// Les dernières lignes du journal de Zigbee2MQTT.
    pub fn log_tail(&self, lines: usize) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(self.data.join("log").join("log.log")) else {
            return Vec::new();
        };
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..]
            .iter()
            .map(|l| l.chars().take(400).collect())
            .collect()
    }

    /// Le réseau Zigbee de la clé est connu : Zigbee2MQTT a déjà tourné ici (il écrit alors
    /// `coordinator_backup.json`) ou une sauvegarde a été reprise. Sans lui, démarrer créerait un
    /// nouveau réseau sur une clé qui en porte peut-être déjà un (celui de Home Assistant).
    pub fn network_known(&self) -> bool {
        self.data.join("coordinator_backup.json").exists()
    }

    /// Les dernières lignes de Zigbee2MQTT dans le journal système (service de l'image SD).
    /// Vide ailleurs, ou si Heim n'a pas le droit de lire le journal.
    pub async fn journal_tail(&self, lines: usize) -> Vec<String> {
        if self.runner != Runner::Systemd {
            return Vec::new();
        }
        let out = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new("journalctl")
                .args([
                    "-u",
                    SERVICE,
                    "-n",
                    &lines.to_string(),
                    "-o",
                    "cat",
                    "--no-pager",
                    "-q",
                ])
                .output(),
        )
        .await;
        match out {
            Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| strip_ansi(l).chars().take(400).collect())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// « active », « inactive », « failed », « activating »… ou « missing ».
    pub async fn state(&self) -> String {
        match &self.runner {
            Runner::Missing => "missing".into(),
            Runner::Child(_) => {
                let c = self.child.lock().unwrap_or_else(|e| e.into_inner());
                if c.running {
                    "active".into()
                } else if c.last_exit.is_some() {
                    // Arrêté sur une erreur, en attente de la relance.
                    "failed".into()
                } else if c.handle.as_ref().is_some_and(|h| !h.is_finished()) {
                    "activating".into()
                } else {
                    "inactive".into()
                }
            }
            Runner::Systemd => {
                let out = tokio::process::Command::new("systemctl")
                    .args(["show", SERVICE, "--property=ActiveState,SubState"])
                    .output()
                    .await;
                match out {
                    Ok(o) => {
                        let text = String::from_utf8_lossy(&o.stdout).to_string();
                        let get = |k: &str| {
                            text.lines()
                                .find_map(|l| l.strip_prefix(k))
                                .unwrap_or("")
                                .trim()
                                .to_string()
                        };
                        // En boucle de relance après une erreur : c'est un échec, pas un démarrage.
                        if get("SubState=") == "auto-restart" {
                            "failed".into()
                        } else {
                            let a = get("ActiveState=");
                            if a.is_empty() { "unknown".into() } else { a }
                        }
                    }
                    Err(_) => "unknown".into(),
                }
            }
        }
    }

    /// Démarre (ou relance) Zigbee2MQTT.
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        let _ = std::fs::remove_file(self.disabled_flag());
        match &self.runner {
            Runner::Missing => Err("Zigbee2MQTT n'est pas installé sur cette box".into()),
            Runner::Systemd => systemctl("restart").await,
            Runner::Child(cmd) => {
                self.stop_child();
                let me = self.clone();
                let cmd = cmd.clone();
                let handle = tokio::spawn(async move { me.supervise(cmd).await });
                self.child.lock().unwrap_or_else(|e| e.into_inner()).handle = Some(handle);
                Ok(())
            }
        }
    }

    pub async fn stop(&self) -> Result<(), String> {
        let _ = std::fs::create_dir_all(&self.data);
        let _ = std::fs::write(self.disabled_flag(), b"");
        match &self.runner {
            Runner::Missing => Ok(()),
            Runner::Systemd => systemctl("stop").await,
            Runner::Child(_) => {
                self.stop_child();
                Ok(())
            }
        }
    }

    fn stop_child(&self) {
        let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(h) = c.handle.take() {
            h.abort();
        }
        c.running = false;
        c.last_exit = None;
    }

    async fn supervise(self: Arc<Self>, cmd: String) {
        let mut backoff = Duration::from_secs(2);
        loop {
            let mut parts = cmd.split_whitespace();
            let Some(program) = parts.next() else { return };
            let mut command = tokio::process::Command::new(program);
            command
                .args(parts)
                .env("ZIGBEE2MQTT_DATA", &self.data)
                .env("Z2M_ONBOARD_NO_SERVER", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true);
            if let Some(dir) = &self.install {
                command.current_dir(dir);
            }
            let started = std::time::Instant::now();
            match command.spawn() {
                Ok(mut child) => {
                    self.child.lock().unwrap_or_else(|e| e.into_inner()).running = true;
                    tracing::info!("zigbee2mqtt local : démarré ({cmd})");
                    let status = child.wait().await;
                    let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
                    c.running = false;
                    c.last_exit = Some(format!("{status:?}"));
                    tracing::warn!(
                        "zigbee2mqtt local : arrêté ({status:?}), relance dans {backoff:?}"
                    );
                }
                Err(e) => {
                    let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
                    c.last_exit = Some(e.to_string());
                    tracing::warn!("zigbee2mqtt local : impossible de lancer « {cmd} » : {e}");
                }
            }
            if started.elapsed() > Duration::from_secs(120) {
                backoff = Duration::from_secs(2);
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }
}

/// Les couleurs de la console de Zigbee2MQTT, retirées.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// ModemManager tourne (il envoie des commandes de modem aux ports série USB, clés Zigbee
/// comprises). Coupé par la mise à jour système à partir de la 0.17.4.
pub async fn modem_manager_active() -> bool {
    tokio::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "ModemManager.service"])
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// Le message de l'erreur qui a arrêté Zigbee2MQTT : la ligne « Error: … » d'avant la pile
/// d'appels (les lignes « at … »), sinon la dernière ligne d'erreur de son journal.
pub fn error_line(log: &[String]) -> Option<String> {
    let thrown = log.iter().rev().find(|l| {
        let t = l.trim_start();
        !t.starts_with("at ")
            && t.split_once(": ").is_some_and(|(head, _)| {
                head.ends_with("Error") && !head.contains(' ') && !head.contains('[')
            })
    });
    let line = thrown.or_else(|| {
        log.iter().rev().find(|l| {
            let t = l.to_ascii_lowercase();
            t.contains("error:")
                && !t.contains("error while starting zigbee-herdsman")
                && !t.contains("failed to start zigbee-herdsman")
                && !t.contains("exiting")
                && !t.contains("check https://")
        })
    })?;
    Some(line.trim().chars().take(300).collect())
}

/// Une explication en clair des pannes courantes, d'après le journal. Rien si on ne sait pas.
pub fn explain(log: &[String], dongles: &[Dongle], modem_manager: bool) -> Option<&'static str> {
    let text = log
        .iter()
        .rev()
        .take(60)
        .map(|l| l.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let has = |k: &str| text.contains(k);
    if has("not consistent with adapter") || has("configuration-adapter mismatch") {
        Some(
            "La clé porte un autre réseau Zigbee que celui de la configuration. Reprenez la sauvegarde de l'ancien Zigbee2MQTT (Reprendre les appareils de Home Assistant), elle remet le bon réseau sur la clé.",
        )
    } else if has("resource busy") || has("cannot lock port") || has("port is locked") {
        Some("La clé est déjà utilisée par un autre programme.")
    } else if has("permission denied") && (has("/dev/tty") || has("/dev/serial")) {
        Some("Zigbee2MQTT n'a pas le droit d'ouvrir la clé (groupe dialout).")
    } else if dongles.is_empty()
        || has("no such file or directory, cannot open /dev")
        || has("no valid usb adapter found")
        || has("failed to find a suitable adapter")
    {
        Some(
            "Aucune clé Zigbee trouvée : branchez-la. Zigbee2MQTT redémarre tout seul dès qu'elle apparaît.",
        )
    } else if modem_manager && (has("failed to connect to the adapter") || has("sys - ping")) {
        Some(
            "ModemManager tourne sur la box et perturbe la clé. Mettez à jour Heim (la mise à jour système le coupe), puis débranchez et rebranchez la clé.",
        )
    } else if has("sys - ping") {
        Some(
            "La clé ne répond pas en Z-Stack (Texas Instruments). Si elle a un firmware EmberZNet (Sonoff E, clé reflashée), choisissez le pilote « ember » dans Réglages avancés. Heim l'essaie tout seul quand aucun pilote n'est choisi.",
        )
    } else if has("failed to connect to the adapter")
        || has("sreq '") && has("timeout")
        || has("failed to start zigbee-herdsman")
    {
        Some(
            "La clé ne répond pas. Débranchez-la, attendez 5 secondes, rebranchez-la (de préférence avec une rallonge USB, loin du Raspberry Pi).",
        )
    } else if has("connect econnrefused") && has("1883") {
        Some("Le broker MQTT de la box ne répond pas : redémarrez Heim (Réglages › Système).")
    } else {
        None
    }
}

async fn systemctl(verb: &str) -> Result<(), String> {
    let out = tokio::process::Command::new("systemctl")
        .args(["--no-ask-password", verb, SERVICE])
        .output()
        .await
        .map_err(|e| format!("systemctl : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        tracing::warn!("systemctl {verb} {SERVICE} : {err}");
        Err(format!("impossible de piloter Zigbee2MQTT : {err}"))
    }
}

/// Un jeton pour l'interface web de Zigbee2MQTT.
pub fn generate_token() -> String {
    let mut raw = [0u8; 16];
    let _ = getrandom::getrandom(&mut raw);
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_real_error() {
        let log: Vec<String> = [
            "[2026-10-31 11:36:20] error: \tz2m: Error while starting zigbee-herdsman",
            "[2026-10-31 11:36:20] error: \tz2m: Failed to start zigbee-herdsman",
            "[2026-10-31 11:36:20] error: \tz2m: Check https://www.zigbee2mqtt.io/guide/installation/20_zigbee2mqtt-fails-to-start_crashes-runtime.html for possible solutions",
            "[2026-10-31 11:36:20] error: \tz2m: Exiting...",
            "Error: SRSP - SYS - ping after 6000ms",
            "    at Timeout._onTimeout (/opt/zigbee2mqtt/node_modules/zigbee-herdsman/src/utils/waitress.ts:64:35)",
            "zigbee2mqtt.service: Failed with result 'exit-code'.",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            error_line(&log).as_deref(),
            Some("Error: SRSP - SYS - ping after 6000ms")
        );
        let short =
            vec!["[x] error: \tz2m: MQTT error: connect ECONNREFUSED 127.0.0.1:1883".to_string()];
        assert!(error_line(&short).is_some());
    }

    #[test]
    fn dongles_are_recognised() {
        assert_eq!(
            guess("usb-ITead_Sonoff_Zigbee_3.0_USB_Dongle_Plus_V2_20240122184111-if00").0,
            Some("ember")
        );
        assert_eq!(
            guess("usb-Silicon_Labs_Sonoff_Zigbee_3.0_USB_Dongle_Plus_0001-if00-port0").0,
            Some("zstack")
        );
        assert_eq!(
            guess("usb-dresden_elektronik_ingenieurtechnik_GmbH_ConBee_II_DE2652-if00").0,
            Some("deconz")
        );
        assert_eq!(guess("usb-1a86_USB_Serial-if00-port0").0, None);
    }

    #[test]
    fn fresh_config() {
        let s = LocalSettings {
            enabled: true,
            adapter: Some("ember".into()),
            ..Default::default()
        };
        let text = render_config(
            None,
            &s,
            Some("/dev/ttyACM0"),
            1883,
            Path::new("/var/lib/heim/zigbee2mqtt"),
        )
        .unwrap();
        let v: Yaml = yaml_serde::from_str(&text).unwrap();
        assert_eq!(v["version"], Yaml::Number(5.into()));
        assert_eq!(v["mqtt"]["server"], key("mqtt://127.0.0.1:1883"));
        assert_eq!(v["serial"]["adapter"], key("ember"));
        assert_eq!(v["advanced"]["network_key"], key("GENERATE"));
        assert_eq!(v["homeassistant"]["enabled"], Yaml::Bool(false));
        assert_eq!(v["frontend"]["enabled"], Yaml::Bool(false));
    }

    #[test]
    fn imported_config_keeps_the_network() {
        let old = r#"
version: 4
homeassistant:
  enabled: true
mqtt:
  base_topic: zigbee2mqtt
  server: mqtt://core-mosquitto:1883
  user: addons
  password: secret
serial:
  port: /dev/ttyUSB0
  adapter: ember
advanced:
  network_key: [1, 2, 3]
  pan_id: 6754
  channel: 25
devices:
  '0x00158d0001a2b3c4':
    friendly_name: Salon/Plafonnier
"#;
        let s = LocalSettings {
            enabled: true,
            frontend: true,
            frontend_token: Some("abc".into()),
            ..Default::default()
        };
        let text = render_config(
            Some(old),
            &s,
            Some("/dev/serial/by-id/usb-x"),
            1883,
            Path::new("/d"),
        )
        .unwrap();
        let v: Yaml = yaml_serde::from_str(&text).unwrap();
        assert_eq!(v["advanced"]["pan_id"], Yaml::Number(6754.into()));
        assert_eq!(v["advanced"]["channel"], Yaml::Number(25.into()));
        assert!(v["advanced"]["network_key"].is_sequence());
        assert!(v["mqtt"].get("password").is_none());
        assert_eq!(v["mqtt"]["server"], key("mqtt://127.0.0.1:1883"));
        assert_eq!(v["serial"]["port"], key("/dev/serial/by-id/usb-x"));
        assert_eq!(v["serial"]["adapter"], key("ember"));
        assert_eq!(v["frontend"]["auth_token"], key("abc"));
        assert_eq!(
            v["devices"]["0x00158d0001a2b3c4"]["friendly_name"],
            key("Salon/Plafonnier")
        );
        assert_eq!(peek(&text)["devices"], json!(1));
    }

    #[test]
    fn import_checks_files() {
        let dir = std::env::temp_dir().join(format!("heim-z2m-{}", std::process::id()));
        let z = ZigbeeLocal::with(dir.clone(), Runner::Missing, None);
        assert!(z.import("pas: [du yaml", None, None).is_err());
        assert!(z.import("mqtt: {}", Some(b"pas du json"), None).is_err());
        let r = z
            .import(
                "serial: {port: /dev/ttyUSB0}\n",
                Some(b"{\"id\":1}\n"),
                Some("{}"),
            )
            .unwrap();
        assert_eq!(r["port"], json!("/dev/ttyUSB0"));
        assert!(dir.join("database.db").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_driver_in_the_configuration_wins_over_the_guess() {
        // Une Sonoff « Dongle Plus » reflashée en EmberZNet : la sauvegarde dit ember, le nom
        // de la clé ferait deviner zstack. La configuration doit garder ember.
        let base = std::env::temp_dir().join(format!("heim-z2m-ember-{}", std::process::id()));
        let serial = base.join("by-id");
        std::fs::create_dir_all(&serial).unwrap();
        let name = "usb-ITead_Sonoff_Zigbee_3.0_USB_Dongle_Plus_7490-if00-port0";
        std::fs::write(serial.join(name), b"").unwrap();
        let data = base.join("data");
        let mut z = ZigbeeLocal::with(data.clone(), Runner::Missing, None);
        Arc::get_mut(&mut z).unwrap().by_id = serial.clone();
        z.import(
            "serial: {adapter: ember}\nmqtt: {base_topic: zigbee2mqtt}\n",
            None,
            None,
        )
        .unwrap();
        z.write_config(&LocalSettings::default(), 1883).unwrap();
        assert_eq!(z.config_summary()["adapter"], json!("ember"));
        // Sans rien dans la configuration : le nom fait deviner zstack, ember en second essai.
        assert_eq!(guess(name).0, Some("zstack"));
        assert_eq!(alternate(name), Some("ember"));
        assert_eq!(
            alternate("usb-ITead_Sonoff_Zigbee_3.0_USB_Dongle_Plus_V2-if00"),
            None
        );
        // Un choix explicite passe devant tout.
        let chosen = LocalSettings {
            adapter: Some("zstack".into()),
            ..Default::default()
        };
        z.write_config(&chosen, 1883).unwrap();
        assert_eq!(z.config_summary()["adapter"], json!("zstack"));
        let _ = std::fs::remove_dir_all(base);
    }
}
