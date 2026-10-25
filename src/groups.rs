//! Groupes : plusieurs lumières ou prises commandées comme un seul appareil
//! (« Salon » = plafonnier + lampadaire + guirlande).
//!
//! Un groupe est l'entité `group.<id>` : allumé si au moins un membre l'est. Une commande
//! (allumer à 40 %, éteindre, basculer) part vers chaque membre.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event};
use crate::model::{Attributes, Category, Domain, EntityId, EntityMeta, State, slugify};
use crate::registry::RegistryHandle;
use crate::settings::Settings;

pub const INTEGRATION: &str = "groups";
pub const SETTINGS_KEY: &str = "groups";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    pub members: Vec<EntityId>,
}

/// Nettoie la liste reçue de l'interface : noms, identifiants, membres valides.
pub fn validate(groups: Vec<Group>) -> Result<Vec<Group>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for mut g in groups {
        g.name = g.name.trim().to_string();
        if g.name.is_empty() {
            return Err("un groupe doit avoir un nom".into());
        }
        if g.id.is_empty() {
            g.id = slugify(&g.name);
        }
        if !seen.insert(g.id.clone()) {
            return Err(format!("deux groupes s'appellent « {} »", g.name));
        }
        g.members
            .retain(|m| m.starts_with("light.") || m.starts_with("switch."));
        g.members.dedup();
        if g.members.len() < 2 {
            return Err(format!(
                "« {} » : choisissez au moins deux appareils",
                g.name
            ));
        }
        g.room = g
            .room
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty());
        out.push(g);
    }
    Ok(out)
}

/// État du groupe à partir de ceux des membres.
pub fn compute(
    members: &[EntityId],
    states: &HashMap<EntityId, State>,
) -> (Value, Attributes, bool) {
    let known: Vec<&State> = members.iter().filter_map(|m| states.get(m)).collect();
    let on: Vec<&&State> = known.iter().filter(|s| s.is_on()).collect();
    let mut attrs = Attributes::new();
    attrs.insert("on_count".into(), json!(on.len()));
    attrs.insert("members".into(), json!(members));
    let levels: Vec<u64> = on
        .iter()
        .filter_map(|s| s.brightness().map(u64::from))
        .collect();
    if !levels.is_empty() {
        attrs.insert(
            "brightness".into(),
            json!(levels.iter().sum::<u64>() / levels.len() as u64),
        );
    }
    let available = known.iter().any(|s| s.available);
    let value = if known.is_empty() {
        Value::Null
    } else if on.is_empty() {
        json!("off")
    } else {
        json!("on")
    };
    (value, attrs, available)
}

pub fn spawn(registry: RegistryHandle, bus: &Bus, settings: Arc<Settings>) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        let mut states: HashMap<EntityId, State> = registry
            .snapshot()
            .await
            .into_iter()
            .filter_map(|e| e.state.map(|s| (e.meta.entity_id, s)))
            .collect();
        reload(&registry, &settings, &mut groups, &states).await;
        loop {
            match rx.recv().await {
                Ok(Event::SettingsChanged { key }) if key == SETTINGS_KEY => {
                    reload(&registry, &settings, &mut groups, &states).await;
                }
                Ok(Event::StateChanged { entity_id, new, .. }) => {
                    if entity_id.starts_with("group.") {
                        continue;
                    }
                    states.insert(entity_id.clone(), new);
                    for g in groups.values().filter(|g| g.members.contains(&entity_id)) {
                        publish(&registry, g, &states).await;
                    }
                }
                Ok(Event::CommandRequested {
                    integration,
                    entity_id,
                    target,
                    ..
                }) if integration == INTEGRATION => {
                    let Some(g) = entity_id
                        .strip_prefix("group.")
                        .and_then(|id| groups.get(id))
                    else {
                        continue;
                    };
                    let source = format!("group:{}", g.id);
                    for m in &g.members {
                        let mut t = target.clone();
                        // Une prise ne connaît pas la luminosité.
                        if m.starts_with("switch.") {
                            t.brightness = None;
                            t.color_temp = None;
                        }
                        registry.apply(m, t, &source).await;
                    }
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    states = registry
                        .snapshot()
                        .await
                        .into_iter()
                        .filter_map(|e| e.state.map(|s| (e.meta.entity_id, s)))
                        .collect();
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn reload(
    registry: &RegistryHandle,
    settings: &Settings,
    groups: &mut BTreeMap<String, Group>,
    states: &HashMap<EntityId, State>,
) {
    let wanted: Vec<Group> = settings.get(SETTINGS_KEY).unwrap_or_default();
    let wanted: BTreeMap<String, Group> = wanted.into_iter().map(|g| (g.id.clone(), g)).collect();
    for id in groups.keys() {
        if !wanted.contains_key(id) {
            registry.remove(&format!("group.{id}")).await;
        }
    }
    *groups = wanted;
    for g in groups.values() {
        let mut capabilities = Attributes::new();
        capabilities.insert("members".into(), json!(g.members));
        if g.members.iter().any(|m| m.starts_with("light.")) {
            capabilities.insert("brightness".into(), json!(true));
        }
        registry
            .upsert(EntityMeta {
                entity_id: format!("group.{}", g.id),
                domain: Domain::Group,
                name: g.name.clone(),
                integration: INTEGRATION.into(),
                device_id: None,
                room: g.room.clone(),
                plan: None,
                capabilities,
                // Nom et pièce viennent des réglages du groupe, pas d'une intégration.
                user_edited: false,
                category: Category::Primary,
                hidden: false,
            })
            .await;
        publish(registry, g, states).await;
    }
}

async fn publish(registry: &RegistryHandle, g: &Group, states: &HashMap<EntityId, State>) {
    let (value, attrs, available) = compute(&g.members, states);
    registry
        .report(format!("group.{}", g.id), value, attrs, available)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(v: &str, b: Option<u8>) -> State {
        let mut a = Attributes::new();
        if let Some(b) = b {
            a.insert("brightness".into(), json!(b));
        }
        State::new(json!(v), a, true)
    }

    #[test]
    fn group_is_on_when_any_member_is() {
        let members = vec!["light.a".to_string(), "light.b".to_string()];
        let mut states = HashMap::new();
        states.insert("light.a".to_string(), st("off", None));
        states.insert("light.b".to_string(), st("on", Some(100)));
        let (v, attrs, available) = compute(&members, &states);
        assert_eq!(v, json!("on"));
        assert_eq!(attrs["on_count"], json!(1));
        assert_eq!(attrs["brightness"], json!(100));
        assert!(available);
        states.insert("light.b".to_string(), st("off", None));
        assert_eq!(compute(&members, &states).0, json!("off"));
    }

    #[test]
    fn validation() {
        let ok = validate(vec![Group {
            id: String::new(),
            name: " Salon ".into(),
            room: None,
            members: vec!["light.a".into(), "switch.b".into(), "sensor.x".into()],
        }])
        .unwrap();
        assert_eq!(ok[0].id, "salon");
        assert_eq!(ok[0].members.len(), 2);
        assert!(
            validate(vec![Group {
                id: String::new(),
                name: "Seul".into(),
                room: None,
                members: vec!["light.a".into()],
            }])
            .is_err()
        );
    }
}
