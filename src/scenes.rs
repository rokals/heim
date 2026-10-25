//! Scènes avec état.
//!
//! Une scène est un état désiré pour un ensemble de lumières et de prises.
//! Le moteur sait en permanence si elle est active en comparant cet état désiré
//! à l'état réel. Si quelqu'un change une lampe à la main, la scène passe
//! d'elle-même à inactive, sans groupe ni automatisation à écrire.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};

use crate::bus::{Bus, Event};
use crate::model::{Attributes, Domain, EntityId, EntityMeta, State, Target, slugify};
use crate::registry::RegistryHandle;
use crate::store::{StoreHandle, Table};

pub const INTEGRATION: &str = "scenes";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    pub targets: BTreeMap<EntityId, Target>,
    /// État d'avant l'activation, pour « désactiver » = revenir comme avant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<BTreeMap<EntityId, Target>>,
}

impl Scene {
    pub fn entity_id(&self) -> EntityId {
        format!("scene.{}", self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneStatus {
    /// Au moins un membre ne correspond pas.
    Off,
    /// Tous les membres correspondent.
    On,
    /// Tous les membres joignables correspondent, mais certains sont injoignables.
    Partial,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneView {
    #[serde(flatten)]
    pub scene: Scene,
    pub status: SceneStatus,
}

/// Ce que l'utilisateur envoie pour créer ou modifier une scène.
#[derive(Debug, Clone, Deserialize)]
pub struct SceneInput {
    pub name: String,
    #[serde(default)]
    pub room: Option<String>,
    pub targets: BTreeMap<EntityId, Target>,
}

/// « Enregistrer l'éclairage actuel comme scène ».
#[derive(Debug, Clone, Deserialize)]
pub struct CaptureInput {
    pub name: String,
    #[serde(default)]
    pub room: Option<String>,
    pub entities: Vec<EntityId>,
}

enum Msg {
    List(oneshot::Sender<Vec<SceneView>>),
    Upsert(
        String,
        SceneInput,
        oneshot::Sender<Result<SceneView, String>>,
    ),
    Capture(
        String,
        CaptureInput,
        oneshot::Sender<Result<SceneView, String>>,
    ),
    Delete(String, oneshot::Sender<bool>),
}

#[derive(Clone)]
pub struct SceneHandle {
    tx: mpsc::Sender<Msg>,
}

impl SceneHandle {
    pub async fn list(&self) -> Vec<SceneView> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::List(tx)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn upsert(&self, id: &str, input: SceneInput) -> Result<SceneView, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Upsert(id.to_string(), input, tx))
            .await
            .map_err(|_| "moteur de scènes arrêté")?;
        rx.await
            .map_err(|_| "moteur de scènes arrêté".to_string())?
    }

    pub async fn capture(&self, id: &str, input: CaptureInput) -> Result<SceneView, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Capture(id.to_string(), input, tx))
            .await
            .map_err(|_| "moteur de scènes arrêté")?;
        rx.await
            .map_err(|_| "moteur de scènes arrêté".to_string())?
    }

    pub async fn delete(&self, id: &str) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Delete(id.to_string(), tx)).await.is_err() {
            return false;
        }
        rx.await.unwrap_or(false)
    }
}

struct Engine {
    scenes: BTreeMap<String, Scene>,
    status: HashMap<String, SceneStatus>,
    /// Dernier état connu des entités membres d'au moins une scène (et des autres, c'est léger).
    cache: HashMap<EntityId, State>,
    registry: RegistryHandle,
    store: StoreHandle,
}

pub fn spawn(
    scenes: Vec<Scene>,
    registry: RegistryHandle,
    bus: Bus,
    store: StoreHandle,
) -> SceneHandle {
    let (tx, rx) = mpsc::channel(64);
    // On s'abonne avant de lire l'instantané : aucun changement ne peut passer entre les deux.
    let bus_rx = bus.subscribe();
    let engine = Engine {
        scenes: scenes.into_iter().map(|s| (s.id.clone(), s)).collect(),
        status: HashMap::new(),
        cache: HashMap::new(),
        registry,
        store,
    };
    tokio::spawn(engine.run(rx, bus_rx));
    SceneHandle { tx }
}

fn compute_status(scene: &Scene, cache: &HashMap<EntityId, State>) -> (SceneStatus, usize) {
    let (mut matched, mut mismatched, mut unavailable) = (0usize, 0usize, 0usize);
    for (id, target) in &scene.targets {
        match cache.get(id) {
            Some(state) if state.available => {
                if target.matches(state) {
                    matched += 1;
                } else {
                    mismatched += 1;
                }
            }
            _ => unavailable += 1,
        }
    }
    let status = if mismatched > 0 || matched == 0 {
        SceneStatus::Off
    } else if unavailable > 0 {
        SceneStatus::Partial
    } else {
        SceneStatus::On
    };
    (status, unavailable)
}

fn validate_targets(targets: &BTreeMap<EntityId, Target>) -> Result<(), String> {
    if targets.is_empty() {
        return Err("une scène doit contenir au moins une lumière ou une prise".into());
    }
    for id in targets.keys() {
        if !(id.starts_with("light.") || id.starts_with("switch.")) {
            return Err(format!(
                "{id} : seules les lumières et les prises peuvent faire partie d'une scène"
            ));
        }
    }
    Ok(())
}

fn scene_id(raw: &str, name: &str) -> Result<String, String> {
    let id = slugify(if raw.is_empty() { name } else { raw });
    if id.is_empty() {
        Err("identifiant de scène invalide".into())
    } else {
        Ok(id)
    }
}

impl Engine {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Msg>,
        mut bus_rx: tokio::sync::broadcast::Receiver<Event>,
    ) {
        self.resync().await;
        let ids: Vec<String> = self.scenes.keys().cloned().collect();
        for id in &ids {
            self.declare(id).await;
            self.refresh(id, true).await;
        }

        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => break,
                },
                ev = bus_rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, new, .. }) => {
                        if entity_id.starts_with("scene.") {
                            continue;
                        }
                        self.cache.insert(entity_id.clone(), new);
                        let touched: Vec<String> = self
                            .scenes
                            .values()
                            .filter(|s| s.targets.contains_key(&entity_id))
                            .map(|s| s.id.clone())
                            .collect();
                        for id in touched {
                            self.refresh(&id, false).await;
                        }
                    }
                    Ok(Event::EntityRemoved { entity_id }) => {
                        self.cache.remove(&entity_id);
                    }
                    Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                        if let Some(id) = entity_id.strip_prefix("scene.") {
                            if target.on {
                                self.activate(id).await;
                            } else {
                                self.deactivate(id).await;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!("scènes : {n} événements perdus, resynchronisation");
                        self.resync().await;
                        let ids: Vec<String> = self.scenes.keys().cloned().collect();
                        for id in ids {
                            self.refresh(&id, false).await;
                        }
                    }
                    Err(RecvError::Closed) => break,
                },
            }
        }
    }

    async fn resync(&mut self) {
        self.cache = self
            .registry
            .snapshot()
            .await
            .into_iter()
            .filter_map(|e| e.state.map(|s| (e.meta.entity_id, s)))
            .collect();
    }

    async fn declare(&self, id: &str) {
        let Some(scene) = self.scenes.get(id) else {
            return;
        };
        let mut capabilities = Attributes::new();
        capabilities.insert(
            "members".into(),
            json!(scene.targets.keys().collect::<Vec<_>>()),
        );
        self.registry
            .upsert(EntityMeta {
                entity_id: scene.entity_id(),
                domain: Domain::Scene,
                name: scene.name.clone(),
                integration: INTEGRATION.into(),
                device_id: None,
                room: scene.room.clone(),
                plan: None,
                capabilities,
                user_edited: false,
                category: crate::model::Category::Primary,
                hidden: false,
            })
            .await;
    }

    /// Recalcule l'état d'une scène et le publie s'il a changé.
    async fn refresh(&mut self, id: &str, force: bool) {
        let Some(scene) = self.scenes.get(id) else {
            return;
        };
        let (status, unavailable) = compute_status(scene, &self.cache);
        let previous = self.status.insert(id.to_string(), status);
        if !force && previous == Some(status) {
            return;
        }
        // La scène vient de se « défaire » d'elle-même : l'état à restaurer n'a plus de sens.
        if status == SceneStatus::Off
            && matches!(previous, Some(SceneStatus::On | SceneStatus::Partial))
            && let Some(scene) = self.scenes.get_mut(id)
            && scene.restore.take().is_some()
        {
            self.store.put(Table::Scenes, id, scene);
        }
        let Some(scene) = self.scenes.get(id) else {
            return;
        };
        let mut attrs = Attributes::new();
        attrs.insert("partial".into(), json!(status == SceneStatus::Partial));
        attrs.insert("unavailable_members".into(), json!(unavailable));
        let value = if status == SceneStatus::Off {
            "off"
        } else {
            "on"
        };
        self.registry
            .report(scene.entity_id(), json!(value), attrs, true)
            .await;
    }

    async fn activate(&mut self, id: &str) {
        let already_on = matches!(
            self.status.get(id),
            Some(SceneStatus::On | SceneStatus::Partial)
        );
        let Some(scene) = self.scenes.get_mut(id) else {
            return;
        };
        if !already_on {
            let snapshot: BTreeMap<EntityId, Target> = scene
                .targets
                .keys()
                .filter_map(|eid| {
                    self.cache
                        .get(eid)
                        .filter(|s| s.available)
                        .map(|s| (eid.clone(), Target::from_state(s)))
                })
                .collect();
            scene.restore = Some(snapshot);
            self.store.put(Table::Scenes, id, scene);
        }
        let source = format!("scene:{id}");
        let targets: Vec<(EntityId, Target)> = scene
            .targets
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        tracing::info!("activation de la scène {id}");
        for (eid, target) in targets {
            self.registry.apply(&eid, target, &source).await;
        }
    }

    async fn deactivate(&mut self, id: &str) {
        let active = matches!(
            self.status.get(id),
            Some(SceneStatus::On | SceneStatus::Partial)
        );
        let Some(scene) = self.scenes.get_mut(id) else {
            return;
        };
        let restore = scene.restore.take();
        self.store.put(Table::Scenes, id, scene);
        if !active {
            // Scène déjà inactive : on n'éteint rien par surprise.
            return;
        }
        let plan: Vec<(EntityId, Target)> = match restore {
            Some(r) => r.into_iter().collect(),
            None => scene
                .targets
                .keys()
                .map(|k| (k.clone(), Target::off()))
                .collect(),
        };
        tracing::info!("désactivation de la scène {id}");
        let source = format!("scene:{id}");
        for (eid, target) in plan {
            self.registry.apply(&eid, target, &source).await;
        }
    }

    async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::List(reply) => {
                let views = self
                    .scenes
                    .values()
                    .map(|s| SceneView {
                        scene: s.clone(),
                        status: self.status.get(&s.id).copied().unwrap_or(SceneStatus::Off),
                    })
                    .collect();
                let _ = reply.send(views);
            }
            Msg::Upsert(raw_id, input, reply) => {
                let result = self
                    .upsert(raw_id, input.name, input.room, input.targets)
                    .await;
                let _ = reply.send(result);
            }
            Msg::Capture(raw_id, input, reply) => {
                let mut targets = BTreeMap::new();
                for eid in input.entities {
                    match self.cache.get(&eid) {
                        Some(s) if s.available => {
                            targets.insert(eid, Target::from_state(s));
                        }
                        _ => {
                            let _ = reply.send(Err(format!(
                                "{eid} est injoignable ou inconnue, impossible de la capturer"
                            )));
                            return;
                        }
                    }
                }
                let result = self.upsert(raw_id, input.name, input.room, targets).await;
                let _ = reply.send(result);
            }
            Msg::Delete(id, reply) => {
                let existed = self.scenes.remove(&id).is_some();
                if existed {
                    self.status.remove(&id);
                    self.store.delete(Table::Scenes, &id);
                    self.registry.remove(&format!("scene.{id}")).await;
                }
                let _ = reply.send(existed);
            }
        }
    }

    async fn upsert(
        &mut self,
        raw_id: String,
        name: String,
        room: Option<String>,
        targets: BTreeMap<EntityId, Target>,
    ) -> Result<SceneView, String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("la scène doit avoir un nom".into());
        }
        let id = scene_id(&raw_id, &name)?;
        validate_targets(&targets)?;
        let restore = self.scenes.get(&id).and_then(|s| s.restore.clone());
        let scene = Scene {
            id: id.clone(),
            name,
            room,
            targets,
            restore,
        };
        self.store.put(Table::Scenes, &id, &scene);
        self.scenes.insert(id.clone(), scene);
        self.declare(&id).await;
        self.refresh(&id, true).await;
        let scene = self.scenes[&id].clone();
        let status = self.status.get(&id).copied().unwrap_or(SceneStatus::Off);
        Ok(SceneView { scene, status })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn st(on: bool, available: bool) -> State {
        State::new(
            json!(if on { "on" } else { "off" }),
            Attributes::new(),
            available,
        )
    }

    fn scene(members: &[&str]) -> Scene {
        Scene {
            id: "soiree".into(),
            name: "Soirée".into(),
            room: None,
            targets: members
                .iter()
                .map(|m| (m.to_string(), Target::on()))
                .collect(),
            restore: None,
        }
    }

    #[test]
    fn status_rules() {
        let s = scene(&["light.a", "light.b"]);
        let mut cache = HashMap::new();
        cache.insert("light.a".to_string(), st(true, true));
        cache.insert("light.b".to_string(), st(true, true));
        assert_eq!(compute_status(&s, &cache).0, SceneStatus::On);

        cache.insert("light.b".to_string(), st(false, true));
        assert_eq!(compute_status(&s, &cache).0, SceneStatus::Off);

        cache.insert("light.b".to_string(), st(true, false));
        assert_eq!(compute_status(&s, &cache).0, SceneStatus::Partial);

        cache.insert("light.a".to_string(), st(true, false));
        assert_eq!(
            compute_status(&s, &cache).0,
            SceneStatus::Off,
            "aucun membre joignable"
        );
    }
}
