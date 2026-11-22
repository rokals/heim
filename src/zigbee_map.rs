//! Carte du réseau Zigbee : qui passe par quel routeur.
//!
//! Zigbee2MQTT interroge chaque routeur sur ses voisins (`bridge/request/networkmap`, format
//! « raw ») : cela prend de quelques secondes à deux minutes et charge un peu le réseau. Heim
//! le fait donc **une fois par nuit à 3 h**, garde le résultat (`zigbee-map.json`) pour
//! l'afficher tout de suite, et on peut relancer le scan à la main.
//!
//! De la liste brute des liens, Heim tire un arbre : la clé en haut, chaque routeur sous celui
//! par lequel il rejoint la clé en moins de sauts (à égalité, le meilleur signal), chaque
//! appareil final sous son routeur « parent ». Et quelques conseils : signal faible, routeur
//! très chargé, appareil absent de la carte.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event};

const FILE: &str = "zigbee-map.json";
/// Au-delà, le scan est abandonné.
const TIMEOUT: Duration = Duration::from_secs(300);
/// En dessous (sur 255), le lien est jugé faible.
pub const WEAK_LQI: u32 = 40;
/// Au-delà, un routeur porte beaucoup d'appareils.
pub const BUSY_CHILDREN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Coordinator,
    Router,
    EndDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub ieee: String,
    pub name: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Par où il rejoint la clé (aucun : la clé elle-même, ou isolé).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Qualité du lien vers son parent (0-255).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lqi: Option<u32>,
    /// Sauts jusqu'à la clé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hops: Option<u32>,
    /// Nombre de voisins vus (routeurs : maillage).
    #[serde(default)]
    pub neighbors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub lqi: u32,
    /// 0 parent, 1 enfant, 2 frère, 3 aucun, 4 ancien enfant (vu par `target`).
    pub relationship: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Advice {
    /// « weak », « busy », « orphan », « missing ».
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ieee: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMap {
    pub scanned_at: DateTime<Utc>,
    pub duration_s: u64,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    #[serde(default)]
    pub advice: Vec<Advice>,
}

fn ieee_of(v: &Value) -> Option<String> {
    v.get("ieeeAddr")
        .or_else(|| v.get("ieee_address"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn role_of(t: &str) -> Role {
    match t {
        "Coordinator" => Role::Coordinator,
        "Router" => Role::Router,
        _ => Role::EndDevice,
    }
}

/// Lit la réponse « raw » de Zigbee2MQTT et calcule l'arbre.
pub fn parse(data: &Value, duration_s: u64) -> Option<NetworkMap> {
    let value = data.get("value").unwrap_or(data);
    let raw_nodes = value.get("nodes")?.as_array()?;
    let mut nodes: Vec<Node> = raw_nodes
        .iter()
        .filter_map(|n| {
            let ieee = ieee_of(n)?;
            let role = role_of(n.get("type").and_then(Value::as_str).unwrap_or(""));
            let name = n
                .get("friendlyName")
                .or_else(|| n.get("friendly_name"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if role == Role::Coordinator {
                        "Clé Zigbee".into()
                    } else {
                        ieee.clone()
                    }
                });
            let model = n
                .get("definition")
                .and_then(|d| d.get("model"))
                .or_else(|| n.get("modelID"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(Node {
                ieee,
                name,
                role,
                model,
                parent: None,
                lqi: None,
                hops: None,
                neighbors: 0,
            })
        })
        .collect();
    let links: Vec<Link> = value
        .get("links")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|l| {
                    let source = l.get("source").and_then(ieee_of)?;
                    let target = l.get("target").and_then(ieee_of)?;
                    let lqi = l
                        .get("lqi")
                        .or_else(|| l.get("linkquality"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    let relationship =
                        l.get("relationship").and_then(Value::as_u64).unwrap_or(3) as u8;
                    (source != target).then_some(Link {
                        source,
                        target,
                        lqi,
                        relationship,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    build_tree(&mut nodes, &links);
    let advice = advise(&nodes);
    Some(NetworkMap {
        scanned_at: Utc::now(),
        duration_s,
        nodes,
        links,
        advice,
    })
}

/// Place chaque appareil sous son parent.
pub fn build_tree(nodes: &mut [Node], links: &[Link]) {
    let role: HashMap<String, Role> = nodes.iter().map(|n| (n.ieee.clone(), n.role)).collect();
    let is_router = |id: &str| matches!(role.get(id), Some(Role::Router | Role::Coordinator));
    // Liens entre routeurs (et la clé), dans les deux sens, avec le meilleur signal vu.
    let mut mesh: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut neighbors: HashMap<String, HashSet<String>> = HashMap::new();
    for l in links {
        neighbors
            .entry(l.target.clone())
            .or_default()
            .insert(l.source.clone());
        neighbors
            .entry(l.source.clone())
            .or_default()
            .insert(l.target.clone());
        if is_router(&l.source) && is_router(&l.target) {
            for (a, b) in [(&l.source, &l.target), (&l.target, &l.source)] {
                let e = mesh
                    .entry(a.clone())
                    .or_default()
                    .entry(b.clone())
                    .or_insert(0);
                *e = (*e).max(l.lqi);
            }
        }
    }
    // Sauts depuis la clé.
    let mut hops: HashMap<String, u32> = HashMap::new();
    let mut queue = VecDeque::new();
    for n in nodes.iter().filter(|n| n.role == Role::Coordinator) {
        hops.insert(n.ieee.clone(), 0);
        queue.push_back(n.ieee.clone());
    }
    while let Some(id) = queue.pop_front() {
        let h = hops[&id];
        for next in mesh
            .get(&id)
            .map(|m| m.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            if !hops.contains_key(&next) {
                hops.insert(next.clone(), h + 1);
                queue.push_back(next);
            }
        }
    }
    for n in nodes.iter_mut() {
        n.neighbors = neighbors.get(&n.ieee).map_or(0, |s| s.len() as u32);
        match n.role {
            Role::Coordinator => n.hops = Some(0),
            Role::Router => {
                let Some(&h) = hops.get(&n.ieee) else {
                    continue;
                };
                n.hops = Some(h);
                // Le voisin un saut plus près de la clé, au meilleur signal.
                if let Some((p, lqi)) = mesh
                    .get(&n.ieee)
                    .into_iter()
                    .flatten()
                    .filter(|(p, _)| hops.get(*p) == Some(&(h.saturating_sub(1))))
                    .max_by_key(|(_, lqi)| **lqi)
                {
                    n.parent = Some(p.clone());
                    n.lqi = Some(*lqi);
                }
            }
            Role::EndDevice => {
                // Son parent déclaré (« enfant de »), sinon le routeur qui l'entend le mieux.
                let pick = |child_only: bool| {
                    links
                        .iter()
                        .filter(|l| l.source == n.ieee && is_router(&l.target))
                        .filter(|l| !child_only || l.relationship == 1)
                        .max_by_key(|l| l.lqi)
                };
                if let Some(l) = pick(true).or_else(|| pick(false)) {
                    n.parent = Some(l.target.clone());
                    n.lqi = Some(l.lqi);
                    n.hops = hops.get(&l.target).map(|h| h + 1);
                }
            }
        }
    }
}

fn advise(nodes: &[Node]) -> Vec<Advice> {
    let mut out = Vec::new();
    let name: HashMap<&str, &str> = nodes
        .iter()
        .map(|n| (n.ieee.as_str(), n.name.as_str()))
        .collect();
    let mut children: HashMap<&str, usize> = HashMap::new();
    for n in nodes {
        if let Some(p) = &n.parent {
            *children.entry(p.as_str()).or_default() += 1;
        }
    }
    for n in nodes {
        if n.role == Role::Coordinator {
            continue;
        }
        match (&n.parent, n.lqi) {
            (None, _) => out.push(Advice {
                kind: "orphan".into(),
                ieee: Some(n.ieee.clone()),
                text: format!("« {} » n'a été vu par aucun routeur pendant le scan.", n.name),
            }),
            (Some(p), Some(q)) if q < WEAK_LQI => out.push(Advice {
                kind: "weak".into(),
                ieee: Some(n.ieee.clone()),
                text: format!(
                    "« {} » a un signal faible ({q}/255) vers « {} » : un routeur (prise, ampoule) plus près l'aiderait.",
                    n.name,
                    name.get(p.as_str()).unwrap_or(&p.as_str())
                ),
            }),
            _ => {}
        }
    }
    for (p, c) in children {
        if c >= BUSY_CHILDREN {
            out.push(Advice {
                kind: "busy".into(),
                ieee: Some(p.to_string()),
                text: format!(
                    "{c} appareils passent par « {} » : s'il est coupé, ils devront trouver un autre chemin.",
                    name.get(p).unwrap_or(&p)
                ),
            });
        }
    }
    out
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub scanning: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<NetworkMap>,
}

pub struct Mapper {
    state: Mutex<Status>,
    path: PathBuf,
    bus: Bus,
}

impl Mapper {
    pub fn new(data_dir: &std::path::Path, bus: Bus) -> Arc<Mapper> {
        let path = data_dir.join(FILE);
        let map = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<NetworkMap>(&b).ok());
        Arc::new(Mapper {
            state: Mutex::new(Status {
                map,
                ..Default::default()
            }),
            path,
            bus,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Status> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> Status {
        self.lock().clone()
    }

    /// Lance un scan (sans effet si un scan est déjà en cours).
    pub fn scan(self: &Arc<Self>) {
        {
            let mut st = self.lock();
            if st.scanning {
                return;
            }
            st.scanning = true;
            st.started = Some(Utc::now());
            st.error = None;
        }
        self.bus.publish(Event::ZigbeeMapChanged);
        let me = self.clone();
        let mut rx = self.bus.subscribe();
        tokio::spawn(async move {
            let t0 = std::time::Instant::now();
            me.bus.publish(Event::ZigbeeMapRequest);
            let deadline = tokio::time::Instant::now() + TIMEOUT;
            let result: Result<NetworkMap, String> = loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Ok(Event::ZigbeeMap { data, error }) => {
                            break match error {
                                Some(e) => Err(e),
                                None => parse(&data, t0.elapsed().as_secs())
                                    .ok_or_else(|| "réponse de Zigbee2MQTT illisible".to_string()),
                            };
                        }
                        Err(RecvError::Closed) => return,
                        _ => {}
                    },
                    _ = tokio::time::sleep_until(deadline) => {
                        break Err("Zigbee2MQTT n'a pas répondu en 5 minutes".into());
                    }
                }
            };
            {
                let mut st = me.lock();
                st.scanning = false;
                match result {
                    Ok(map) => {
                        if let Ok(b) = serde_json::to_vec(&map) {
                            let _ = std::fs::write(&me.path, b);
                        }
                        tracing::info!(
                            "carte Zigbee : {} appareils en {} s",
                            map.nodes.len(),
                            map.duration_s
                        );
                        st.map = Some(map);
                    }
                    Err(e) => {
                        tracing::warn!("carte Zigbee : {e}");
                        st.error = Some(e);
                    }
                }
            }
            me.bus.publish(Event::ZigbeeMapChanged);
        });
    }

    /// Chaque nuit à 3 h.
    pub fn spawn_nightly(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut last_day = None;
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let now = Local::now();
                if now.hour() == 3 && last_day != Some(now.date_naive()) {
                    last_day = Some(now.date_naive());
                    me.scan();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_tree_from_raw_map() {
        let data = json!({"type": "raw", "value": {
            "nodes": [
                {"ieeeAddr": "0xc", "type": "Coordinator"},
                {"ieeeAddr": "0xr1", "friendlyName": "Prise salon", "type": "Router"},
                {"ieeeAddr": "0xr2", "friendlyName": "Ampoule grenier", "type": "Router"},
                {"ieeeAddr": "0xe1", "friendlyName": "Porte grenier", "type": "EndDevice"},
                {"ieeeAddr": "0xe2", "friendlyName": "Capteur jardin", "type": "EndDevice"}
            ],
            "links": [
                {"source": {"ieeeAddr": "0xr1"}, "target": {"ieeeAddr": "0xc"}, "lqi": 200, "relationship": 2},
                {"source": {"ieeeAddr": "0xr2"}, "target": {"ieeeAddr": "0xr1"}, "lqi": 120, "relationship": 2},
                {"source": {"ieeeAddr": "0xr2"}, "target": {"ieeeAddr": "0xc"}, "lqi": 20, "relationship": 3},
                {"source": {"ieeeAddr": "0xe1"}, "target": {"ieeeAddr": "0xr2"}, "lqi": 90, "relationship": 1},
                {"source": {"ieeeAddr": "0xe1"}, "target": {"ieeeAddr": "0xr1"}, "lqi": 150, "relationship": 3},
                {"source": {"ieeeAddr": "0xe2"}, "target": {"ieeeAddr": "0xr1"}, "lqi": 25, "relationship": 1}
            ]
        }});
        let m = parse(&data, 12).unwrap();
        let get = |id: &str| m.nodes.iter().find(|n| n.ieee == id).unwrap();
        assert_eq!(get("0xc").name, "Clé Zigbee");
        assert_eq!(get("0xr1").parent.as_deref(), Some("0xc"));
        // Le grenier est à un saut de la clé (lien faible) : il y reste, même si la prise
        // l'entend mieux, car c'est le chemin le plus court.
        assert_eq!(get("0xr2").hops, Some(1));
        // L'appareil final suit son parent déclaré, pas le meilleur signal.
        assert_eq!(get("0xe1").parent.as_deref(), Some("0xr2"));
        assert_eq!(get("0xe1").hops, Some(2));
        assert!(
            m.advice
                .iter()
                .any(|a| a.kind == "weak" && a.ieee.as_deref() == Some("0xe2"))
        );
        assert!(
            m.advice
                .iter()
                .any(|a| a.kind == "weak" && a.ieee.as_deref() == Some("0xr2"))
        );
    }
}
