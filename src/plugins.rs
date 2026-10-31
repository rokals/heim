//! Les plugins : chaque connexion se choisit à la carte.
//!
//! - **Intégrés** (Zigbee, Tuya, Matter, découverte MQTT) : déjà dans Heim, « installer »
//!   veut dire les faire apparaître dans Connexions. Rien à télécharger.
//! - **Externes** : un dépôt avec un fichier `heim-plugin.json` (nom, catégorie, version,
//!   programmes compilés par machine avec leur SHA-256, réglages à demander). Heim télécharge
//!   le programme qui correspond à la box, vérifie l'empreinte, puis le lance à côté de lui :
//!   un plugin qui plante ne fait pas tomber Heim. Le plugin parle à Heim par le broker MQTT,
//!   au format de découverte de Home Assistant (voir `integrations::mqtt_discovery`).
//!
//! Le store officiel est `plugins/store.json` du dépôt de Heim (une copie est gravée dans le
//! programme, pour marcher sans Internet). N'importe quel autre dépôt peut être ajouté : il
//! est alors marqué « communauté ».

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const SETTINGS_KEY: &str = "plugins";
/// Les connexions compilées dans Heim.
pub const BUILTIN: &[&str] = &[
    "zigbee2mqtt",
    "mqtt_discovery",
    "tuya",
    "matter",
    "screen35",
];
pub const CATEGORIES: &[&str] = &["connections", "display", "features", "network"];
const EMBEDDED_STORE: &str = include_str!("../plugins/store.json");
const MAX_BINARY: usize = 64 * 1024 * 1024;
const LOG_LINES: usize = 200;

/// Un texte simple, ou un texte par langue (`{"fr": "...", "en": "..."}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    One(String),
    Many(BTreeMap<String, String>),
}

impl Default for Text {
    fn default() -> Self {
        Text::One(String::new())
    }
}

impl Text {
    pub fn get(&self, lang: &str) -> String {
        match self {
            Text::One(s) => s.clone(),
            Text::Many(m) => m
                .get(lang)
                .or_else(|| m.get("en"))
                .or_else(|| m.get("fr"))
                .or_else(|| m.values().next())
                .cloned()
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binary {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingDef {
    pub key: String,
    /// string, password, number, boolean, select
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub label: Text,
    #[serde(default)]
    pub help: Option<Text>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub options: Vec<Value>,
}

fn default_kind() -> String {
    "string".into()
}

fn default_category() -> String {
    "connections".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_en: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Text,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Mémoire prévue, en Mo (affichée avant d'installer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_note: Option<Text>,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub binaries: BTreeMap<String, Binary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingDef>,
    /// network, usb, mqtt… (montré avant d'installer).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Dans le store : où lire le manifeste complet d'un plugin externe officiel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_url: Option<String>,
}

impl Manifest {
    pub fn display_name(&self, lang: &str) -> String {
        match (&self.name_en, lang) {
            (Some(en), "en") => en.clone(),
            _ => self.name.clone(),
        }
    }

    /// Ce qu'un manifeste externe doit respecter avant d'être installé.
    pub fn validate_external(&self) -> Result<&Binary, String> {
        if !valid_id(&self.id) {
            return Err(
                "identifiant de plugin invalide (a-z, 0-9, - et _, 2 à 40 caractères)".into(),
            );
        }
        if BUILTIN.contains(&self.id.as_str()) || self.builtin {
            return Err("ce nom est réservé à une connexion intégrée".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 60 {
            return Err("nom de plugin manquant ou trop long".into());
        }
        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(format!("catégorie inconnue : {}", self.category));
        }
        if self.settings.len() > 40 {
            return Err("trop de réglages".into());
        }
        let key = arch_key().ok_or("cette machine n'est pas prise en charge")?;
        let bin = self
            .binaries
            .get(key)
            .ok_or_else(|| format!("ce plugin n'existe pas pour cette machine ({key})"))?;
        if bin.sha256.len() != 64 || !bin.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("empreinte SHA-256 invalide dans le manifeste".into());
        }
        if !allowed_url(&bin.url) {
            return Err("adresse de téléchargement refusée (https seulement)".into());
        }
        Ok(bin)
    }
}

pub fn valid_id(id: &str) -> bool {
    (2..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// https partout ; http seulement vers la machine elle-même (essais).
fn allowed_url(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
}

/// La clé des programmes de ce Heim : aarch64-linux, armv7-linux, x86_64-linux, x86_64-windows.
pub fn arch_key() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "linux") => Some("aarch64-linux"),
        ("arm", "linux") => Some("armv7-linux"),
        ("x86_64", "linux") => Some("x86_64-linux"),
        ("x86_64", "windows") => Some("x86_64-windows"),
        ("aarch64", "macos") => Some("aarch64-macos"),
        _ => None,
    }
}

/// « alex/heim-wled », « https://github.com/alex/heim-wled » ou l'adresse d'un manifeste.
pub fn manifest_url(source: &str) -> Result<String, String> {
    let s = source.trim().trim_end_matches('/');
    if s.ends_with(".json") && allowed_url(s) {
        return Ok(s.to_string());
    }
    let repo = s
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/")
        .trim_end_matches(".git");
    let parts: Vec<&str> = repo.split('/').collect();
    let ok = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if parts.len() == 2 && ok(parts[0]) && ok(parts[1]) {
        return Ok(format!(
            "https://raw.githubusercontent.com/{}/{}/HEAD/heim-plugin.json",
            parts[0], parts[1]
        ));
    }
    Err("dépôt attendu sous la forme proprietaire/nom, ou l'adresse d'un heim-plugin.json".into())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    #[serde(default)]
    pub builtin: bool,
    /// Plugin externe : l'adresse du manifeste.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub official: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Manifest>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub settings: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<DateTime<Utc>>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginsSettings {
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    /// Le store officiel (par défaut : celui du dépôt de Heim).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_url: Option<String>,
}

impl PluginsSettings {
    pub fn has(&self, id: &str) -> bool {
        self.installed.contains_key(id)
    }

    pub fn enabled(&self, id: &str) -> bool {
        self.installed.get(id).is_some_and(|p| p.enabled)
    }
}

pub fn embedded_store() -> Vec<Manifest> {
    #[derive(Deserialize)]
    struct Store {
        plugins: Vec<Manifest>,
    }
    serde_json::from_str::<Store>(EMBEDDED_STORE)
        .map(|s| s.plugins)
        .unwrap_or_default()
}

/// Vérifie et complète les réglages saisis pour un plugin.
pub fn clean_settings(
    defs: &[SettingDef],
    input: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for d in defs {
        let v = input.get(&d.key).cloned().filter(|v| !v.is_null());
        let v = match (v, &d.default) {
            (Some(v), _) => v,
            (None, Some(def)) => def.clone(),
            (None, None) => {
                if d.required {
                    return Err(format!("réglage obligatoire : {}", d.label.get("fr")));
                }
                continue;
            }
        };
        let v = match d.kind.as_str() {
            "number" => match &v {
                Value::Number(_) => v,
                Value::String(s) if s.trim().is_empty() && !d.required => continue,
                Value::String(s) => s
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
                    .ok_or_else(|| format!("{} : nombre attendu", d.label.get("fr")))?,
                _ => return Err(format!("{} : nombre attendu", d.label.get("fr"))),
            },
            "boolean" => Value::Bool(
                v.as_bool()
                    .unwrap_or(matches!(v.as_str(), Some("true" | "1"))),
            ),
            "select" => {
                if !d.options.is_empty() && !d.options.contains(&v) {
                    return Err(format!("{} : choix inconnu", d.label.get("fr")));
                }
                v
            }
            _ => {
                let s = v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string());
                if s.len() > 2000 {
                    return Err(format!("{} : trop long", d.label.get("fr")));
                }
                if s.trim().is_empty() && d.required {
                    return Err(format!("réglage obligatoire : {}", d.label.get("fr")));
                }
                Value::String(s)
            }
        };
        out.insert(d.key.clone(), v);
    }
    Ok(out)
}

/// Ce qu'on montre d'un plugin installé : les mots de passe ne repartent pas.
pub fn public_settings(defs: &[SettingDef], s: &Map<String, Value>) -> Map<String, Value> {
    s.iter()
        .map(|(k, v)| {
            let secret = defs.iter().any(|d| &d.key == k && d.kind == "password");
            (
                k.clone(),
                if secret && !v.as_str().unwrap_or("").is_empty() {
                    json!("••••••")
                } else {
                    v.clone()
                },
            )
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunState {
    pub running: bool,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_exit: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub log: VecDeque<String>,
}

/// Comment joindre le broker, pour les plugins.
#[derive(Debug, Clone, Default)]
pub struct MqttEnv {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

struct Runtime {
    handle: tokio::task::JoinHandle<()>,
    state: Arc<Mutex<RunState>>,
}

pub struct Plugins {
    pub dir: PathBuf,
    runtime: Mutex<HashMap<String, Runtime>>,
    cache: Mutex<Option<(Instant, Vec<Manifest>)>>,
    #[cfg(feature = "webhook")]
    http: reqwest::Client,
}

impl Plugins {
    pub fn new(data_dir: &Path) -> Arc<Self> {
        Arc::new(Plugins {
            dir: data_dir.join("plugins"),
            runtime: Mutex::new(HashMap::new()),
            cache: Mutex::new(None),
            #[cfg(feature = "webhook")]
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .user_agent(concat!("heim/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        })
    }

    /// Le store : la copie gravée, complétée par le store en ligne (gardé une heure).
    pub async fn store(&self, url: Option<&str>) -> Vec<Manifest> {
        let mut list = embedded_store();
        if let Some((at, cached)) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone()
            && at.elapsed() < Duration::from_secs(3600)
        {
            merge(&mut list, cached);
            return list;
        }
        #[cfg(feature = "webhook")]
        if let Some(url) = url.filter(|u| allowed_url(u)) {
            #[derive(Deserialize)]
            struct Store {
                plugins: Vec<Manifest>,
            }
            match self
                .get(url)
                .await
                .and_then(|b| serde_json::from_slice::<Store>(&b).map_err(|e| e.to_string()))
            {
                Ok(s) => {
                    *self.cache.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((Instant::now(), s.plugins.clone()));
                    merge(&mut list, s.plugins);
                }
                Err(e) => tracing::debug!("store des plugins injoignable ({url}) : {e}"),
            }
        }
        #[cfg(not(feature = "webhook"))]
        let _ = url;
        list
    }

    #[cfg(feature = "webhook")]
    async fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let r = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("téléchargement impossible : {e}"))?;
        if !r.status().is_success() {
            return Err(format!("{url} : réponse {}", r.status()));
        }
        let b = r.bytes().await.map_err(|e| e.to_string())?;
        if b.len() > MAX_BINARY {
            return Err("fichier trop gros".into());
        }
        Ok(b.to_vec())
    }

    /// Lit le manifeste d'un dépôt.
    pub async fn fetch_manifest(&self, source: &str) -> Result<(Manifest, String), String> {
        let url = manifest_url(source)?;
        #[cfg(feature = "webhook")]
        {
            let raw = self.get(&url).await?;
            let m: Manifest = serde_json::from_slice(&raw)
                .map_err(|e| format!("heim-plugin.json illisible : {e}"))?;
            m.validate_external()?;
            Ok((m, url))
        }
        #[cfg(not(feature = "webhook"))]
        Err(format!("téléchargement désactivé dans ce Heim ({url})"))
    }

    fn bin_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id).join("bin")
    }

    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id).join("data")
    }

    fn bin_path(&self, id: &str) -> PathBuf {
        let name = if cfg!(windows) {
            "plugin.exe"
        } else {
            "plugin"
        };
        self.bin_dir(id).join(name)
    }

    /// Télécharge le programme du plugin, vérifie son empreinte et le met en place.
    pub async fn download(&self, m: &Manifest) -> Result<(), String> {
        let bin = m.validate_external()?;
        #[cfg(feature = "webhook")]
        {
            use sha2::Digest;
            let bytes = self.get(&bin.url).await?;
            let got: String = sha2::Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !got.eq_ignore_ascii_case(&bin.sha256) {
                return Err("le programme téléchargé ne correspond pas à son empreinte : rien n'a été installé".into());
            }
            let dir = self.bin_dir(&m.id);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(self.data_dir(&m.id)).map_err(|e| e.to_string())?;
            let tmp = dir.join("plugin.tmp");
            std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            std::fs::rename(&tmp, self.bin_path(&m.id)).map_err(|e| e.to_string())?;
            Ok(())
        }
        #[cfg(not(feature = "webhook"))]
        {
            let _ = bin;
            Err("téléchargement désactivé dans ce Heim".into())
        }
    }

    pub fn remove_files(&self, id: &str) {
        if valid_id(id) {
            let _ = std::fs::remove_dir_all(self.dir.join(id));
        }
    }

    /// Lance (ou relance) un plugin externe. Il est surveillé et relancé s'il s'arrête.
    pub fn start(self: &Arc<Self>, p: &Installed, mqtt: MqttEnv, lang: String) {
        self.stop(&p.id);
        let state = Arc::new(Mutex::new(RunState::default()));
        let me = self.clone();
        let (id, settings) = (p.id.clone(), Value::Object(p.settings.clone()));
        let st = state.clone();
        let handle = tokio::spawn(async move { me.supervise(id, settings, mqtt, lang, st).await });
        self.runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(p.id.clone(), Runtime { handle, state });
    }

    pub fn stop(&self, id: &str) {
        if let Some(r) = self
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
        {
            r.handle.abort();
        }
    }

    pub fn state(&self, id: &str) -> Option<RunState> {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        let r = rt.get(id)?;
        let s = r.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Some(s)
    }

    pub fn log(&self, id: &str) -> Vec<String> {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.get(id)
            .map(|r| {
                r.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .log
                    .iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn supervise(
        self: Arc<Self>,
        id: String,
        settings: Value,
        mqtt: MqttEnv,
        lang: String,
        state: Arc<Mutex<RunState>>,
    ) {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut backoff = Duration::from_secs(2);
        loop {
            let bin = self.bin_path(&id);
            let data = self.data_dir(&id);
            let _ = std::fs::create_dir_all(&data);
            let mut cmd = tokio::process::Command::new(&bin);
            cmd.current_dir(&data)
                .env("HEIM_PLUGIN_ID", &id)
                .env("HEIM_PLUGIN_DATA", &data)
                .env("HEIM_PLUGIN_SETTINGS", settings.to_string())
                .env("HEIM_LANG", &lang)
                .env("HEIM_MQTT_HOST", &mqtt.host)
                .env("HEIM_MQTT_PORT", mqtt.port.to_string())
                .env("HEIM_DISCOVERY_PREFIX", "homeassistant")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            if let Some(u) = &mqtt.username {
                cmd.env("HEIM_MQTT_USERNAME", u);
            }
            if let Some(p) = &mqtt.password {
                cmd.env("HEIM_MQTT_PASSWORD", p);
            }
            let started = Instant::now();
            match cmd.spawn() {
                Ok(mut child) => {
                    {
                        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                        s.running = true;
                        s.pid = child.id();
                        s.started_at = Some(Utc::now());
                    }
                    tracing::info!("plugin {id} : démarré");
                    let push = |st: &Arc<Mutex<RunState>>, line: String| {
                        let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                        if s.log.len() >= LOG_LINES {
                            s.log.pop_front();
                        }
                        s.log.push_back(line.chars().take(500).collect());
                    };
                    let mut out = child.stdout.take().map(|o| BufReader::new(o).lines());
                    let mut err = child.stderr.take().map(|o| BufReader::new(o).lines());
                    let status = loop {
                        tokio::select! {
                            l = async { match out.as_mut() { Some(o) => o.next_line().await, None => std::future::pending().await } } => match l {
                                Ok(Some(l)) => push(&state, l),
                                _ => out = None,
                            },
                            l = async { match err.as_mut() { Some(e) => e.next_line().await, None => std::future::pending().await } } => match l {
                                Ok(Some(l)) => push(&state, l),
                                _ => err = None,
                            },
                            s = child.wait() => break s,
                        }
                    };
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    s.running = false;
                    s.pid = None;
                    s.restarts += 1;
                    s.last_exit = Some(match status {
                        Ok(st) => format!("arrêté ({st})"),
                        Err(e) => e.to_string(),
                    });
                    tracing::warn!(
                        "plugin {id} : {}, relance dans {backoff:?}",
                        s.last_exit.clone().unwrap_or_default()
                    );
                }
                Err(e) => {
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    s.last_exit = Some(format!("impossible de lancer le plugin : {e}"));
                    tracing::warn!("plugin {id} : impossible de lancer {} : {e}", bin.display());
                }
            }
            if started.elapsed() > Duration::from_secs(120) {
                backoff = Duration::from_secs(2);
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(120));
        }
    }
}

/// Le store en ligne complète la copie gravée (un plugin officiel de plus) sans la remplacer.
fn merge(list: &mut Vec<Manifest>, more: Vec<Manifest>) {
    for m in more {
        if m.builtin || BUILTIN.contains(&m.id.as_str()) {
            continue;
        }
        if let Some(i) = list.iter().position(|x| x.id == m.id) {
            list[i] = m;
        } else {
            list.push(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_store_has_the_builtins() {
        let s = embedded_store();
        for id in BUILTIN {
            assert!(s.iter().any(|m| m.id == *id && m.builtin), "{id}");
        }
        assert!(s.iter().all(|m| CATEGORIES.contains(&m.category.as_str())));
    }

    #[test]
    fn sources() {
        assert_eq!(
            manifest_url("alex/heim-wled").unwrap(),
            "https://raw.githubusercontent.com/alex/heim-wled/HEAD/heim-plugin.json"
        );
        assert_eq!(
            manifest_url("https://github.com/alex/heim-wled.git").unwrap(),
            "https://raw.githubusercontent.com/alex/heim-wled/HEAD/heim-plugin.json"
        );
        assert!(manifest_url("https://example.org/p/heim-plugin.json").is_ok());
        assert!(manifest_url("http://example.org/p.json").is_err());
        assert!(manifest_url("../../etc").is_err());
        assert!(manifest_url("a/b/c").is_err());
    }

    fn manifest() -> Manifest {
        let key = arch_key().unwrap_or("x86_64-linux");
        serde_json::from_value(json!({
            "id": "wled", "name": "WLED", "version": "1.0.0", "category": "connections",
            "description": {"fr": "Rubans LED", "en": "LED strips"},
            "binaries": { key: {"url": "https://example.org/wled", "sha256": "a".repeat(64)} },
            "settings": [
                {"key": "host", "type": "string", "label": {"fr": "Adresse"}, "required": true},
                {"key": "port", "type": "number", "default": 80},
                {"key": "token", "type": "password"},
                {"key": "mode", "type": "select", "options": ["a", "b"], "default": "a"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn manifests_are_checked() {
        let m = manifest();
        if arch_key().is_some() {
            assert!(m.validate_external().is_ok());
        }
        assert_eq!(m.description.get("en"), "LED strips");
        assert_eq!(m.description.get("de"), "LED strips");
        let mut bad = m.clone();
        bad.id = "zigbee2mqtt".into();
        assert!(bad.validate_external().is_err());
        let mut bad = m.clone();
        bad.id = "../x".into();
        assert!(bad.validate_external().is_err());
        let mut bad = m;
        for b in bad.binaries.values_mut() {
            b.url = "http://example.org/x".into();
        }
        assert!(bad.validate_external().is_err());
    }

    #[test]
    fn settings_are_cleaned() {
        let m = manifest();
        assert!(clean_settings(&m.settings, &Map::new()).is_err());
        let input = json!({"host": "192.168.1.9", "port": "81", "token": "s3cret"});
        let s = clean_settings(&m.settings, input.as_object().unwrap()).unwrap();
        assert_eq!(s["port"], json!(81.0));
        assert_eq!(s["mode"], json!("a"));
        assert_eq!(public_settings(&m.settings, &s)["token"], json!("••••••"));
        let bad = json!({"host": "x", "mode": "z"});
        assert!(clean_settings(&m.settings, bad.as_object().unwrap()).is_err());
    }
}
