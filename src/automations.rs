//! Automatismes : « Quand… Si… Alors… ».
//!
//! - **Quand** (déclencheurs) : un appareil change d'état (éventuellement depuis un certain
//!   temps), une valeur franchit un seuil, une heure, le lever ou le coucher du soleil.
//! - **Si** (conditions, toutes vraies) : plage horaire, jours, nuit/jour, état d'un appareil.
//! - **Alors** (actions, dans l'ordre) : commander, allumer pour X min, scène, notifier, attendre.
//!
//! Un automatisme relancé alors qu'il tourne encore (une attente en cours) repart de zéro :
//! c'est le comportement attendu pour « mouvement → allumer, attendre, éteindre ».
//!
//! **État** : chaque automatisme porte un vrai/faux, publié comme l'entité
//! `automation.<id>`. Au repos il vaut `rest_state` (faux par défaut) ; quand ses actions
//! s'exécutent il passe à l'inverse, et y reste jusqu'au **réarmement** : appui long sur son
//! bouton, action « Réarmer » d'un autre automatisme, ou ses propres règles (un déclencheur,
//! « à minuit », « au bout de 12 h »). C'est le « courrier arrivé » de la boîte aux lettres.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::bus::{Bus, Event};
use crate::model::{Action, Attributes, Category, Domain, EntityId, EntityMeta, State, slugify};
use crate::notify::{Level, Notifier, NotifyRequest};
use crate::registry::RegistryHandle;
use crate::settings::{self, Location, Settings};
use crate::store::{StoreHandle, Table};
use crate::sun;
use crate::timers::TimerHandle;

const MAX_DELAY_S: u64 = 24 * 3600;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SunEvent {
    Sunrise,
    Sunset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Un appareil passe à un état (« on », « off »...), éventuellement depuis `for_s` secondes.
    State {
        entity_id: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        for_s: Option<u64>,
    },
    /// Une valeur passe au-dessus ou en dessous d'un seuil.
    Numeric {
        entity_id: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        above: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        below: Option<f64>,
    },
    /// À une heure donnée (« 07:30 »), certains jours (1 = lundi … 7 = dimanche ; vide = tous).
    Time {
        at: String,
        #[serde(default)]
        days: Vec<u8>,
    },
    /// Au lever ou au coucher du soleil, avec un décalage en minutes.
    Sun {
        event: SunEvent,
        #[serde(default)]
        offset_min: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    State {
        entity_id: EntityId,
        is: Value,
    },
    /// Entre deux heures ; « 22:00 » → « 06:00 » passe minuit.
    Time {
        after: String,
        before: String,
    },
    Days {
        days: Vec<u8>,
    },
    Dark,
    Daylight,
}

/// Ce qu'une action peut faire à un automatisme (le sien ou un autre).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOp {
    /// Remettre l'état au repos.
    Rearm,
    Enable,
    Disable,
    /// Lancer ses actions tout de suite.
    Run,
}

/// Quand l'état revient-il au repos tout seul ?
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rearm {
    /// « La trappe arrière de la boîte s'ouvre », « à minuit »…
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// …à condition que (toutes vraies).
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Au bout de tant de minutes après le déclenchement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_min: Option<u64>,
}

/// Dernière fois qu'un déclencheur a réagi : lancé, ou bloqué par une condition (et laquelle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub at: DateTime<Utc>,
    pub ran: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    /// « turn_on », « turn_off » ou « toggle ».
    Command {
        entity_id: EntityId,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brightness: Option<u8>,
    },
    TurnOnFor {
        entity_id: EntityId,
        minutes: u64,
    },
    Scene {
        scene_id: String,
        #[serde(default = "yes")]
        on: bool,
    },
    Notify {
        message: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        to: Vec<String>,
        #[serde(default)]
        except: Vec<String>,
    },
    Delay {
        seconds: u64,
    },
    /// Réarmer, activer, désactiver ou lancer un automatisme.
    Automation {
        automation_id: String,
        op: AutomationOp,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub triggers: Vec<Trigger>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// État actuel (vrai / faux).
    #[serde(default)]
    pub state: bool,
    /// État au repos, celui du réarmement.
    #[serde(default)]
    pub rest_state: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub rearm: Rearm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<Check>,
}

impl Automation {
    pub fn entity_id(&self) -> String {
        format!("automation.{}", self.id)
    }

    /// Déclenché = pas au repos.
    pub fn fired(&self) -> bool {
        self.state != self.rest_state
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationInput {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub triggers: Vec<Trigger>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(default)]
    pub rest_state: bool,
    #[serde(default)]
    pub rearm: Rearm,
}

// ---- Règles pures, testées sans horloge ni appareil ---------------------------------

pub fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn day_ok(days: &[u8], weekday: u8) -> bool {
    days.is_empty() || days.contains(&weekday)
}

/// Vrai si `now` (heure locale, à la minute) correspond à l'heure et aux jours demandés.
pub fn time_matches(at: &str, days: &[u8], now: NaiveTime, weekday: u8) -> bool {
    parse_hm(at).is_some_and(|t| t.hour() == now.hour() && t.minute() == now.minute())
        && day_ok(days, weekday)
}

/// Plage horaire, y compris quand elle passe minuit.
pub fn in_range(after: &str, before: &str, now: NaiveTime) -> bool {
    let (Some(a), Some(b)) = (parse_hm(after), parse_hm(before)) else {
        return false;
    };
    if a <= b {
        now >= a && now < b
    } else {
        now >= a || now < b
    }
}

fn numeric_inside(v: f64, above: Option<f64>, below: Option<f64>) -> bool {
    above.is_none_or(|a| v > a) && below.is_none_or(|b| v < b)
}

#[derive(Debug, PartialEq)]
pub enum Hit {
    No,
    Now,
    /// Il faudra vérifier que l'état tient encore dans `n` secondes.
    After(u64),
    /// L'état attendu a cessé : annuler une attente en cours.
    Cancel,
}

/// Que fait ce déclencheur face à ce changement d'état ?
pub fn on_state_change(
    trigger: &Trigger,
    entity_id: &str,
    old: Option<&State>,
    new: &State,
) -> Hit {
    match trigger {
        Trigger::State {
            entity_id: id,
            to,
            for_s,
        } if id == entity_id => {
            let value_changed = old.is_none_or(|o| o.value != new.value);
            let matches = to.as_ref().is_none_or(|t| *t == new.value);
            match (matches, value_changed, for_s) {
                (true, true, Some(s)) if *s > 0 => Hit::After(*s),
                (true, true, _) => Hit::Now,
                (false, _, Some(_)) => Hit::Cancel,
                _ => Hit::No,
            }
        }
        Trigger::Numeric {
            entity_id: id,
            above,
            below,
        } if id == entity_id => {
            let Some(v) = new.value.as_f64() else {
                return Hit::No;
            };
            let was = old
                .and_then(|o| o.value.as_f64())
                .is_some_and(|o| numeric_inside(o, *above, *below));
            if numeric_inside(v, *above, *below) && !was {
                Hit::Now
            } else {
                Hit::No
            }
        }
        _ => Hit::No,
    }
}

pub fn conditions_ok(
    conditions: &[Condition],
    states: &HashMap<EntityId, State>,
    now_local: DateTime<Local>,
    location: Location,
) -> bool {
    first_failing(conditions, states, now_local, location, &HashMap::new()).is_none()
}

/// Mot lisible pour une valeur d'état : « allumé », « ouvert », « vrai »…
fn value_word(id: &str, v: &Value) -> String {
    let domain = id.split('.').next().unwrap_or("");
    match (domain, v.as_str()) {
        ("automation", Some("on")) => "vrai".into(),
        ("automation", Some("off")) => "faux".into(),
        ("binary_sensor", Some("on")) => "actif".into(),
        ("binary_sensor", Some("off")) => "inactif".into(),
        ("person", Some("home")) => "à la maison".into(),
        ("person", Some("away")) => "absent".into(),
        (_, Some("on")) => "allumé".into(),
        (_, Some("off")) => "éteint".into(),
        (_, Some(s)) => s.to_string(),
        _ => v.to_string(),
    }
}

/// La première condition fausse, expliquée en français (« il ne faisait pas nuit »).
pub fn first_failing(
    conditions: &[Condition],
    states: &HashMap<EntityId, State>,
    now_local: DateTime<Local>,
    location: Location,
    names: &HashMap<EntityId, String>,
) -> Option<String> {
    let now_utc = now_local.with_timezone(&Utc);
    conditions.iter().find_map(|c| {
        let ok = match c {
            Condition::State { entity_id, is } => {
                states.get(entity_id).is_some_and(|s| s.value == *is)
            }
            Condition::Time { after, before } => in_range(after, before, now_local.time()),
            Condition::Days { days } => {
                day_ok(days, now_local.weekday().number_from_monday() as u8)
            }
            Condition::Dark => sun::is_dark(now_utc, location.latitude, location.longitude),
            Condition::Daylight => !sun::is_dark(now_utc, location.latitude, location.longitude),
        };
        if ok {
            return None;
        }
        Some(match c {
            Condition::State { entity_id, is } => {
                let name = names
                    .get(entity_id)
                    .cloned()
                    .unwrap_or_else(|| entity_id.clone());
                match states.get(entity_id) {
                    Some(s) => format!(
                        "« {name} » était {} et non {}",
                        value_word(entity_id, &s.value),
                        value_word(entity_id, is)
                    ),
                    None => format!("« {name} » n'a pas encore donné son état"),
                }
            }
            Condition::Time { after, before } => format!("hors de la plage {after} – {before}"),
            Condition::Days { .. } => "pas un des jours choisis".into(),
            Condition::Dark => "il ne faisait pas nuit".into(),
            Condition::Daylight => "il faisait nuit".into(),
        })
    })
}

fn sun_minute(
    event: SunEvent,
    offset_min: i32,
    now: DateTime<Local>,
    loc: Location,
) -> Option<(u32, u32)> {
    let (rise, set) = sun::sun_times(now.date_naive(), loc.latitude, loc.longitude)?;
    let base = if event == SunEvent::Sunrise {
        rise
    } else {
        set
    };
    let t = (base + TimeDelta::minutes(offset_min as i64)).with_timezone(&Local);
    Some((t.hour(), t.minute()))
}

fn validate(input: &AutomationInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("l'automatisme doit avoir un nom".into());
    }
    if input.triggers.is_empty() {
        return Err("ajoutez au moins un « Quand »".into());
    }
    if input.actions.is_empty() {
        return Err("ajoutez au moins un « Alors »".into());
    }
    let entity = |id: &str| {
        if id.contains('.') {
            Ok(())
        } else {
            Err(format!("appareil invalide : {id}"))
        }
    };
    for t in &input.triggers {
        match t {
            Trigger::State { entity_id, .. } => entity(entity_id)?,
            Trigger::Numeric {
                entity_id,
                above,
                below,
            } => {
                entity(entity_id)?;
                if above.is_none() && below.is_none() {
                    return Err(
                        "un seuil doit préciser « au-dessus de » ou « en dessous de »".into(),
                    );
                }
            }
            Trigger::Time { at, .. } => {
                parse_hm(at).ok_or_else(|| format!("heure invalide : {at} (format 07:30)"))?;
            }
            Trigger::Sun { offset_min, .. } => {
                if offset_min.abs() > 6 * 60 {
                    return Err("décalage trop grand (6 h au plus)".into());
                }
            }
        }
    }
    for t in &input.rearm.triggers {
        if let Trigger::State { entity_id, .. } | Trigger::Numeric { entity_id, .. } = t {
            entity(entity_id)?;
        }
        if let Trigger::Time { at, .. } = t {
            parse_hm(at).ok_or_else(|| format!("heure de réarmement invalide : {at}"))?;
        }
    }
    if input
        .rearm
        .after_min
        .is_some_and(|m| m == 0 || m > 31 * 24 * 60)
    {
        return Err("délai de réarmement invalide".into());
    }
    for c in input.conditions.iter().chain(&input.rearm.conditions) {
        match c {
            Condition::Time { after, before } => {
                parse_hm(after).ok_or_else(|| format!("heure invalide : {after}"))?;
                parse_hm(before).ok_or_else(|| format!("heure invalide : {before}"))?;
            }
            Condition::State { entity_id, .. } => entity(entity_id)?,
            _ => {}
        }
    }
    for a in &input.actions {
        match a {
            Step::Command {
                entity_id, action, ..
            } => {
                entity(entity_id)?;
                if !["turn_on", "turn_off", "toggle"].contains(&action.as_str()) {
                    return Err(format!("action inconnue : {action}"));
                }
            }
            Step::TurnOnFor { entity_id, minutes } => {
                entity(entity_id)?;
                if *minutes == 0 || *minutes > 7 * 24 * 60 {
                    return Err("durée invalide".into());
                }
            }
            Step::Delay { seconds } if *seconds > MAX_DELAY_S => {
                return Err("attente trop longue (24 h au plus)".into());
            }
            Step::Notify { message, .. } if message.trim().is_empty() => {
                return Err("le message est vide".into());
            }
            Step::Automation { automation_id, .. } if automation_id.trim().is_empty() => {
                return Err("choisissez l'automatisme".into());
            }
            _ => {}
        }
    }
    Ok(())
}

// ---- Moteur ----------------------------------------------------------------------

pub const INTEGRATION: &str = "automations";
/// Les déclencheurs de réarmement partagent la table des attentes « depuis X min ».
const REARM_BASE: usize = 10_000;

#[derive(Clone)]
pub struct Deps {
    pub registry: RegistryHandle,
    pub timers: TimerHandle,
    pub notify: Arc<Notifier>,
    pub settings: Arc<Settings>,
    pub bus: Bus,
    pub store: StoreHandle,
}

enum Msg {
    List(oneshot::Sender<Vec<Automation>>),
    Upsert(
        String,
        AutomationInput,
        oneshot::Sender<Result<Automation, String>>,
    ),
    Delete(String, oneshot::Sender<bool>),
    Run(String, oneshot::Sender<Result<(), String>>),
    /// Activer / désactiver, réarmer, forcer l'état (bouton du tableau de bord, autre automatisme).
    Op {
        id: String,
        op: OpKind,
        source: String,
        reply: Option<oneshot::Sender<Result<Automation, String>>>,
    },
    /// Fin d'une attente « depuis X minutes » : on vérifie que l'état tient toujours.
    Recheck {
        id: String,
        trigger: usize,
    },
    Finished {
        id: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpKind {
    Enabled(bool),
    /// `None` : réarmer (retour au repos).
    State(Option<bool>),
    Run,
}

#[derive(Clone)]
pub struct AutomationHandle {
    tx: mpsc::Sender<Msg>,
}

const STOPPED: &str = "moteur d'automatismes arrêté";

impl AutomationHandle {
    pub async fn list(&self) -> Vec<Automation> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::List(tx)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn upsert(&self, id: &str, input: AutomationInput) -> Result<Automation, String> {
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

    /// Lance les actions tout de suite, sans regarder les conditions (bouton « Essayer »).
    pub async fn run(&self, id: &str) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Run(id.to_string(), tx))
            .await
            .map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }

    /// Activer/désactiver, réarmer ou forcer l'état.
    pub async fn op(&self, id: &str, op: OpKind, source: &str) -> Result<Automation, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Op {
                id: id.to_string(),
                op,
                source: source.to_string(),
                reply: Some(tx),
            })
            .await
            .map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }
}

struct Engine {
    automations: BTreeMap<String, Automation>,
    states: HashMap<EntityId, State>,
    names: HashMap<EntityId, String>,
    waiting: HashMap<(String, usize), JoinHandle<()>>,
    running: HashMap<String, JoinHandle<()>>,
    deps: Deps,
    tx: mpsc::Sender<Msg>,
    publish: mpsc::UnboundedSender<Automation>,
    last_minute: Option<(u32, u32)>,
}

pub fn spawn(automations: Vec<Automation>, deps: Deps) -> AutomationHandle {
    let (tx, rx) = mpsc::channel(64);
    let bus_rx = deps.bus.subscribe();
    // Un seul publieur, dans l'ordre : deux changements rapprochés ne s'inversent jamais.
    let (publish, mut pub_rx) = mpsc::unbounded_channel::<Automation>();
    let registry = deps.registry.clone();
    tokio::spawn(async move {
        while let Some(a) = pub_rx.recv().await {
            declare(&registry, &a).await;
        }
    });
    let engine = Engine {
        automations: automations.into_iter().map(|a| (a.id.clone(), a)).collect(),
        states: HashMap::new(),
        names: HashMap::new(),
        waiting: HashMap::new(),
        running: HashMap::new(),
        deps,
        tx: tx.clone(),
        publish,
        last_minute: None,
    };
    tokio::spawn(engine.run(rx, bus_rx));
    AutomationHandle { tx }
}

/// L'entité `automation.<id>` : sa valeur est l'état (on = vrai), ses attributs le reste.
async fn declare(registry: &RegistryHandle, a: &Automation) {
    let mut capabilities = Attributes::new();
    capabilities.insert("rest_state".into(), json!(a.rest_state));
    registry
        .upsert(EntityMeta {
            entity_id: a.entity_id(),
            domain: Domain::Automation,
            name: a.name.clone(),
            integration: INTEGRATION.into(),
            device_id: None,
            room: None,
            plan: None,
            capabilities,
            user_edited: false,
            category: Category::Primary,
            hidden: false,
        })
        .await;
    let mut attrs = Attributes::new();
    attrs.insert("enabled".into(), json!(a.enabled));
    attrs.insert("fired".into(), json!(a.fired()));
    attrs.insert("rest_state".into(), json!(a.rest_state));
    attrs.insert("fired_at".into(), json!(a.fired_at));
    attrs.insert("last_run".into(), json!(a.last_run));
    attrs.insert("last_error".into(), json!(a.last_error));
    attrs.insert("last_check".into(), json!(a.last_check));
    registry
        .report(
            a.entity_id(),
            json!(if a.state { "on" } else { "off" }),
            attrs,
            true,
        )
        .await;
}

fn until_next_minute() -> Duration {
    let now = Local::now();
    let elapsed = now.second() as u64 * 1000 + now.timestamp_subsec_millis() as u64;
    Duration::from_millis(60_000 - elapsed.min(59_999) + 50)
}

fn trigger_at_minute(t: &Trigger, now: DateTime<Local>, loc: Location) -> bool {
    let weekday = now.weekday().number_from_monday() as u8;
    match t {
        Trigger::Time { at, days } => time_matches(at, days, now.time(), weekday),
        Trigger::Sun { event, offset_min } => {
            sun_minute(*event, *offset_min, now, loc) == Some((now.hour(), now.minute()))
        }
        _ => false,
    }
}

impl Engine {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Msg>,
        mut bus_rx: tokio::sync::broadcast::Receiver<Event>,
    ) {
        self.resync().await;
        for a in self.automations.values() {
            let _ = self.publish.send(a.clone());
        }
        // Échéance fixe : un flot continu d'événements ne doit pas repousser le top de la minute.
        let mut next_minute = tokio::time::Instant::now() + until_next_minute();
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => break,
                },
                ev = bus_rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, old, new }) => {
                        self.states.insert(entity_id.clone(), new.clone());
                        self.on_state(&entity_id, old.as_ref(), &new);
                    }
                    Ok(Event::EntityUpdated { entity }) => {
                        self.names.insert(entity.meta.entity_id.clone(), entity.meta.name.clone());
                    }
                    Ok(Event::CommandRequested { integration, entity_id, target, source }) if integration == INTEGRATION => {
                        if let Some(id) = entity_id.strip_prefix("automation.") {
                            let _ = self.apply_op(id, OpKind::State(Some(target.on)), &source);
                        }
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(_)) => self.resync().await,
                    Err(RecvError::Closed) => break,
                },
                _ = tokio::time::sleep_until(next_minute) => {
                    self.on_minute();
                    next_minute = tokio::time::Instant::now() + until_next_minute();
                }
            }
        }
    }

    async fn resync(&mut self) {
        let all = self.deps.registry.snapshot().await;
        self.names = all
            .iter()
            .map(|e| (e.meta.entity_id.clone(), e.meta.name.clone()))
            .collect();
        self.states = all
            .into_iter()
            .filter_map(|e| e.state.map(|s| (e.meta.entity_id, s)))
            .collect();
    }

    fn location(&self) -> Location {
        self.deps.settings.get_or_default(settings::LOCATION)
    }

    fn wait(&mut self, key: (String, usize), secs: u64) {
        if let Some(h) = self.waiting.remove(&key) {
            h.abort();
        }
        let tx = self.tx.clone();
        let (id, trigger) = key.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let _ = tx.send(Msg::Recheck { id, trigger }).await;
        });
        self.waiting.insert(key, handle);
    }

    fn on_state(&mut self, entity_id: &str, old: Option<&State>, new: &State) {
        let mut fire = Vec::new();
        let mut rearm = Vec::new();
        let mut waits = Vec::new();
        let mut cancels = Vec::new();
        for a in self.automations.values() {
            let lists = [
                (0, if a.enabled { &a.triggers[..] } else { &[][..] }),
                (
                    REARM_BASE,
                    if a.fired() {
                        &a.rearm.triggers[..]
                    } else {
                        &[][..]
                    },
                ),
            ];
            for (base, triggers) in lists {
                for (i, t) in triggers.iter().enumerate() {
                    let key = (a.id.clone(), base + i);
                    match on_state_change(t, entity_id, old, new) {
                        Hit::Now if base == 0 => fire.push(a.id.clone()),
                        Hit::Now => rearm.push(a.id.clone()),
                        Hit::After(secs) => waits.push((key, secs)),
                        Hit::Cancel => cancels.push(key),
                        Hit::No => {}
                    }
                }
            }
        }
        for (key, secs) in waits {
            self.wait(key, secs);
        }
        for key in cancels {
            if let Some(h) = self.waiting.remove(&key) {
                h.abort();
            }
        }
        for id in fire {
            self.trigger(&id, false);
        }
        for id in rearm {
            self.rearm_checked(&id);
        }
    }

    fn on_minute(&mut self) {
        let now = Local::now();
        let minute = (now.hour(), now.minute());
        if self.last_minute == Some(minute) {
            return;
        }
        self.last_minute = Some(minute);
        let loc = self.location();
        let fire: Vec<String> = self
            .automations
            .values()
            .filter(|a| a.enabled)
            .filter(|a| a.triggers.iter().any(|t| trigger_at_minute(t, now, loc)))
            .map(|a| a.id.clone())
            .collect();
        let utc = Utc::now();
        let rearm: Vec<String> = self
            .automations
            .values()
            .filter(|a| a.fired())
            .filter(|a| {
                a.rearm
                    .triggers
                    .iter()
                    .any(|t| trigger_at_minute(t, now, loc))
                    || a.rearm
                        .after_min
                        .zip(a.fired_at)
                        .is_some_and(|(m, at)| utc >= at + TimeDelta::minutes(m as i64))
            })
            .map(|a| a.id.clone())
            .collect();
        for id in fire {
            self.trigger(&id, false);
        }
        for id in rearm {
            self.rearm_checked(&id);
        }
    }

    /// Réarmement par ses propres règles : seulement si ses conditions sont vraies.
    fn rearm_checked(&mut self, id: &str) {
        let Some(a) = self.automations.get(id) else {
            return;
        };
        if !a.fired()
            || !conditions_ok(
                &a.rearm.conditions,
                &self.states,
                Local::now(),
                self.location(),
            )
        {
            return;
        }
        let _ = self.apply_op(id, OpKind::State(None), "réarmement automatique");
    }

    fn save(&mut self, id: &str) {
        if let Some(a) = self.automations.get(id) {
            self.deps.store.put(Table::Automations, id, a);
            let _ = self.publish.send(a.clone());
        }
    }

    fn apply_op(&mut self, id: &str, op: OpKind, source: &str) -> Result<Automation, String> {
        if op == OpKind::Run {
            if self.trigger(id, true) {
                return self
                    .automations
                    .get(id)
                    .cloned()
                    .ok_or_else(|| format!("automatisme inconnu : {id}"));
            }
            return Err(format!("automatisme inconnu : {id}"));
        }
        let a = self
            .automations
            .get_mut(id)
            .ok_or_else(|| format!("automatisme inconnu : {id}"))?;
        match op {
            OpKind::Enabled(on) => a.enabled = on,
            OpKind::State(value) => {
                let value = value.unwrap_or(a.rest_state);
                if value != a.state {
                    a.state = value;
                    a.fired_at = if a.fired() { Some(Utc::now()) } else { None };
                    tracing::info!(
                        "automatisme « {} » : état {} ({source})",
                        a.name,
                        if value { "vrai" } else { "faux" }
                    );
                }
            }
            OpKind::Run => unreachable!(),
        }
        if matches!(op, OpKind::Enabled(_)) {
            // Désactivé : plus d'attentes en cours pour ses déclencheurs principaux.
            let id_owned = id.to_string();
            self.waiting.retain(|(a, i), h| {
                let keep = *a != id_owned || *i >= REARM_BASE;
                if !keep {
                    h.abort();
                }
                keep
            });
        }
        self.save(id);
        Ok(self.automations[id].clone())
    }

    /// Vérifie les conditions puis lance les actions dans une tâche à part.
    fn trigger(&mut self, id: &str, force: bool) -> bool {
        let loc = self.location();
        let Some(a) = self.automations.get_mut(id) else {
            return false;
        };
        if !force
            && let Some(reason) =
                first_failing(&a.conditions, &self.states, Local::now(), loc, &self.names)
        {
            tracing::debug!("automatisme {id} : pas lancé, {reason}");
            a.last_check = Some(Check {
                at: Utc::now(),
                ran: false,
                reason: Some(reason.clone()),
            });
            let name = a.name.clone();
            self.save(id);
            self.deps.bus.publish(Event::AutomationSkipped {
                id: id.to_string(),
                name,
                reason,
            });
            return false;
        }
        tracing::info!("automatisme « {} » déclenché", a.name);
        a.last_check = Some(Check {
            at: Utc::now(),
            ran: true,
            reason: None,
        });
        // L'état passe à l'inverse du repos dès le déclenchement (pas à la fin des actions).
        if !a.fired() {
            a.state = !a.rest_state;
            a.fired_at = Some(Utc::now());
        }
        let steps = a.actions.clone();
        let source = format!("automation:{id}");
        self.save(id);
        if let Some(previous) = self.running.remove(id) {
            previous.abort();
        }
        let deps = self.deps.clone();
        let tx = self.tx.clone();
        let id = id.to_string();
        let handle = tokio::spawn({
            let id = id.clone();
            async move {
                let error = execute(&steps, &deps, &tx, &source).await.err();
                let _ = tx.send(Msg::Finished { id, error }).await;
            }
        });
        self.running.insert(id, handle);
        true
    }

    async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::List(reply) => {
                let _ = reply.send(self.automations.values().cloned().collect());
            }
            Msg::Upsert(raw_id, input, reply) => {
                let _ = reply.send(self.upsert(raw_id, input));
            }
            Msg::Delete(id, reply) => {
                let removed = self.automations.remove(&id);
                if let Some(a) = &removed {
                    self.deps.store.delete(Table::Automations, &id);
                    self.deps.registry.remove(&a.entity_id()).await;
                    self.waiting.retain(|(a, _), h| {
                        let keep = *a != id;
                        if !keep {
                            h.abort();
                        }
                        keep
                    });
                }
                let _ = reply.send(removed.is_some());
            }
            Msg::Run(id, reply) => {
                let ok = self.automations.contains_key(&id) && self.trigger(&id, true);
                let _ = reply.send(if ok {
                    Ok(())
                } else {
                    Err(format!("automatisme inconnu : {id}"))
                });
            }
            Msg::Op {
                id,
                op,
                source,
                reply,
            } => {
                let result = self.apply_op(&id, op, &source);
                match reply {
                    Some(r) => {
                        let _ = r.send(result);
                    }
                    None => {
                        if let Err(e) = result {
                            tracing::warn!("{e}");
                        }
                    }
                }
            }
            Msg::Recheck { id, trigger } => {
                self.waiting.remove(&(id.clone(), trigger));
                let (list, index, rearm) = match trigger.checked_sub(REARM_BASE) {
                    Some(i) => (true, i, true),
                    None => (false, trigger, false),
                };
                let still = self
                    .automations
                    .get(&id)
                    .and_then(|a| {
                        if list {
                            a.rearm.triggers.get(index)
                        } else {
                            a.triggers.get(index)
                        }
                    })
                    .is_some_and(|t| match t {
                        Trigger::State { entity_id, to, .. } => to.as_ref().is_none_or(|to| {
                            self.states.get(entity_id).is_some_and(|s| s.value == *to)
                        }),
                        _ => false,
                    });
                if still {
                    if rearm {
                        self.rearm_checked(&id);
                    } else {
                        self.trigger(&id, false);
                    }
                }
            }
            Msg::Finished { id, error } => {
                self.running.remove(&id);
                if let Some(a) = self.automations.get_mut(&id) {
                    a.last_run = Some(Utc::now());
                    a.last_error = error.clone();
                }
                self.save(&id);
                if let Some(e) = &error {
                    tracing::warn!("automatisme {id} : {e}");
                }
                self.deps.bus.publish(Event::AutomationRan {
                    id,
                    ok: error.is_none(),
                    detail: error,
                });
            }
        }
    }

    fn upsert(&mut self, raw_id: String, input: AutomationInput) -> Result<Automation, String> {
        validate(&input)?;
        let name = input.name.trim().to_string();
        let id = slugify(if raw_id.is_empty() { &name } else { &raw_id });
        if id.is_empty() {
            return Err("identifiant invalide".into());
        }
        let previous = self.automations.get(&id);
        // L'état est gardé, sauf si l'état au repos change : on repart du repos.
        let state = match previous {
            Some(p) if p.rest_state == input.rest_state => p.state,
            _ => input.rest_state,
        };
        let fired_at = previous
            .filter(|_| state != input.rest_state)
            .and_then(|p| p.fired_at);
        let automation = Automation {
            id: id.clone(),
            name,
            enabled: input.enabled,
            triggers: input.triggers,
            conditions: input.conditions,
            actions: input.actions,
            last_run: previous.and_then(|p| p.last_run),
            last_error: previous.and_then(|p| p.last_error.clone()),
            state,
            rest_state: input.rest_state,
            fired_at,
            rearm: input.rearm,
            last_check: previous.and_then(|p| p.last_check.clone()),
        };
        // Les attentes en cours de l'ancienne version n'ont plus de sens.
        self.waiting.retain(|(a, _), h| {
            let keep = *a != id;
            if !keep {
                h.abort();
            }
            keep
        });
        self.automations.insert(id.clone(), automation.clone());
        self.save(&id);
        Ok(automation)
    }
}

async fn execute(
    steps: &[Step],
    deps: &Deps,
    engine: &mpsc::Sender<Msg>,
    source: &str,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for step in steps {
        let result: Result<(), String> = match step {
            Step::Command {
                entity_id,
                action,
                brightness,
            } => {
                let action = match action.as_str() {
                    "turn_on" => Action::TurnOn {
                        brightness: *brightness,
                        color_temp: None,
                    },
                    "turn_off" => Action::TurnOff,
                    _ => Action::Toggle,
                };
                deps.registry
                    .command(entity_id, action, source)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Step::TurnOnFor { entity_id, minutes } => deps
                .timers
                .run_for(entity_id, minutes * 60)
                .await
                .map(|_| ()),
            Step::Scene { scene_id, on } => {
                let action = if *on {
                    Action::TurnOn {
                        brightness: None,
                        color_temp: None,
                    }
                } else {
                    Action::TurnOff
                };
                deps.registry
                    .command(&format!("scene.{scene_id}"), action, source)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Step::Notify {
                message,
                title,
                to,
                except,
            } => deps
                .notify
                .send(NotifyRequest {
                    title: title.clone(),
                    message: message.clone(),
                    level: Level::Important,
                    to: to.clone(),
                    except: except.clone(),
                    action: None,
                })
                .map(|_| ())
                .map_err(|e| {
                    if e.contains("aucun destinataire") {
                        "notification non envoyée : aucun téléphone n'a encore ouvert Heim"
                            .to_string()
                    } else {
                        e
                    }
                }),
            Step::Delay { seconds } => {
                tokio::time::sleep(Duration::from_secs(*seconds)).await;
                Ok(())
            }
            Step::Automation { automation_id, op } => {
                let op = match op {
                    AutomationOp::Rearm => OpKind::State(None),
                    AutomationOp::Enable => OpKind::Enabled(true),
                    AutomationOp::Disable => OpKind::Enabled(false),
                    AutomationOp::Run => OpKind::Run,
                };
                // Pas de réponse attendue : le moteur est peut-être en train de nous attendre.
                engine
                    .send(Msg::Op {
                        id: automation_id.clone(),
                        op,
                        source: source.to_string(),
                        reply: None,
                    })
                    .await
                    .map_err(|_| STOPPED.to_string())
            }
        };
        if let Err(e) = result {
            // Une lampe injoignable ne doit pas empêcher la notification qui suit.
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" ; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn st(v: Value) -> State {
        State::new(v, Default::default(), true)
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn state_trigger_with_and_without_delay() {
        let door = Trigger::State {
            entity_id: "binary_sensor.porte".into(),
            to: Some(json!("on")),
            for_s: None,
        };
        assert_eq!(
            on_state_change(
                &door,
                "binary_sensor.porte",
                Some(&st(json!("off"))),
                &st(json!("on"))
            ),
            Hit::Now
        );
        // Même valeur (seul un attribut a bougé) : rien.
        assert_eq!(
            on_state_change(
                &door,
                "binary_sensor.porte",
                Some(&st(json!("on"))),
                &st(json!("on"))
            ),
            Hit::No
        );
        assert_eq!(
            on_state_change(&door, "light.x", Some(&st(json!("off"))), &st(json!("on"))),
            Hit::No
        );

        let open_10min = Trigger::State {
            entity_id: "binary_sensor.porte".into(),
            to: Some(json!("on")),
            for_s: Some(600),
        };
        assert_eq!(
            on_state_change(
                &open_10min,
                "binary_sensor.porte",
                Some(&st(json!("off"))),
                &st(json!("on"))
            ),
            Hit::After(600)
        );
        assert_eq!(
            on_state_change(
                &open_10min,
                "binary_sensor.porte",
                Some(&st(json!("on"))),
                &st(json!("off"))
            ),
            Hit::Cancel
        );
    }

    #[test]
    fn numeric_crossing_only() {
        let hot = Trigger::Numeric {
            entity_id: "sensor.t".into(),
            above: Some(25.0),
            below: None,
        };
        assert_eq!(
            on_state_change(&hot, "sensor.t", Some(&st(json!(24.0))), &st(json!(25.5))),
            Hit::Now
        );
        assert_eq!(
            on_state_change(&hot, "sensor.t", Some(&st(json!(26.0))), &st(json!(27.0))),
            Hit::No,
            "déjà au-dessus"
        );
    }

    #[test]
    fn time_and_ranges() {
        assert!(time_matches("07:30", &[], t(7, 30), 3));
        assert!(
            !time_matches("07:30", &[6, 7], t(7, 30), 3),
            "pas le mercredi"
        );
        assert!(in_range("22:00", "06:00", t(23, 15)));
        assert!(in_range("22:00", "06:00", t(5, 59)));
        assert!(!in_range("22:00", "06:00", t(12, 0)));
        assert!(in_range("08:00", "12:00", t(8, 0)));
    }

    #[test]
    fn json_shape_is_readable() {
        let input: AutomationInput = serde_json::from_value(json!({
            "name": "Entrée la nuit",
            "triggers": [{"type": "state", "entity_id": "binary_sensor.porte", "to": "on"}],
            "conditions": [{"type": "dark"}],
            "actions": [
                {"type": "turn_on_for", "entity_id": "light.entree", "minutes": 5},
                {"type": "notify", "message": "Porte ouverte"}
            ]
        }))
        .unwrap();
        assert!(validate(&input).is_ok());
        assert!(input.enabled);
        let bad: AutomationInput = serde_json::from_value(json!({
            "name": "x", "triggers": [{"type": "time", "at": "25:99"}], "actions": [{"type": "delay", "seconds": 1}]
        }))
        .unwrap();
        assert!(validate(&bad).is_err());
    }
}
