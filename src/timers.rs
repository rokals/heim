//! Timers avec décompte.
//!
//! Un timer stocke une **heure de fin absolue**, jamais un compteur qui décrémente.
//! L'interface calcule le temps restant elle-même (décompte fluide, zéro charge
//! pour le noyau), et le timer survit à un redémarrage ou une coupure de courant.
//! Une seule tâche dort jusqu'à la prochaine échéance, quel que soit le nombre de timers.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};

use crate::bus::{Bus, Event};
use crate::model::{Action, Attributes, Domain, EntityId, EntityMeta, slugify};
use crate::registry::RegistryHandle;
use crate::store::{StoreHandle, Table};

pub const INTEGRATION: &str = "timers";
const MAX_DURATION_S: u64 = 7 * 24 * 3600;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TimerStatus {
    #[default]
    Idle,
    Running {
        ends_at: DateTime<Utc>,
    },
    Paused {
        remaining_ms: u64,
    },
    Finished {
        at: DateTime<Utc>,
    },
}

/// Ce que fait le timer à l'échéance : par exemple éteindre une prise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishAction {
    pub entity_id: EntityId,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// Durée par défaut, en secondes.
    pub duration_s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_finish: Option<FinishAction>,
    #[serde(default)]
    pub status: TimerStatus,
    /// Minuterie créée par « allumer pour X min » sur une carte : invisible dans les listes,
    /// annulée si l'appareil est éteint à la main avant la fin.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
}

impl Timer {
    pub fn entity_id(&self) -> EntityId {
        format!("timer.{}", self.id)
    }

    pub fn remaining_s(&self, now: DateTime<Utc>) -> Option<u64> {
        match &self.status {
            TimerStatus::Running { ends_at } => {
                Some((*ends_at - now).num_milliseconds().max(0) as u64 / 1000)
            }
            TimerStatus::Paused { remaining_ms } => Some(remaining_ms / 1000),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TimerView {
    #[serde(flatten)]
    pub timer: Timer,
    pub remaining_s: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimerInput {
    pub name: String,
    #[serde(default)]
    pub room: Option<String>,
    pub duration_s: u64,
    #[serde(default)]
    pub on_finish: Option<FinishAction>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TimerOp {
    Start {
        #[serde(default)]
        duration_s: Option<u64>,
    },
    Pause,
    Resume,
    Cancel,
}

enum Msg {
    List(oneshot::Sender<Vec<TimerView>>),
    Upsert(
        String,
        TimerInput,
        oneshot::Sender<Result<TimerView, String>>,
    ),
    Delete(String, oneshot::Sender<bool>),
    Control(String, TimerOp, oneshot::Sender<Result<TimerView, String>>),
    RunFor {
        entity_id: EntityId,
        seconds: u64,
        reply: oneshot::Sender<Result<TimerView, String>>,
    },
}

#[derive(Clone)]
pub struct TimerHandle {
    tx: mpsc::Sender<Msg>,
}

const STOPPED: &str = "moteur de timers arrêté";

/// Identifiant de la minuterie automatique d'une entité.
pub fn auto_timer_id(entity_id: &str) -> String {
    format!("auto_{}", slugify(entity_id))
}

impl TimerHandle {
    pub async fn list(&self) -> Vec<TimerView> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::List(tx)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn upsert(&self, id: &str, input: TimerInput) -> Result<TimerView, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Upsert(id.to_string(), input, tx))
            .await
            .map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }

    pub async fn delete(&self, id: &str) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Delete(id.to_string(), tx)).await.is_err() {
            return false;
        }
        rx.await.unwrap_or(false)
    }

    /// Allume un appareil pour une durée, puis l'éteint tout seul.
    pub async fn run_for(&self, entity_id: &str, seconds: u64) -> Result<TimerView, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::RunFor {
                entity_id: entity_id.to_string(),
                seconds,
                reply: tx,
            })
            .await
            .map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }

    pub async fn control(&self, id: &str, op: TimerOp) -> Result<TimerView, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Control(id.to_string(), op, tx))
            .await
            .map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }
}

struct Engine {
    timers: BTreeMap<String, Timer>,
    registry: RegistryHandle,
    store: StoreHandle,
}

pub fn spawn(
    timers: Vec<Timer>,
    registry: RegistryHandle,
    bus: Bus,
    store: StoreHandle,
) -> TimerHandle {
    let (tx, rx) = mpsc::channel(64);
    let bus_rx = bus.subscribe();
    let engine = Engine {
        timers: timers.into_iter().map(|t| (t.id.clone(), t)).collect(),
        registry,
        store,
    };
    tokio::spawn(engine.run(rx, bus_rx));
    TimerHandle { tx }
}

fn validate_duration(d: u64) -> Result<u64, String> {
    if d == 0 || d > MAX_DURATION_S {
        Err(format!("durée invalide : {d} s (entre 1 s et 7 jours)"))
    } else {
        Ok(d)
    }
}

impl Engine {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Msg>,
        mut bus_rx: tokio::sync::broadcast::Receiver<Event>,
    ) {
        let ids: Vec<String> = self.timers.keys().cloned().collect();
        for id in &ids {
            self.declare(id).await;
            self.publish(id).await;
        }
        // Timers arrivés à échéance pendant que Heim était éteint.
        self.finish_due().await;

        loop {
            let next = self.next_deadline();
            let sleep = async move {
                match next {
                    Some(at) => {
                        tokio::time::sleep((at - Utc::now()).to_std().unwrap_or_default()).await
                    }
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => break,
                },
                ev = bus_rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, new, .. }) if new.value == "off" => {
                        // Éteint à la main avant la fin : la minuterie automatique n'a plus lieu d'être.
                        let id = auto_timer_id(&entity_id);
                        if matches!(self.timers.get(&id).map(|t| &t.status), Some(TimerStatus::Running { .. } | TimerStatus::Paused { .. })) {
                            let _ = self.control(&id, TimerOp::Cancel).await;
                        }
                    }
                    Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                        if let Some(id) = entity_id.strip_prefix("timer.") {
                            let op = if target.on { TimerOp::Start { duration_s: None } } else { TimerOp::Cancel };
                            if let Err(e) = self.control(id, op).await {
                                tracing::warn!("timer {id} : {e}");
                            }
                        }
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = sleep => self.finish_due().await,
            }
        }
    }

    fn next_deadline(&self) -> Option<DateTime<Utc>> {
        self.timers
            .values()
            .filter_map(|t| match t.status {
                TimerStatus::Running { ends_at } => Some(ends_at),
                _ => None,
            })
            .min()
    }

    async fn finish_due(&mut self) {
        let now = Utc::now();
        let due: Vec<String> = self
            .timers
            .values()
            .filter(|t| matches!(t.status, TimerStatus::Running { ends_at } if ends_at <= now))
            .map(|t| t.id.clone())
            .collect();
        for id in due {
            let Some(timer) = self.timers.get_mut(&id) else {
                continue;
            };
            timer.status = TimerStatus::Finished { at: now };
            let action = timer.on_finish.clone();
            self.store.put(Table::Timers, &id, timer);
            self.publish(&id).await;
            tracing::info!("timer {id} terminé");
            if let Some(FinishAction { entity_id, action }) = action
                && let Err(e) = self
                    .registry
                    .command(&entity_id, action, &format!("timer:{id}"))
                    .await
            {
                tracing::warn!("timer {id} : action de fin impossible : {e}");
            }
        }
    }

    async fn declare(&self, id: &str) {
        let Some(t) = self.timers.get(id) else { return };
        self.registry
            .upsert(EntityMeta {
                entity_id: t.entity_id(),
                domain: Domain::Timer,
                name: t.name.clone(),
                integration: INTEGRATION.into(),
                device_id: None,
                room: t.room.clone(),
                plan: None,
                capabilities: if t.auto {
                    [("auto".to_string(), json!(true))].into_iter().collect()
                } else {
                    Attributes::new()
                },
                user_edited: false,
                category: crate::model::Category::Primary,
                hidden: t.auto,
            })
            .await;
    }

    async fn publish(&self, id: &str) {
        let Some(t) = self.timers.get(id) else { return };
        let mut attrs = Attributes::new();
        attrs.insert("duration_s".into(), json!(t.duration_s));
        // On écrit toujours les trois clés pour que la fusion d'attributs n'en laisse pas traîner une ancienne.
        attrs.insert("ends_at".into(), json!(null));
        attrs.insert("remaining_s".into(), json!(null));
        attrs.insert("finished_at".into(), json!(null));
        let value = match &t.status {
            TimerStatus::Idle => "idle",
            TimerStatus::Running { ends_at } => {
                attrs.insert("ends_at".into(), json!(ends_at));
                "running"
            }
            TimerStatus::Paused { remaining_ms } => {
                attrs.insert("remaining_s".into(), json!(remaining_ms / 1000));
                "paused"
            }
            TimerStatus::Finished { at } => {
                attrs.insert("finished_at".into(), json!(at));
                "finished"
            }
        };
        if let Some(f) = &t.on_finish {
            attrs.insert("on_finish".into(), json!(f));
        }
        self.registry
            .report(t.entity_id(), json!(value), attrs, true)
            .await;
    }

    fn view(&self, id: &str) -> Option<TimerView> {
        let t = self.timers.get(id)?;
        Some(TimerView {
            timer: t.clone(),
            remaining_s: t.remaining_s(Utc::now()),
        })
    }

    async fn control(&mut self, id: &str, op: TimerOp) -> Result<TimerView, String> {
        let timer = self
            .timers
            .get_mut(id)
            .ok_or_else(|| format!("timer inconnu : {id}"))?;
        let now = Utc::now();
        timer.status = match (op, &timer.status) {
            (TimerOp::Start { duration_s }, _) => {
                let d = validate_duration(duration_s.unwrap_or(timer.duration_s))?;
                TimerStatus::Running {
                    ends_at: now + TimeDelta::seconds(d as i64),
                }
            }
            (TimerOp::Pause, TimerStatus::Running { ends_at }) => TimerStatus::Paused {
                remaining_ms: (*ends_at - now).num_milliseconds().max(0) as u64,
            },
            (TimerOp::Resume, TimerStatus::Paused { remaining_ms }) => TimerStatus::Running {
                ends_at: now + TimeDelta::milliseconds(*remaining_ms as i64),
            },
            (TimerOp::Cancel, _) => TimerStatus::Idle,
            (op, status) => {
                return Err(format!(
                    "impossible de faire {op:?} sur un timer à l'état {status:?}"
                ));
            }
        };
        self.store.put(Table::Timers, id, &self.timers[id]);
        self.publish(id).await;
        Ok(self.view(id).expect("timer présent"))
    }

    async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::List(reply) => {
                let views = self.timers.keys().filter_map(|id| self.view(id)).collect();
                let _ = reply.send(views);
            }
            Msg::Upsert(raw_id, input, reply) => {
                let _ = reply.send(self.upsert(raw_id, input).await);
            }
            Msg::Delete(id, reply) => {
                let existed = self.timers.remove(&id).is_some();
                if existed {
                    self.store.delete(Table::Timers, &id);
                    self.registry.remove(&format!("timer.{id}")).await;
                }
                let _ = reply.send(existed);
            }
            Msg::Control(id, op, reply) => {
                let _ = reply.send(self.control(&id, op).await);
            }
            Msg::RunFor {
                entity_id,
                seconds,
                reply,
            } => {
                let _ = reply.send(self.run_for(entity_id, seconds).await);
            }
        }
    }

    async fn run_for(&mut self, entity_id: EntityId, seconds: u64) -> Result<TimerView, String> {
        let seconds = validate_duration(seconds)?;
        let target = self
            .registry
            .get(&entity_id)
            .await
            .ok_or_else(|| format!("appareil inconnu : {entity_id}"))?;
        let id = auto_timer_id(&entity_id);
        let timer = Timer {
            id: id.clone(),
            name: format!("Arrêt auto de {}", target.meta.name),
            room: target.meta.room.clone(),
            duration_s: seconds,
            on_finish: Some(FinishAction {
                entity_id: entity_id.clone(),
                action: Action::TurnOff,
            }),
            status: TimerStatus::Idle,
            auto: true,
        };
        self.timers.insert(id.clone(), timer);
        self.declare(&id).await;
        self.registry
            .command(
                &entity_id,
                Action::TurnOn {
                    brightness: None,
                    color_temp: None,
                },
                "minuterie",
            )
            .await
            .map_err(|e| e.to_string())?;
        self.control(
            &id,
            TimerOp::Start {
                duration_s: Some(seconds),
            },
        )
        .await
    }

    async fn upsert(&mut self, raw_id: String, input: TimerInput) -> Result<TimerView, String> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err("le timer doit avoir un nom".into());
        }
        let id = slugify(if raw_id.is_empty() { &name } else { &raw_id });
        if id.is_empty() {
            return Err("identifiant de timer invalide".into());
        }
        let duration_s = validate_duration(input.duration_s)?;
        if let Some(f) = &input.on_finish
            && !(f.entity_id.contains('.'))
        {
            return Err(format!("entité d'action invalide : {}", f.entity_id));
        }
        let status = self
            .timers
            .get(&id)
            .map(|t| t.status.clone())
            .unwrap_or_default();
        let timer = Timer {
            id: id.clone(),
            name,
            room: input.room,
            duration_s,
            on_finish: input.on_finish,
            status,
            auto: false,
        };
        self.store.put(Table::Timers, &id, &timer);
        self.timers.insert(id.clone(), timer);
        self.declare(&id).await;
        self.publish(&id).await;
        Ok(self.view(&id).expect("timer présent"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_action_json_is_flat() {
        let f: FinishAction =
            serde_json::from_str(r#"{"entity_id":"switch.prise","action":"turn_off"}"#).unwrap();
        assert_eq!(f.action, Action::TurnOff);
        let back = serde_json::to_value(&f).unwrap();
        assert_eq!(
            back,
            serde_json::json!({"entity_id":"switch.prise","action":"turn_off"})
        );
    }

    #[test]
    fn timer_roundtrip() {
        let t = Timer {
            auto: false,
            id: "cafe".into(),
            name: "Café".into(),
            room: None,
            duration_s: 600,
            on_finish: None,
            status: TimerStatus::Paused { remaining_ms: 4200 },
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Timer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.remaining_s(Utc::now()), Some(4));
    }
}
