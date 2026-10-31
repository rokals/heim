//! Tuya en local : prises, ampoules et interrupteurs Wi-Fi (LSC Action, Lidl Silvercrest...),
//! sans passer par le cloud une fois la clé locale connue.
//!
//! Il faut pour chaque appareil son identifiant et sa « clé locale » (16 caractères). Le plus
//! simple : `python -m tinytuya wizard` sur un PC (compte développeur Tuya gratuit), puis
//! importer le fichier `devices.json` dans Réglages > Tuya. La catégorie et la liste des points
//! de données (« mapping ») de ce fichier disent à Heim ce qu'est l'appareil.
//!
//! Limites connues :
//! - un appareil Tuya n'accepte souvent qu'UNE connexion locale : couper LocalTuya / Tuya Local
//!   dans Home Assistant pour les appareils repris par Heim ;
//! - les appareils sur pile (sonnettes, capteurs) dorment et ne parlent qu'au cloud.

use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast::error::RecvError, mpsc};

use super::tuya_light::{self as light, Intent, Lamp};
use super::tuya_proto::{self as proto, Session, Version};
use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Attributes, Category, Domain, EntityMeta, Target, slugify};
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "tuya";
pub const SETTINGS_KEY: &str = "tuya";
const PORT: u16 = 6668;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DpInfo {
    pub code: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub values: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuyaDevice {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Clé locale (16 caractères). Jamais renvoyée à l'interface.
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// « 3.3 », « 3.4 », « 3.5 » ; vide : déduite de l'annonce réseau de l'appareil.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mapping: BTreeMap<String, DpInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TuyaSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub devices: Vec<TuyaDevice>,
}

/// Lit un `devices.json` de tinytuya (ou un `snapshot.json`).
pub fn import_tinytuya(text: &str) -> Result<Vec<TuyaDevice>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("fichier illisible : {e}"))?;
    let list = v
        .as_array()
        .or_else(|| v.get("devices").and_then(Value::as_array))
        .ok_or("ce fichier ne contient pas de liste d'appareils")?;
    let mut out = Vec::new();
    for d in list {
        let (Some(id), Some(key)) = (
            d.get("id").and_then(Value::as_str),
            d.get("key")
                .or_else(|| d.get("local_key"))
                .and_then(Value::as_str),
        ) else {
            continue;
        };
        // Les sous-appareils d'une passerelle Zigbee Tuya n'ont pas de connexion propre.
        if d.get("sub").and_then(Value::as_bool) == Some(true) || key.len() != 16 {
            continue;
        }
        let str_of = |k: &str| {
            d.get(k)
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };
        let mapping = d
            .get("mapping")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(dp, info)| {
                        Some((
                            dp.clone(),
                            serde_json::from_value::<DpInfo>(info.clone()).ok()?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let version = d
            .get("version")
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_f64().map(|f| format!("{f:.1}")))
            })
            .filter(|v| Version::parse(v).is_some());
        out.push(TuyaDevice {
            id: id.to_string(),
            name: str_of("name").unwrap_or_else(|| id.to_string()),
            key: key.to_string(),
            ip: str_of("ip").filter(|ip| ip.parse::<Ipv4Addr>().is_ok()),
            version,
            category: str_of("category"),
            product_name: str_of("product_name"),
            mapping,
        });
    }
    if out.is_empty() {
        return Err("aucun appareil utilisable dans ce fichier (il faut « id » et « key »)".into());
    }
    Ok(out)
}

// ---- Ce que Heim fait d'un appareil -----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    Light(Lamp),
    Switch {
        dp: String,
    },
    /// Mesure : point de données, échelle (10^-scale), unité, type de mesure.
    Sensor {
        dp: String,
        scale: i32,
        unit: &'static str,
        kind: &'static str,
        diagnostic: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Mapped {
    pub meta: EntityMeta,
    pub role: Role,
}

fn range(info: &DpInfo, default: (f64, f64)) -> (f64, f64) {
    let v = if info.values.is_string() {
        serde_json::from_str(info.values.as_str().unwrap_or("{}")).unwrap_or_default()
    } else {
        info.values.clone()
    };
    (
        v.get("min").and_then(Value::as_f64).unwrap_or(default.0),
        v.get("max").and_then(Value::as_f64).unwrap_or(default.1),
    )
}

fn scale(info: &DpInfo) -> i32 {
    let v = if info.values.is_string() {
        serde_json::from_str(info.values.as_str().unwrap_or("{}")).unwrap_or_default()
    } else {
        info.values.clone()
    };
    v.get("scale").and_then(Value::as_i64).unwrap_or(0) as i32
}

/// Mapping par défaut quand le fichier n'en donne pas : on devine d'après la catégorie.
fn default_mapping(category: &str) -> BTreeMap<String, DpInfo> {
    let dp = |code: &str, values: Value| DpInfo {
        code: code.into(),
        kind: None,
        values,
    };
    let mut m = BTreeMap::new();
    match category {
        "dj" | "dd" | "fwd" | "dc" | "xdd" | "tgq" | "tgkg" => {
            m.insert("20".into(), dp("switch_led", Value::Null));
            m.insert("21".into(), dp("work_mode", Value::Null));
            m.insert(
                "22".into(),
                dp("bright_value_v2", json!({"min": 10, "max": 1000})),
            );
            m.insert(
                "23".into(),
                dp("temp_value_v2", json!({"min": 0, "max": 1000})),
            );
            m.insert(
                "24".into(),
                dp(
                    "colour_data_v2",
                    json!({"h": {"min": 0, "max": 360}, "s": {"min": 0, "max": 1000}, "v": {"min": 0, "max": 1000}}),
                ),
            );
        }
        // Prises, multiprises, interrupteurs (et appareil ajouté à la main sans catégorie).
        "cz" | "pc" | "kg" | "tdq" | "" => {
            m.insert("1".into(), dp("switch_1", Value::Null));
            if matches!(category, "cz" | "pc") {
                m.insert("18".into(), dp("cur_current", json!({"scale": 0})));
                m.insert("19".into(), dp("cur_power", json!({"scale": 1})));
                m.insert("20".into(), dp("cur_voltage", json!({"scale": 1})));
            }
        }
        // Caméras, sonnettes, capteurs sur pile... : on ne devine pas.
        _ => {}
    }
    m
}

/// Les entités Heim d'un appareil Tuya.
pub fn map_device(d: &TuyaDevice) -> Vec<Mapped> {
    let mapping = if d.mapping.is_empty() {
        default_mapping(d.category.as_deref().unwrap_or(""))
    } else {
        d.mapping.clone()
    };
    let by_code: HashMap<&str, (&String, &DpInfo)> = mapping
        .iter()
        .map(|(dp, i)| (i.code.as_str(), (dp, i)))
        .collect();
    let base = slugify(if d.name.is_empty() { &d.id } else { &d.name });
    let mut caps = Attributes::new();
    caps.insert("device_name".into(), json!(d.name));
    caps.insert("manufacturer".into(), json!("Tuya"));
    if let Some(p) = &d.product_name {
        caps.insert("model".into(), json!(p));
    }
    let meta =
        |domain: Domain, suffix: &str, name: String, extra: Attributes, category: Category| {
            let mut capabilities = caps.clone();
            capabilities.extend(extra);
            EntityMeta {
                entity_id: if suffix.is_empty() {
                    format!("{}.{base}", domain.prefix())
                } else {
                    format!("{}.{base}_{suffix}", domain.prefix())
                },
                domain,
                name,
                integration: INTEGRATION.into(),
                device_id: Some(format!("tuya:{}", d.id)),
                room: None,
                plan: None,
                capabilities,
                user_edited: false,
                category,
                hidden: false,
            }
        };
    let mut out = Vec::new();

    if let Some((sw, _)) = by_code.get("switch_led") {
        let bright = ["bright_value_v2", "bright_value"].iter().find_map(|c| {
            by_code.get(c).map(|(dp, i)| {
                let (min, max) = range(
                    i,
                    if *c == "bright_value" {
                        (25.0, 255.0)
                    } else {
                        (10.0, 1000.0)
                    },
                );
                ((*dp).clone(), min, max)
            })
        });
        let temp = ["temp_value_v2", "temp_value"].iter().find_map(|c| {
            by_code.get(c).map(|(dp, i)| {
                let (min, max) = range(
                    i,
                    if *c == "temp_value" {
                        (0.0, 255.0)
                    } else {
                        (0.0, 1000.0)
                    },
                );
                ((*dp).clone(), min, max)
            })
        });
        let colour = ["colour_data_v2", "colour_data"].iter().find_map(|c| {
            by_code
                .get(c)
                .map(|(dp, i)| ((*dp).clone(), light::format_of(c, &dp_values(i))))
        });
        let mut extra = Attributes::new();
        if bright.is_some() || colour.is_some() {
            extra.insert("brightness".into(), json!(true));
        }
        if temp.is_some() {
            extra.insert("color_temp".into(), json!([153, 500]));
        }
        if colour.is_some() {
            extra.insert("hs_color".into(), json!(true));
        }
        out.push(Mapped {
            meta: meta(Domain::Light, "", d.name.clone(), extra, Category::Primary),
            role: Role::Light(Lamp {
                switch: (*sw).clone(),
                bright,
                temp,
                mode: by_code.get("work_mode").map(|(dp, _)| (*dp).clone()),
                colour,
            }),
        });
    }

    let mut switches: Vec<(&String, &DpInfo)> = mapping
        .iter()
        .filter(|(_, i)| {
            i.code == "switch"
                || i.code == "switch_on"
                || (i.code.starts_with("switch_") && i.code[7..].parse::<u8>().is_ok())
                || i.code.starts_with("switch_usb")
        })
        .collect();
    switches.sort_by_key(|(dp, _)| dp.parse::<u32>().unwrap_or(999));
    let many = switches.len() > 1;
    for (i, (dp, info)) in switches.iter().enumerate() {
        let (suffix, name) = if !many {
            (String::new(), d.name.clone())
        } else if info.code.starts_with("switch_usb") {
            (info.code.replace("switch_", ""), format!("{} USB", d.name))
        } else {
            (format!("l{}", i + 1), format!("{} {}", d.name, i + 1))
        };
        out.push(Mapped {
            meta: meta(
                Domain::Switch,
                &suffix,
                name,
                Attributes::new(),
                Category::Primary,
            ),
            role: Role::Switch { dp: (*dp).clone() },
        });
    }

    for (code, unit, kind, label, diagnostic, default_scale) in [
        ("cur_power", "W", "power", "puissance", false, 1),
        ("cur_voltage", "V", "voltage", "tension", true, 1),
        ("cur_current", "mA", "current", "courant", true, 0),
        ("add_ele", "kWh", "energy", "énergie", true, 3),
    ] {
        if let Some((dp, info)) = by_code.get(code) {
            let mut extra = Attributes::new();
            extra.insert("unit".into(), json!(unit));
            extra.insert("kind".into(), json!(kind));
            let s = if info.values.is_null() {
                default_scale
            } else {
                scale(info)
            };
            out.push(Mapped {
                meta: meta(
                    Domain::Sensor,
                    kind,
                    format!("{} {label}", d.name),
                    extra,
                    if diagnostic {
                        Category::Diagnostic
                    } else {
                        Category::Primary
                    },
                ),
                role: Role::Sensor {
                    dp: (*dp).clone(),
                    scale: s,
                    unit,
                    kind,
                    diagnostic,
                },
            });
        }
    }
    out
}

/// L'état Heim d'une entité, d'après les points de données reçus.
pub fn state_of(role: &Role, dps: &Map<String, Value>) -> Option<(Value, Attributes)> {
    match role {
        Role::Switch { dp } => dps
            .get(dp)
            .and_then(Value::as_bool)
            .map(|on| (json!(if on { "on" } else { "off" }), Attributes::new())),
        Role::Light(lamp) => {
            let on = dps.get(&lamp.switch).and_then(Value::as_bool);
            let mut attrs = Attributes::new();
            if let Some(b) = lamp.brightness_now(dps) {
                attrs.insert("brightness".into(), json!(b));
            }
            if let Some(m) = lamp.values(dps).get("color_mode") {
                attrs.insert("color_mode".into(), m.clone());
            }
            if let Some((dp, min, max)) = &lamp.temp
                && let Some(v) = dps.get(dp).and_then(Value::as_f64)
            {
                // Tuya : min = le plus chaud. Heim : mireds (153 froid, 500 chaud).
                let ratio = ((v - min) / (max - min)).clamp(0.0, 1.0);
                attrs.insert(
                    "color_temp".into(),
                    json!((500.0 - ratio * 347.0).round() as u16),
                );
            }
            match on {
                Some(on) => Some((json!(if on { "on" } else { "off" }), attrs)),
                None if !attrs.is_empty() => Some((Value::Null, attrs)),
                None => None,
            }
        }
        Role::Sensor { dp, scale, .. } => dps.get(dp).and_then(Value::as_f64).map(|v| {
            let x = v / 10f64.powi(*scale);
            (json!((x * 100.0).round() / 100.0), Attributes::new())
        }),
    }
}

/// Les points de données à envoyer pour une commande Heim. `last` : le dernier état reçu
/// (une lampe en couleur règle sa luminosité dans la couleur).
pub fn dps_for(
    role: &Role,
    target: &Target,
    last: &Map<String, Value>,
    seen: Option<light::Format>,
) -> Option<Map<String, Value>> {
    match role {
        Role::Switch { dp } => {
            let mut dps = Map::new();
            dps.insert(dp.clone(), json!(target.on));
            Some(dps)
        }
        Role::Light(lamp) => {
            let intent = Intent {
                on: Some(target.on),
                brightness: target.brightness,
                color_temp: target.color_temp,
                ..Default::default()
            };
            Some(lamp.dps(&intent, last, seen))
        }
        Role::Sensor { .. } => None,
    }
}

// ---- Découverte : les appareils s'annoncent toutes les 5 s en UDP ------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Announce {
    pub id: String,
    pub ip: String,
    pub version: String,
    pub product_key: Option<String>,
}

pub fn parse_announce(v: &Value) -> Option<Announce> {
    Some(Announce {
        id: v
            .get("gwId")
            .or_else(|| v.get("devId"))
            .and_then(Value::as_str)?
            .to_string(),
        ip: v.get("ip").and_then(Value::as_str)?.to_string(),
        version: v
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("3.3")
            .to_string(),
        product_key: v
            .get("productKey")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

async fn listen_udp(port: u16, tx: mpsc::Sender<Announce>) {
    let socket = match tokio::net::UdpSocket::bind(("0.0.0.0", port)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "tuya : écoute UDP {port} impossible ({e}) — indiquez l'adresse IP des appareils à la main"
            );
            return;
        }
    };
    let mut buf = vec![0u8; 2048];
    loop {
        let Ok((n, _)) = socket.recv_from(&mut buf).await else {
            continue;
        };
        if let Some(a) = proto::decode_broadcast(&buf[..n])
            .as_ref()
            .and_then(parse_announce)
            && tx.send(a).await.is_err()
        {
            return;
        }
    }
}

/// L'adresse IPv4 de la box sur le réseau local (celle de la route par défaut).
fn local_ipv4() -> Option<Ipv4Addr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // Aucun paquet n'est envoyé : on lit juste l'adresse choisie.
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Port 7000 : demande d'annonce diffusée (appareils 3.5, qui ne s'annoncent pas d'eux-mêmes),
/// répétée toutes les 30 s pendant 5 min, puis toutes les 5 min ; les réponses arrivent ici.
async fn listen_app(tx: mpsc::Sender<Announce>) {
    let socket = match tokio::net::UdpSocket::bind(("0.0.0.0", 7000)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("tuya : écoute UDP 7000 impossible ({e})");
            return;
        }
    };
    let _ = socket.set_broadcast(true);
    let started = tokio::time::Instant::now();
    let mut buf = vec![0u8; 2048];
    loop {
        if let Some(ip) = local_ipv4() {
            let req = proto::discovery_request(&ip.to_string());
            let _ = socket.send_to(&req, ("255.255.255.255", 7000)).await;
        }
        let every = if started.elapsed() < Duration::from_secs(300) {
            30
        } else {
            300
        };
        let until = tokio::time::Instant::now() + Duration::from_secs(every);
        while let Ok(Ok((n, _))) = tokio::time::timeout_at(until, socket.recv_from(&mut buf)).await
        {
            if let Some(a) = proto::decode_broadcast(&buf[..n])
                .as_ref()
                .and_then(parse_announce)
                && tx.send(a).await.is_err()
            {
                return;
            }
        }
    }
}

/// Cet appareil répond-il à cette adresse, avec sa clé ? Renvoie la version qui marche.
/// 3.4 et 3.5 : la négociation de session ne réussit qu'avec la bonne clé ; 3.3 : seule la
/// bonne clé déchiffre la réponse à la demande d'état.
async fn identify(ip: &str, d: &TuyaDevice) -> Option<Version> {
    for version in [Version::V35, Version::V34, Version::V33] {
        let attempt = async {
            let mut c = Conn::open(ip, d, version).await?;
            if version == Version::V33 {
                let q = c.session.query(&[]);
                c.send(&q).await?;
                let f = c.next_frame().await?;
                f.json().ok_or_else(|| "réponse illisible".to_string())?;
            }
            Ok::<_, String>(())
        };
        if let Ok(Ok(())) = tokio::time::timeout(Duration::from_secs(4), attempt).await {
            return Some(version);
        }
    }
    None
}

/// Dernier recours quand un appareil ne s'annonce pas : on cherche sur le réseau local (/24)
/// les machines qui écoutent sur le port Tuya (6668), et on essaie la clé de chaque appareil
/// sans adresse. Rien n'est envoyé aux adresses déjà attribuées.
pub async fn locate(
    pending: Vec<TuyaDevice>,
    taken: std::collections::HashSet<String>,
) -> Vec<Announce> {
    let Some(me) = local_ipv4() else {
        return Vec::new();
    };
    let [a, b, c, _] = me.octets();
    let mut probes = tokio::task::JoinSet::new();
    for host in 1..=254u8 {
        let ip = Ipv4Addr::new(a, b, c, host);
        if ip == me || taken.contains(&ip.to_string()) {
            continue;
        }
        probes.spawn(async move {
            let open =
                tokio::time::timeout(Duration::from_millis(800), TcpStream::connect((ip, PORT)))
                    .await
                    .is_ok_and(|r| r.is_ok());
            open.then_some(ip.to_string())
        });
    }
    let mut hosts = Vec::new();
    while let Some(r) = probes.join_next().await {
        if let Ok(Some(ip)) = r {
            hosts.push(ip);
        }
    }
    hosts.sort();
    tracing::info!(
        "tuya : {} appareil(s) Tuya sur le réseau, recherche de {} clé(s)",
        hosts.len(),
        pending.len()
    );
    let mut found = Vec::new();
    let mut left = pending;
    for ip in hosts {
        let mut hit = None;
        for (i, d) in left.iter().enumerate() {
            if let Some(v) = identify(&ip, d).await {
                hit = Some((i, v));
                break;
            }
        }
        if let Some((i, v)) = hit {
            let d = left.remove(i);
            tracing::info!("tuya : {} trouvé en {ip} (v{})", d.name, v.as_str());
            found.push(Announce {
                id: d.id,
                ip,
                version: v.as_str().into(),
                product_key: None,
            });
        }
        if left.is_empty() {
            break;
        }
    }
    found
}

/// Écoute les annonces pendant quelques secondes (bouton « Chercher » de l'interface).
pub async fn scan(seconds: u64) -> Vec<Announce> {
    let (tx, mut rx) = mpsc::channel(64);
    let a = tokio::spawn(listen_udp(6666, tx.clone()));
    let c = tokio::spawn(listen_app(tx.clone()));
    let b = tokio::spawn(listen_udp(6667, tx));
    let mut found: BTreeMap<String, Announce> = BTreeMap::new();
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            a = rx.recv() => match a { Some(a) => { found.insert(a.id.clone(), a); } None => break },
        }
    }
    a.abort();
    b.abort();
    c.abort();
    found.into_values().collect()
}

// ---- Réglages complets (comme pour Zigbee) ---------------------------------------------

/// Mesures et états en lecture seule : pas de réglage à proposer.
const READ_ONLY: &[&str] = &[
    "cur_power",
    "cur_voltage",
    "cur_current",
    "add_ele",
    "fault",
    "countdown_left",
    "va_temperature",
    "va_humidity",
    "battery_percentage",
    "battery_state",
    "doorcontact_state",
];

/// Les commandes principales (allumer, luminosité...) restent sur la carte de l'appareil.
const MAIN: &[&str] = &[
    "switch_led",
    "bright_value",
    "bright_value_v2",
    "temp_value",
    "temp_value_v2",
    "work_mode",
    "colour_data",
    "colour_data_v2",
];

fn dp_values(info: &DpInfo) -> Value {
    if info.values.is_string() {
        serde_json::from_str(info.values.as_str().unwrap_or("{}")).unwrap_or_default()
    } else {
        info.values.clone()
    }
}

pub fn device_controls(d: &TuyaDevice) -> Vec<Value> {
    let mapping = if d.mapping.is_empty() {
        default_mapping(d.category.as_deref().unwrap_or(""))
    } else {
        d.mapping.clone()
    };
    let mut out = Vec::new();
    // Une lampe : les réglages du panneau de lampe (luminosité, blanc, couleur, modes), sous
    // les mêmes noms que Zigbee, pour que la fiche et la tuile les montrent pareil.
    if let Some(Role::Light(lamp)) = map_device(d)
        .into_iter()
        .map(|m| m.role)
        .find(|r| matches!(r, Role::Light(_)))
    {
        out.push(json!({"key": "state", "name": "state", "kind": "binary", "label": "État", "group": "light",
                        "value_on": "ON", "value_off": "OFF", "value_toggle": "TOGGLE"}));
        if lamp.bright.is_some() || lamp.colour.is_some() {
            out.push(json!({"key": "brightness", "name": "brightness", "kind": "numeric", "label": "Luminosité",
                            "group": "light", "min": 1, "max": 254}));
        }
        if lamp.temp.is_some() {
            out.push(json!({"key": "color_temp", "name": "color_temp", "kind": "numeric", "label": "Blanc",
                            "group": "light", "min": 153, "max": 500, "unit": "mired"}));
        }
        if lamp.colour.is_some() {
            out.push(
                json!({"key": "color", "name": "color_hs", "kind": "color", "label": "Couleur",
                            "group": "light", "format": "hs"}),
            );
        }
        if let Some(info) = lamp.mode.as_ref().and_then(|m| mapping.get(m)) {
            let effects: Vec<Value> = dp_values(info)
                .get("range")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|v| !matches!(v.as_str(), Some("white" | "colour")))
                .cloned()
                .collect();
            if !effects.is_empty() {
                out.push(
                    json!({"key": "effect", "name": "effect", "kind": "enum", "label": "Mode",
                                "group": "light", "values": effects}),
                );
            }
        }
    }
    let mut dps: Vec<(&String, &DpInfo)> = mapping.iter().collect();
    dps.sort_by_key(|(dp, _)| dp.parse::<u32>().unwrap_or(999));
    for (dp, info) in dps {
        let code = info.code.as_str();
        if READ_ONLY.contains(&code) || MAIN.contains(&code) {
            continue;
        }
        let values = dp_values(info);
        let kind = info
            .kind
            .as_deref()
            .unwrap_or(if code.starts_with("switch") {
                "Boolean"
            } else {
                ""
            });
        let group = (code.starts_with("switch")).then_some("switch");
        let c = match kind {
            "Boolean" | "bool" => {
                json!({"key": dp, "name": code, "kind": "binary", "label": code, "value_on": true, "value_off": false, "group": group})
            }
            "Enum" | "enum" => {
                let range = values.get("range").cloned().unwrap_or(json!([]));
                json!({"key": dp, "name": code, "kind": "enum", "label": code, "values": range, "category": "config"})
            }
            "Integer" | "value" => {
                let sc =
                    10f64.powi(values.get("scale").and_then(Value::as_i64).unwrap_or(0) as i32);
                let num = |k: &str| values.get(k).and_then(Value::as_f64).map(|v| json!(v / sc));
                json!({"key": dp, "name": code, "kind": "numeric", "label": code, "min": num("min"), "max": num("max"),
                       "step": num("step"), "unit": values.get("unit"), "scale": sc, "category": "config"})
            }
            _ => continue,
        };
        out.push(c);
    }
    out
}

/// Échelle de chaque point de données entier (« scale » 1 : 853 -> 85.3).
pub fn scales(d: &TuyaDevice) -> HashMap<String, f64> {
    let mapping = if d.mapping.is_empty() {
        default_mapping(d.category.as_deref().unwrap_or(""))
    } else {
        d.mapping.clone()
    };
    mapping
        .iter()
        .filter_map(|(dp, info)| {
            let s = dp_values(info).get("scale").and_then(Value::as_i64)?;
            (s != 0).then(|| (dp.clone(), 10f64.powi(s as i32)))
        })
        .collect()
}

fn scaled_for_display(scales: &HashMap<String, f64>, dp: &str, v: &Value) -> Value {
    match (v.as_f64(), scales.get(dp)) {
        (Some(n), Some(sc)) => json!((n / sc * 1000.0).round() / 1000.0),
        _ => v.clone(),
    }
}

// ---- Connexion à un appareil ----------------------------------------------------------

enum DeviceMsg {
    Command(String, Target),
    Located(String, String),
    Set(Map<String, Value>),
}

struct Conn {
    stream: TcpStream,
    session: Session,
    buf: Vec<u8>,
}

impl Conn {
    async fn open(ip: &str, d: &TuyaDevice, version: Version) -> Result<Self, String> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect((ip, PORT)))
            .await
            .map_err(|_| format!("{ip} ne répond pas"))?
            .map_err(|e| {
                format!("{ip} refuse la connexion ({e}) — déjà connecté à une autre application ?")
            })?;
        let _ = stream.set_nodelay(true);
        let mut conn = Conn {
            stream,
            session: Session::new(&d.id, &d.key, version)?,
            buf: Vec::new(),
        };
        if version.negotiates() {
            let start = conn.session.negotiation_start();
            conn.send(&start).await?;
            let resp = tokio::time::timeout(Duration::from_secs(5), conn.next_frame())
                .await
                .map_err(|_| {
                    "pas de réponse à la négociation (version ou clé incorrecte ?)".to_string()
                })??;
            let finish = conn.session.negotiation_finish(&resp)?;
            conn.send(&finish).await?;
        }
        Ok(conn)
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.stream
            .write_all(bytes)
            .await
            .map_err(|e| format!("envoi impossible : {e}"))
    }

    async fn next_frame(&mut self) -> Result<proto::Frame, String> {
        loop {
            match self.session.decode(&self.buf) {
                Ok(Some((frame, used))) => {
                    self.buf.drain(..used);
                    return Ok(frame);
                }
                Ok(None) => {}
                Err(e) => {
                    // Octets illisibles : on se recale au prochain préfixe.
                    let skip = self.buf[1..]
                        .windows(4)
                        .position(|w| {
                            w == proto::PREFIX_55AA.to_be_bytes()
                                || w == proto::PREFIX_6699.to_be_bytes()
                        })
                        .map(|p| p + 1)
                        .unwrap_or(self.buf.len());
                    tracing::debug!("tuya {} : {e}", self.session.dev_id);
                    self.buf.drain(..skip);
                    if e.contains("clé") {
                        return Err(e);
                    }
                    continue;
                }
            }
            let mut chunk = [0u8; 2048];
            let n = self
                .stream
                .read(&mut chunk)
                .await
                .map_err(|e| format!("connexion coupée : {e}"))?;
            if n == 0 {
                return Err("l'appareil a fermé la connexion".into());
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

struct DeviceTask {
    dev: TuyaDevice,
    mapped: Vec<Mapped>,
    registry: RegistryHandle,
    ip: Option<String>,
    version: Version,
    controls: crate::devices::Controls,
    control_list: Vec<Value>,
    scales: HashMap<String, f64>,
    /// Le dernier état de chaque point de données reçu.
    last: Map<String, Value>,
    /// Le format de couleur que l'appareil envoie lui-même : on lui répond pareil.
    colour_fmt: Option<light::Format>,
}

impl DeviceTask {
    fn lamp(&self) -> Option<&Lamp> {
        self.mapped.iter().find_map(|m| match &m.role {
            Role::Light(l) => Some(l),
            _ => None,
        })
    }

    /// Les réglages de la fiche → points de données : ceux de la lampe (couleur, luminosité,
    /// blanc, mode) convertis, les autres tels quels (échelle remise).
    fn set_dps(&self, values: &Map<String, Value>) -> Map<String, Value> {
        let mut rest = values.clone();
        let intent = Intent::take_from(&mut rest);
        let mut out = self.raw_dps(&rest);
        if let (Some(i), Some(lamp)) = (intent, self.lamp()) {
            out.extend(lamp.dps(&i, &self.last, self.colour_fmt));
        }
        out
    }

    /// Réglage demandé dans l'interface -> points de données bruts (échelle remise).
    fn raw_dps(&self, values: &Map<String, Value>) -> Map<String, Value> {
        values
            .iter()
            .map(|(dp, v)| {
                let sc = self
                    .control_list
                    .iter()
                    .find(|c| c.get("key").and_then(Value::as_str) == Some(dp.as_str()))
                    .and_then(|c| c.get("scale"))
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                let raw = match v.as_f64() {
                    Some(n) if sc != 1.0 || v.is_f64() => json!((n * sc).round() as i64),
                    _ => v.clone(),
                };
                (dp.clone(), raw)
            })
            .collect()
    }

    async fn report(&mut self, dps: &Map<String, Value>) {
        for (k, v) in dps {
            self.last.insert(k.clone(), v.clone());
        }
        let seen = self
            .lamp()
            .and_then(|l| l.colour.as_ref())
            .and_then(|(dp, _)| dps.get(dp))
            .and_then(light::decode)
            .map(|(_, f)| f);
        if seen.is_some() && seen != self.colour_fmt {
            tracing::info!("tuya : {} : couleur au format {:?}", self.dev.name, seen);
            self.colour_fmt = seen;
        }
        let lamp_values = self.lamp().map(|l| l.values(&self.last));
        if let Some(d) =
            crate::devices::lock(&self.controls).get_mut(&format!("tuya:{}", self.dev.id))
        {
            for (k, v) in dps {
                d.values
                    .insert(k.clone(), scaled_for_display(&self.scales, k, v));
            }
            if let Some(lv) = lamp_values {
                d.values.extend(lv);
            }
        }
        for m in &self.mapped {
            // Une lampe se lit sur l'état complet (la luminosité dépend du mode).
            let view = match &m.role {
                Role::Light(_) => &self.last,
                _ => dps,
            };
            if let Some((value, attrs)) = state_of(&m.role, view) {
                if value.is_null() {
                    self.registry.seen(m.meta.entity_id.clone(), attrs).await;
                } else {
                    self.registry
                        .report(m.meta.entity_id.clone(), value, attrs, true)
                        .await;
                }
            }
        }
    }

    async fn set_available(&self, available: bool) {
        for m in &self.mapped {
            self.registry
                .set_available(m.meta.entity_id.clone(), available)
                .await;
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<DeviceMsg>) {
        for m in &self.mapped {
            self.registry.upsert(m.meta.clone()).await;
        }
        let known: Vec<String> = self
            .mapped
            .iter()
            .map(|m| match &m.role {
                Role::Switch { dp } | Role::Sensor { dp, .. } => dp.clone(),
                Role::Light(l) => l.switch.clone(),
            })
            .collect();
        let mut backoff = Duration::from_secs(2);
        let mut warned = false;
        loop {
            let Some(ip) = self.ip.clone() else {
                // Pas d'adresse : on attend l'annonce réseau de l'appareil.
                match rx.recv().await {
                    Some(DeviceMsg::Located(ip, v)) => {
                        self.ip = Some(ip);
                        if self.dev.version.is_none() {
                            self.version = Version::parse(&v).unwrap_or(Version::V33);
                        }
                    }
                    Some(DeviceMsg::Command(..) | DeviceMsg::Set(_)) => {}
                    None => return,
                }
                continue;
            };
            match Conn::open(&ip, &self.dev, self.version).await {
                Ok(mut conn) => {
                    backoff = Duration::from_secs(2);
                    warned = false;
                    tracing::info!(
                        "tuya : {} connecté ({ip}, {})",
                        self.dev.name,
                        self.version.as_str()
                    );
                    self.set_available(true).await;
                    let q = conn.session.query(&known);
                    let result = match conn.send(&q).await {
                        Ok(()) => self.serve(&mut conn, &mut rx).await,
                        Err(e) => Err(e),
                    };
                    match result {
                        Ok(()) => return,
                        Err(e) => tracing::info!("tuya : {} déconnecté ({e})", self.dev.name),
                    }
                }
                Err(e) => {
                    if !warned {
                        tracing::warn!("tuya : {} : {e}", self.dev.name);
                        warned = true;
                    }
                    self.set_available(false).await;
                }
            }
            // Attente avant de réessayer, en restant à l'écoute (nouvelle adresse...).
            let sleep = tokio::time::sleep(backoff);
            tokio::pin!(sleep);
            loop {
                tokio::select! {
                    _ = &mut sleep => break,
                    msg = rx.recv() => match msg {
                        Some(DeviceMsg::Located(ip, _)) if Some(&ip) != self.ip.as_ref() => { self.ip = Some(ip); break; }
                        Some(_) => {}
                        None => return,
                    },
                }
            }
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }

    /// Tant que la connexion tient : états reçus, commandes envoyées, battement de cœur.
    async fn serve(
        &mut self,
        conn: &mut Conn,
        rx: &mut mpsc::Receiver<DeviceMsg>,
    ) -> Result<(), String> {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.tick().await;
        let mut last_rx = tokio::time::Instant::now();
        loop {
            tokio::select! {
                frame = conn.next_frame() => {
                    let frame = frame?;
                    last_rx = tokio::time::Instant::now();
                    if let Some(dps) = frame.dps() {
                        self.report(&dps).await;
                    } else if frame.payload.windows(12).any(|w| w == b"data unvalid") && !conn.session.device22 {
                        tracing::info!("tuya : {} : mode « device22 »", self.dev.name);
                        conn.session.device22 = true;
                        let known: Vec<String> = self.mapped.iter().map(|m| match &m.role {
                            Role::Switch { dp } | Role::Sensor { dp, .. } => dp.clone(),
                            Role::Light(l) => l.switch.clone(),
                        }).collect();
                        let q = conn.session.query(&known);
                        conn.send(&q).await?;
                    }
                }
                _ = heartbeat.tick() => {
                    if last_rx.elapsed() > Duration::from_secs(35) {
                        return Err("plus de nouvelles depuis 35 s".into());
                    }
                    let hb = conn.session.heartbeat();
                    conn.send(&hb).await?;
                }
                msg = rx.recv() => match msg {
                    Some(DeviceMsg::Command(entity_id, target)) => {
                        let role = self.mapped.iter().find(|m| m.meta.entity_id == entity_id).map(|m| m.role.clone());
                        if let Some(dps) = role.as_ref().and_then(|r| dps_for(r, &target, &self.last, self.colour_fmt)) {
                            let c = conn.session.control(&dps);
                            conn.send(&c).await?;
                        }
                    }
                    Some(DeviceMsg::Set(values)) => {
                        let dps = self.set_dps(&values);
                        tracing::info!("tuya : {} ← {}", self.dev.name, serde_json::to_string(&dps).unwrap_or_default());
                        // Ce qui est parti, visible dans Infos (pour comprendre une lampe qui ne suit pas).
                        if let Some(d) = crate::devices::lock(&self.controls).get_mut(&format!("tuya:{}", self.dev.id)) {
                            d.values.insert("last_sent".into(), serde_json::json!(dps));
                        }
                        let c = conn.session.control(&dps);
                        conn.send(&c).await?;
                    }
                    Some(DeviceMsg::Located(ip, _)) => {
                        if Some(&ip) != self.ip.as_ref() {
                            self.ip = Some(ip);
                            return Err("nouvelle adresse".into());
                        }
                    }
                    None => return Ok(()),
                },
            }
        }
    }
}

// ---- L'intégration ------------------------------------------------------------------------

/// Les appareils entendus sur le réseau (id -> annonce), partagés avec la page Réglages.
pub type Seen = std::sync::Arc<std::sync::Mutex<BTreeMap<String, Announce>>>;

pub async fn run(
    settings: TuyaSettings,
    registry: RegistryHandle,
    bus: Bus,
    seen: Seen,
    controls: crate::devices::Controls,
) -> anyhow::Result<()> {
    let mut bus_rx = bus.subscribe();
    bus.integration_status(INTEGRATION, IntegrationState::Starting, None);
    let mut tasks: HashMap<String, mpsc::Sender<DeviceMsg>> = HashMap::new();
    let mut by_entity: HashMap<String, String> = HashMap::new();
    let mut joins = Vec::new();
    for d in &settings.devices {
        let mapped = map_device(d);
        if mapped.is_empty() {
            tracing::warn!(
                "tuya : {} ({}) : type d'appareil non pris en charge",
                d.name,
                d.category.as_deref().unwrap_or("?")
            );
            continue;
        }
        for m in &mapped {
            by_entity.insert(m.meta.entity_id.clone(), d.id.clone());
        }
        let (tx, rx) = mpsc::channel(16);
        tasks.insert(d.id.clone(), tx);
        let control_list = device_controls(d);
        {
            let mut info = Map::new();
            info.insert("vendor".into(), json!("Tuya"));
            if let Some(p) = &d.product_name {
                info.insert("model".into(), json!(p));
            }
            if let Some(v) = &d.version {
                info.insert("protocol".into(), json!(v));
            }
            if let Some(ip) = &d.ip {
                info.insert("ip".into(), json!(ip));
            }
            crate::devices::lock(&controls).insert(
                format!("tuya:{}", d.id),
                crate::devices::DeviceControls {
                    integration: INTEGRATION.into(),
                    device_id: format!("tuya:{}", d.id),
                    name: d.name.clone(),
                    controls: control_list.clone(),
                    values: Map::new(),
                    info,
                },
            );
        }
        let task = DeviceTask {
            controls: controls.clone(),
            scales: scales(d),
            control_list,
            dev: d.clone(),
            mapped,
            registry: registry.clone(),
            ip: d.ip.clone(),
            version: d
                .version
                .as_deref()
                .and_then(Version::parse)
                .unwrap_or(Version::V33),
            last: Map::new(),
            colour_fmt: None,
        };
        joins.push(tokio::spawn(task.run(rx)));
    }
    let (atx, mut arx) = mpsc::channel(64);
    joins.push(tokio::spawn(listen_udp(6666, atx.clone())));
    joins.push(tokio::spawn(listen_udp(6667, atx.clone())));
    joins.push(tokio::spawn(listen_app(atx.clone())));
    // Les appareils dont on connaît l'adresse (réglée, ou entendue sur le réseau).
    let mut located: HashMap<String, String> = settings
        .devices
        .iter()
        .filter_map(|d| Some((d.id.clone(), d.ip.clone()?)))
        .collect();
    // Recherche active des appareils muets : 20 s après le démarrage, puis toutes les 10 min.
    let mut sweep = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(20),
        Duration::from_secs(600),
    );
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut sweeping: Option<tokio::task::JoinHandle<()>> = None;
    bus.integration_status(
        INTEGRATION,
        IntegrationState::Connected,
        Some(format!("{} appareil(s)", tasks.len())),
    );
    struct Abort(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for Abort {
        fn drop(&mut self) {
            for j in &self.0 {
                j.abort();
            }
        }
    }
    let _guard = Abort(joins);
    loop {
        tokio::select! {
            ev = bus_rx.recv() => match ev {
                Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                    if let Some(tx) = by_entity.get(&entity_id).and_then(|id| tasks.get(id)) {
                        let _ = tx.send(DeviceMsg::Command(entity_id, target)).await;
                    }
                }
                Ok(Event::DeviceSet { integration, device_id, values }) if integration == INTEGRATION => {
                    if let Some(tx) = device_id.strip_prefix("tuya:").and_then(|id| tasks.get(id)) {
                        let _ = tx.send(DeviceMsg::Set(values)).await;
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
            _ = sweep.tick() => {
                let pending: Vec<TuyaDevice> = settings
                    .devices
                    .iter()
                    .filter(|d| tasks.contains_key(&d.id) && !located.contains_key(&d.id))
                    .cloned()
                    .collect();
                if !pending.is_empty() && sweeping.as_ref().is_none_or(|h| h.is_finished()) {
                    let taken = located.values().cloned().collect();
                    let tx = atx.clone();
                    sweeping = Some(tokio::spawn(async move {
                        for a in locate(pending, taken).await {
                            let _ = tx.send(a).await;
                        }
                    }));
                }
            }
            a = arx.recv() => if let Some(a) = a {
                located.insert(a.id.clone(), a.ip.clone());
                seen.lock().unwrap_or_else(|e| e.into_inner()).insert(a.id.clone(), a.clone());
                if let Some(tx) = tasks.get(&a.id) {
                    let _ = tx.send(DeviceMsg::Located(a.ip, a.version)).await;
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES_JSON: &str = r#"[
      {"name": "Prise salon", "id": "bf5a1c2d3e4f5a6b7c8d9e", "key": "a1b2c3d4e5f6g7h8", "category": "cz",
       "product_name": "LSC Smart Plug", "ip": "192.168.1.42", "version": "3.3",
       "mapping": {"1": {"code": "switch_1", "type": "Boolean", "values": {}},
                   "19": {"code": "cur_power", "type": "Integer", "values": {"unit": "W", "min": 0, "max": 99999, "scale": 1, "step": 1}},
                   "20": {"code": "cur_voltage", "type": "Integer", "values": {"unit": "V", "scale": 1}}}},
      {"name": "Ampoule", "id": "bf11223344556677889900", "key": "zyxwvutsrqponmlk", "category": "dj",
       "mapping": {"20": {"code": "switch_led", "type": "Boolean"}, "21": {"code": "work_mode", "type": "Enum"},
                   "22": {"code": "bright_value_v2", "type": "Integer", "values": "{\"min\":10,\"max\":1000}"},
                   "23": {"code": "temp_value_v2", "type": "Integer", "values": {"min": 0, "max": 1000}}}},
      {"name": "Sonnette", "id": "bfdoorbell0000000000", "key": "short", "category": "sp"},
      {"name": "Capteur Zigbee", "id": "a4c13800000000", "key": "a1b2c3d4e5f6g7h8", "sub": true}
    ]"#;

    #[test]
    fn imports_tinytuya_wizard_file() {
        let list = import_tinytuya(DEVICES_JSON).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].ip.as_deref(), Some("192.168.1.42"));
        let plug = map_device(&list[0]);
        let ids: Vec<&str> = plug.iter().map(|m| m.meta.entity_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "switch.prise_salon",
                "sensor.prise_salon_power",
                "sensor.prise_salon_voltage"
            ]
        );
        let bulb = map_device(&list[1]);
        assert_eq!(bulb[0].meta.domain, Domain::Light);
        assert_eq!(bulb[0].meta.capabilities["brightness"], json!(true));
    }

    #[test]
    fn states_and_commands_convert() {
        let list = import_tinytuya(DEVICES_JSON).unwrap();
        let plug = map_device(&list[0]);
        let mut dps = Map::new();
        dps.insert("1".into(), json!(true));
        dps.insert("19".into(), json!(853));
        assert_eq!(state_of(&plug[0].role, &dps).unwrap().0, json!("on"));
        assert_eq!(state_of(&plug[1].role, &dps).unwrap().0, json!(85.3));

        let bulb = map_device(&list[1]);
        let t = Target {
            on: true,
            brightness: Some(255),
            color_temp: Some(153),
        };
        let out = dps_for(&bulb[0].role, &t, &Map::new(), None).unwrap();
        assert_eq!(out["20"], json!(true));
        assert_eq!(out["22"], json!(1000));
        assert_eq!(out["23"], json!(1000));
        assert_eq!(out["21"], json!("white"));
        let mut back = Map::new();
        back.insert("20".into(), json!(true));
        back.insert("22".into(), json!(10));
        let (v, attrs) = state_of(&bulb[0].role, &back).unwrap();
        assert_eq!(v, json!("on"));
        assert_eq!(attrs["brightness"], json!(1));
    }

    #[test]
    fn a_colour_bulb_gets_the_lamp_panel() {
        let d = TuyaDevice {
            id: "bf0".into(),
            name: "Applique".into(),
            key: "0123456789abcdef".into(),
            ip: None,
            version: None,
            category: Some("dj".into()),
            product_name: None,
            mapping: BTreeMap::new(),
        };
        let keys: Vec<String> = device_controls(&d)
            .iter()
            .filter_map(|c| c.get("key").and_then(Value::as_str).map(str::to_string))
            .collect();
        for k in ["brightness", "color_temp", "color"] {
            assert!(keys.contains(&k.to_string()), "{k} manque : {keys:?}");
        }
        let lamp = map_device(&d);
        let Role::Light(l) = &lamp[0].role else {
            panic!("pas une lampe")
        };
        assert!(l.colour.is_some());
    }

    #[test]
    fn default_mapping_for_plugs() {
        let d = TuyaDevice {
            id: "x".into(),
            name: "Prise".into(),
            key: "a1b2c3d4e5f6g7h8".into(),
            ip: None,
            version: None,
            category: Some("cz".into()),
            product_name: None,
            mapping: BTreeMap::new(),
        };
        let m = map_device(&d);
        assert_eq!(m[0].meta.entity_id, "switch.prise");
        assert!(m.iter().any(|x| x.meta.entity_id == "sensor.prise_power"));
        let bell = TuyaDevice {
            category: Some("sp".into()),
            ..d
        };
        assert!(
            map_device(&bell).is_empty(),
            "une sonnette n'est pas une prise"
        );
    }
}
