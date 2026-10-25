//! Registre d'états : seul propriétaire de toutes les entités.
//!
//! C'est un acteur : une tâche unique qui reçoit des messages. Pas de verrou partagé,
//! donc pas de course entre deux changements d'état. Le registre n'attend jamais
//! personne (il publie sur le bus et envoie au store sans bloquer), ce qui exclut
//! tout interblocage avec les moteurs qui l'appellent.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;

use crate::bus::{Bus, Event};
use crate::model::{
    Action, Attributes, Desired, EntityId, EntityMeta, EntityView, PlanPos, State, Target,
};
use crate::store::StoreHandle;

/// Délai au-delà duquel un appareil qui n'a pas confirmé une commande est signalé.
const COMMAND_TIMEOUT: chrono::TimeDelta = chrono::TimeDelta::seconds(15);
const SWEEP_EVERY: Duration = Duration::from_secs(5);
/// Attributs bavards qui ne justifient pas, seuls, un événement ni une écriture.
const NOISY_ATTRIBUTES: &[&str] = &["linkquality", "last_seen", "update"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    NotFound(EntityId),
    NotControllable(EntityId),
    Stopped,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NotFound(id) => write!(f, "entité inconnue : {id}"),
            CommandError::NotControllable(id) => write!(f, "{id} ne se commande pas (capteur)"),
            CommandError::Stopped => write!(f, "le noyau est arrêté"),
        }
    }
}

impl std::error::Error for CommandError {}

pub enum CommandRequest {
    Action(Action),
    Target(Target),
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct MetaPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// `Some(None)` retire la pièce.
    #[serde(default, with = "double_option")]
    pub room: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub plan: Option<Option<PlanPos>>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

type Reply<T> = oneshot::Sender<T>;

enum Msg {
    Upsert(EntityMeta),
    Report {
        entity_id: EntityId,
        value: Value,
        attributes: Attributes,
        available: bool,
    },
    Availability {
        entity_id: EntityId,
        available: bool,
    },
    Seen {
        entity_id: EntityId,
        attributes: Attributes,
    },
    Command {
        entity_id: EntityId,
        request: CommandRequest,
        source: String,
        reply: Option<Reply<Result<Target, CommandError>>>,
    },
    UpdateMeta {
        entity_id: EntityId,
        patch: MetaPatch,
        reply: Reply<Result<EntityView, CommandError>>,
    },
    Remove(EntityId),
    RenameRoom {
        from: String,
        to: Option<String>,
        reply: Reply<usize>,
    },
    Snapshot(Reply<Vec<EntityView>>),
    Get(EntityId, Reply<Option<EntityView>>),
}

/// Attribut posé par « Pile changée » : la valeur périmée à ignorer, et depuis quand.
pub const STALE_VALUE: &str = "stale_value";
pub const STALE_SINCE: &str = "stale_since";

#[derive(Clone)]
pub struct RegistryHandle {
    tx: mpsc::Sender<Msg>,
}

impl RegistryHandle {
    /// Déclare une entité (ou met à jour ce que l'intégration sait d'elle).
    pub async fn upsert(&self, meta: EntityMeta) {
        let _ = self.tx.send(Msg::Upsert(meta)).await;
    }

    /// Rapporte l'état réel d'une entité. Les attributs sont fusionnés avec les précédents.
    pub async fn report(
        &self,
        entity_id: impl Into<EntityId>,
        value: Value,
        attributes: Attributes,
        available: bool,
    ) {
        let _ = self
            .tx
            .send(Msg::Report {
                entity_id: entity_id.into(),
                value,
                attributes,
                available,
            })
            .await;
    }

    /// L'appareil a parlé sans donner de nouvelle valeur pour cette entité : on note qu'il est
    /// vivant (« vu il y a… ») et ses attributs techniques (qualité du signal).
    pub async fn seen(&self, entity_id: impl Into<EntityId>, attributes: Attributes) {
        let _ = self
            .tx
            .send(Msg::Seen {
                entity_id: entity_id.into(),
                attributes,
            })
            .await;
    }

    pub async fn set_available(&self, entity_id: impl Into<EntityId>, available: bool) {
        let _ = self
            .tx
            .send(Msg::Availability {
                entity_id: entity_id.into(),
                available,
            })
            .await;
    }

    /// Commande avec réponse : renvoie la cible effectivement demandée.
    pub async fn command(
        &self,
        entity_id: &str,
        action: Action,
        source: &str,
    ) -> Result<Target, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::Command {
                entity_id: entity_id.to_string(),
                request: CommandRequest::Action(action),
                source: source.to_string(),
                reply: Some(reply),
            })
            .await
            .map_err(|_| CommandError::Stopped)?;
        rx.await.map_err(|_| CommandError::Stopped)?
    }

    /// Applique une cible sans attendre de réponse (scènes, timers).
    pub async fn apply(&self, entity_id: &str, target: Target, source: &str) {
        let _ = self
            .tx
            .send(Msg::Command {
                entity_id: entity_id.to_string(),
                request: CommandRequest::Target(target),
                source: source.to_string(),
                reply: None,
            })
            .await;
    }

    pub async fn update_meta(
        &self,
        entity_id: &str,
        patch: MetaPatch,
    ) -> Result<EntityView, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::UpdateMeta {
                entity_id: entity_id.to_string(),
                patch,
                reply,
            })
            .await
            .map_err(|_| CommandError::Stopped)?;
        rx.await.map_err(|_| CommandError::Stopped)?
    }

    /// Renomme une pièce (ou la vide si `to` est `None`) pour toutes ses entités.
    pub async fn rename_room(&self, from: &str, to: Option<String>) -> usize {
        let (reply, rx) = oneshot::channel();
        if self
            .tx
            .send(Msg::RenameRoom {
                from: from.to_string(),
                to,
                reply,
            })
            .await
            .is_err()
        {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    pub async fn remove(&self, entity_id: &str) {
        let _ = self.tx.send(Msg::Remove(entity_id.to_string())).await;
    }

    pub async fn snapshot(&self) -> Vec<EntityView> {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Msg::Snapshot(reply)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn get(&self, entity_id: &str) -> Option<EntityView> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::Get(entity_id.to_string(), reply))
            .await
            .ok()?;
        rx.await.ok().flatten()
    }
}

struct Entity {
    meta: EntityMeta,
    state: Option<State>,
    desired: Option<Desired>,
}

impl Entity {
    fn view(&self) -> EntityView {
        EntityView {
            meta: self.meta.clone(),
            state: self.state.clone(),
            desired: self.desired.clone(),
        }
    }
}

struct Registry {
    entities: HashMap<EntityId, Entity>,
    bus: Bus,
    store: StoreHandle,
}

pub fn spawn(
    initial: Vec<(EntityMeta, Option<State>)>,
    bus: Bus,
    store: StoreHandle,
) -> RegistryHandle {
    let mut entities = HashMap::with_capacity(initial.len());
    for (meta, state) in initial {
        // Un appareil n'est disponible qu'après avoir donné signe de vie depuis le démarrage.
        let state = state.map(|mut s| {
            if meta.domain.physical()
                || matches!(
                    meta.domain,
                    crate::model::Domain::Sensor | crate::model::Domain::BinarySensor
                )
            {
                s.available = false;
            }
            s
        });
        entities.insert(
            meta.entity_id.clone(),
            Entity {
                meta,
                state,
                desired: None,
            },
        );
    }
    let (tx, rx) = mpsc::channel(512);
    let registry = Registry {
        entities,
        bus,
        store,
    };
    tokio::spawn(registry.run(rx));
    RegistryHandle { tx }
}

impl Registry {
    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        sweep.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg),
                    None => break,
                },
                _ = sweep.tick() => self.sweep_desired(),
            }
        }
        tracing::debug!("registre arrêté");
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Upsert(meta) => self.upsert(meta),
            Msg::Report {
                entity_id,
                value,
                attributes,
                available,
            } => self.report(entity_id, Some(value), attributes, available, true),
            Msg::Availability {
                entity_id,
                available,
            } => self.report(entity_id, None, Attributes::new(), available, false),
            Msg::Seen {
                entity_id,
                attributes,
            } => self.report(entity_id, None, attributes, true, true),
            Msg::Command {
                entity_id,
                request,
                source,
                reply,
            } => {
                let result = self.command(&entity_id, request, source);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                } else if let Err(e) = result {
                    tracing::warn!("commande ignorée : {e}");
                }
            }
            Msg::UpdateMeta {
                entity_id,
                patch,
                reply,
            } => {
                let _ = reply.send(self.update_meta(&entity_id, patch));
            }
            Msg::Remove(id) => {
                if self.entities.remove(&id).is_some() {
                    self.store.delete_entity(id.clone());
                    self.bus.publish(Event::EntityRemoved { entity_id: id });
                }
            }
            Msg::RenameRoom { from, to, reply } => {
                let to = to.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
                let mut count = 0;
                for entity in self.entities.values_mut() {
                    if entity.meta.room.as_deref() == Some(from.as_str()) {
                        entity.meta.room = to.clone();
                        entity.meta.user_edited = true;
                        count += 1;
                        let view = entity.view();
                        self.store.save_meta(view.meta.clone());
                        self.bus.publish(Event::EntityUpdated {
                            entity: Arc::new(view),
                        });
                    }
                }
                let _ = reply.send(count);
            }
            Msg::Snapshot(reply) => {
                let mut all: Vec<EntityView> = self.entities.values().map(Entity::view).collect();
                all.sort_by(|a, b| a.meta.entity_id.cmp(&b.meta.entity_id));
                let _ = reply.send(all);
            }
            Msg::Get(id, reply) => {
                let _ = reply.send(self.entities.get(&id).map(Entity::view));
            }
        }
    }

    fn upsert(&mut self, mut meta: EntityMeta) {
        let changed = match self.entities.get_mut(&meta.entity_id) {
            Some(existing) => {
                // Masquer est toujours un choix de l'utilisateur : l'intégration ne le défait jamais.
                meta.hidden = existing.meta.hidden;
                if existing.meta.user_edited {
                    meta.name = existing.meta.name.clone();
                    meta.room = existing.meta.room.clone();
                    meta.plan = existing.meta.plan;
                    meta.user_edited = true;
                }
                if existing.meta == meta {
                    false
                } else {
                    existing.meta = meta.clone();
                    true
                }
            }
            None => {
                self.entities.insert(
                    meta.entity_id.clone(),
                    Entity {
                        meta: meta.clone(),
                        state: None,
                        desired: None,
                    },
                );
                true
            }
        };
        if changed {
            self.store.save_meta(meta.clone());
            if let Some(e) = self.entities.get(&meta.entity_id) {
                self.bus.publish(Event::EntityUpdated {
                    entity: Arc::new(e.view()),
                });
            }
        }
    }

    fn report(
        &mut self,
        id: EntityId,
        value: Option<Value>,
        attributes: Attributes,
        available: bool,
        seen: bool,
    ) {
        let Some(entity) = self.entities.get_mut(&id) else {
            tracing::debug!("état reçu pour une entité inconnue : {id}");
            return;
        };
        let now = Utc::now();
        let old = entity.state.clone();
        let mut new = old
            .clone()
            .unwrap_or_else(|| State::new(Value::Null, Attributes::new(), available));

        // Pile changée : l'ancien niveau, que Zigbee2MQTT renvoie de mémoire, est ignoré
        // jusqu'à ce que l'appareil donne un vrai nouveau niveau.
        let mut value = value;
        let mut attributes = attributes;
        if let Some(stale) = new.attributes.get(STALE_VALUE).cloned() {
            match &value {
                Some(v) if *v == stale => value = None,
                Some(v) if !v.is_null() => {
                    new.attributes.remove(STALE_VALUE);
                    new.attributes.remove(STALE_SINCE);
                    attributes.remove(STALE_VALUE);
                    attributes.remove(STALE_SINCE);
                }
                _ => {}
            }
        }
        let value_changed = matches!(&value, Some(v) if *v != new.value);
        let mut significant = old.is_none() || value_changed || new.available != available;
        for (key, val) in attributes {
            if new.attributes.get(&key) != Some(&val) {
                if !NOISY_ATTRIBUTES.contains(&key.as_str()) {
                    significant = true;
                }
                new.attributes.insert(key, val);
            }
        }
        if let Some(v) = value {
            new.value = v;
        }
        new.available = available;
        if seen {
            new.last_seen = Some(now);
        }

        if !significant {
            // On garde l'info en mémoire, sans réveiller tout le monde ni écrire sur la carte SD.
            entity.state = Some(new);
            return;
        }
        if value_changed || old.is_none() {
            new.last_changed = now;
        }
        new.last_updated = now;

        if entity
            .desired
            .as_ref()
            .is_some_and(|d| d.target.matches(&new))
        {
            entity.desired = None;
        }
        entity.state = Some(new.clone());
        self.store.save_state(id.clone(), new.clone());
        self.bus.publish(Event::StateChanged {
            entity_id: id,
            old,
            new,
        });
    }

    fn command(
        &mut self,
        id: &str,
        request: CommandRequest,
        source: String,
    ) -> Result<Target, CommandError> {
        let entity = self
            .entities
            .get_mut(id)
            .ok_or_else(|| CommandError::NotFound(id.to_string()))?;
        if !entity.meta.domain.controllable() {
            return Err(CommandError::NotControllable(id.to_string()));
        }
        let target = match request {
            CommandRequest::Target(t) => t,
            CommandRequest::Action(Action::TurnOn {
                brightness,
                color_temp,
            }) => Target {
                on: true,
                brightness,
                color_temp,
            },
            CommandRequest::Action(Action::TurnOff) => Target::off(),
            CommandRequest::Action(Action::Toggle) => {
                if entity.state.as_ref().is_some_and(State::is_on) {
                    Target::off()
                } else {
                    Target::on()
                }
            }
        };
        if entity.meta.domain.physical() {
            entity.desired = Some(Desired {
                target: target.clone(),
                since: Utc::now(),
            });
        }
        tracing::debug!("commande {id} -> {target:?} (source : {source})");
        self.bus.publish(Event::CommandRequested {
            integration: entity.meta.integration.clone(),
            entity_id: id.to_string(),
            target: target.clone(),
            source,
        });
        Ok(target)
    }

    fn update_meta(&mut self, id: &str, patch: MetaPatch) -> Result<EntityView, CommandError> {
        let entity = self
            .entities
            .get_mut(id)
            .ok_or_else(|| CommandError::NotFound(id.to_string()))?;
        if let Some(name) = patch.name {
            let name = name.trim();
            if !name.is_empty() {
                entity.meta.name = name.to_string();
            }
        }
        if let Some(room) = patch.room {
            entity.meta.room = room.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        }
        if let Some(plan) = patch.plan {
            entity.meta.plan = plan;
        }
        if let Some(hidden) = patch.hidden {
            entity.meta.hidden = hidden;
        }
        entity.meta.user_edited = true;
        let view = entity.view();
        self.store.save_meta(view.meta.clone());
        self.bus.publish(Event::EntityUpdated {
            entity: Arc::new(view.clone()),
        });
        Ok(view)
    }

    fn sweep_desired(&mut self) {
        let now = Utc::now();
        for (id, entity) in self.entities.iter_mut() {
            let expired = entity
                .desired
                .as_ref()
                .is_some_and(|d| now - d.since > COMMAND_TIMEOUT);
            if expired && let Some(d) = entity.desired.take() {
                tracing::warn!("{id} n'a pas confirmé la commande {:?}", d.target);
                self.bus.publish(Event::CommandTimeout {
                    entity_id: id.clone(),
                    target: d.target,
                });
            }
        }
    }
}

/// Distingue « champ absent » de « champ à null » dans un PATCH JSON.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(de).map(Some)
    }
}
