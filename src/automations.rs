//! Automatismes : « Quand… Si… Alors… ».
//!
//! - **Quand** (déclencheurs, chacun nommé) : un appareil change d'état (éventuellement depuis
//!   un certain temps), une valeur franchit un seuil, un bouton ou une télécommande envoie une
//!   action (« appui simple sur le bouton 1 »), une heure, toutes les N minutes, le lever ou le
//!   coucher du soleil, quelqu'un arrive ou tout le monde part, l'alarme change, Heim démarre.
//! - **Si** (conditions, toutes vraies) : plage horaire, jours, nuit/jour, état ou mesure d'un
//!   appareil, présence, alarme, « déclenché par tel Quand », et des groupes « au moins une »,
//!   « aucune ».
//! - **Alors** (actions, dans l'ordre) : commander (luminosité, couleur, blanc, fondu), effet
//!   lumineux, allumer pour X min, scène, notifier (avec variables), attendre, attendre que,
//!   alarme, autre automatisme, et des blocs : « selon le déclencheur » (une branche par
//!   Quand : bouton 1 → salon, bouton 2 → cuisine), « si… sinon », « répéter », « arrêter ».
//!
//! **Relance** pendant qu'il tourne encore : recommencer (par défaut, comme « mouvement →
//! allumer, attendre, éteindre »), ignorer, mettre en file, ou en parallèle.
//!
//! **État** : chaque automatisme porte un vrai/faux, publié comme l'entité
//! `automation.<id>`. Au repos il vaut `rest_state` (faux par défaut) ; quand ses actions
//! s'exécutent il passe à l'inverse, et y reste jusqu'au **réarmement** : appui long sur son
//! bouton, action « Réarmer » d'un autre automatisme, ou ses propres règles (un déclencheur,
//! « à minuit », « au bout de 12 h »). C'est le « courrier arrivé » de la boîte aux lettres.
//!
//! **Historique** : les dernières exécutions (déclencheur, branche suivie, chaque action,
//! où et pourquoi ça s'est arrêté) restent en mémoire pour comprendre ce qui s'est passé.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
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
/// Exécutions gardées par automatisme pour l'historique.
const MAX_TRACES: usize = 15;
/// Lignes d'historique par exécution (une boucle ne doit pas remplir la mémoire).
const MAX_TRACE_STEPS: usize = 200;
/// Exécutions en parallèle ou en file, au plus.
const MAX_RUNS: usize = 10;
/// Profondeur des blocs imbriqués (si dans selon dans répéter…).
const MAX_DEPTH: usize = 8;

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SunEvent {
    Sunrise,
    Sunset,
}

/// Présence (téléphones de la maison, entités `person.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceEvent {
    /// Le premier rentre dans une maison vide.
    FirstHome,
    /// Le dernier part : la maison est vide.
    LastAway,
    /// Quelqu'un rentre.
    Arrive,
    /// Quelqu'un part.
    Leave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Un appareil passe à un état (« on », « off »...), éventuellement depuis `for_s` secondes.
    State {
        entity_id: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<Value>,
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
    /// Un appareil envoie une action : bouton, télécommande, cube… (`event` vide : n'importe
    /// laquelle).
    Device {
        device_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },
    /// À une heure donnée (« 07:30 »), certains jours (1 = lundi … 7 = dimanche ; vide = tous).
    Time {
        at: String,
        #[serde(default)]
        days: Vec<u8>,
    },
    /// Toutes les N minutes (calées sur minuit : toutes les 15 min = h00, h15, h30, h45).
    Interval {
        minutes: u32,
    },
    /// Au lever ou au coucher du soleil, avec un décalage en minutes.
    Sun {
        event: SunEvent,
        #[serde(default)]
        offset_min: i32,
    },
    Presence {
        event: PresenceEvent,
    },
    /// L'alarme passe à « disarmed », « arming », « armed », « pending » ou « triggered ».
    Alarm {
        event: String,
    },
    /// Heim vient de démarrer (après une coupure, une mise à jour).
    Startup,
}

/// Un « Quand » : son identifiant (pour « selon le déclencheur »), son nom, sa règle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct When {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub trigger: Trigger,
}

impl From<Trigger> for When {
    fn from(trigger: Trigger) -> Self {
        When {
            id: String::new(),
            name: String::new(),
            trigger,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    State {
        entity_id: EntityId,
        is: Value,
    },
    Numeric {
        entity_id: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        above: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        below: Option<f64>,
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
    /// Quelqu'un est à la maison (ou personne, `home: false`).
    Presence {
        home: bool,
    },
    /// L'alarme est « disarmed », « armed » (y compris délai d'entrée), « arming », « triggered ».
    Alarm {
        is: String,
    },
    /// Lancé par l'un de ces « Quand ».
    Triggered {
        ids: Vec<String>,
    },
    /// Toutes vraies (un groupe, pour l'imbriquer dans « au moins une »).
    All {
        conditions: Vec<Condition>,
    },
    /// Au moins une vraie.
    Any {
        conditions: Vec<Condition>,
    },
    /// Aucune vraie.
    Not {
        conditions: Vec<Condition>,
    },
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

/// Relancé pendant qu'il tourne encore ?
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// Repartir de zéro (l'exécution en cours s'arrête).
    #[default]
    Restart,
    /// Ignorer le nouveau déclenchement.
    Single,
    /// Attendre la fin, puis relancer.
    Queued,
    /// Lancer une exécution de plus, en même temps.
    Parallel,
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

/// Une branche de « selon le déclencheur » : pour ces Quand (vide : tous), si ces conditions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub when: Vec<String>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    /// « turn_on », « turn_off » ou « toggle » ; à l'allumage : luminosité, couleur (#rrggbb),
    /// blanc (mireds), fondu (secondes).
    Command {
        entity_id: EntityId,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brightness: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color_temp: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<f64>,
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
    /// Effet lumineux de Heim (`effect` vide : arrêter l'effet).
    Effect {
        entity_id: EntityId,
        #[serde(default)]
        effect: String,
        #[serde(default)]
        colors: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minutes: Option<f64>,
    },
    /// « arm_away », « arm_night » ou « disarm ».
    Alarm {
        op: String,
    },
    /// La première branche qui convient ; sinon `default`.
    Choose {
        #[serde(default)]
        branches: Vec<Branch>,
        #[serde(default)]
        default: Vec<Step>,
        /// Créé par « une action par déclencheur » : l'éditeur l'affiche à part.
        #[serde(default, skip_serializing_if = "is_false")]
        by_trigger: bool,
    },
    If {
        #[serde(default)]
        conditions: Vec<Condition>,
        #[serde(default)]
        then: Vec<Step>,
        #[serde(default, rename = "else")]
        otherwise: Vec<Step>,
    },
    /// `times` fois, tant que `while` est vrai (vérifié avant chaque tour).
    Repeat {
        #[serde(default)]
        times: u32,
        #[serde(default, rename = "while")]
        while_: Vec<Condition>,
        #[serde(default)]
        steps: Vec<Step>,
    },
    /// Attendre que les conditions soient vraies, `timeout_s` au plus.
    WaitUntil {
        conditions: Vec<Condition>,
        #[serde(default)]
        timeout_s: u64,
        /// À l'échéance : arrêter là (sinon on continue).
        #[serde(default)]
        stop_on_timeout: bool,
    },
    Stop {
        #[serde(default)]
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub triggers: Vec<When>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(default)]
    pub mode: RunMode,
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
    pub triggers: Vec<When>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(default)]
    pub mode: RunMode,
    #[serde(default)]
    pub rest_state: bool,
    #[serde(default)]
    pub rearm: Rearm,
}

/// Ce qui a lancé une exécution : disponible pour les conditions et les variables du message.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Ctx {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

/// Une ligne de l'historique d'une exécution.
#[derive(Debug, Clone, Serialize)]
pub struct TraceStep {
    pub at: DateTime<Utc>,
    /// « 2 », « 3.b1.1 »… : où se trouve l'action dans l'automatisme.
    pub path: String,
    pub label: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Une exécution (ou un déclenchement bloqué), pour l'historique.
#[derive(Debug, Clone, Serialize)]
pub struct Trace {
    pub at: DateTime<Utc>,
    pub ctx: Ctx,
    /// « running », « ok », « error », « stopped », « aborted », « skipped ».
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub steps: Vec<TraceStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<DateTime<Utc>>,
}

impl Trace {
    fn new(ctx: Ctx) -> Trace {
        Trace {
            at: Utc::now(),
            ctx,
            result: "running".into(),
            detail: None,
            steps: Vec::new(),
            ended: None,
        }
    }

    fn end(&mut self, result: &str, detail: Option<String>) {
        self.result = result.into();
        self.detail = detail;
        self.ended = Some(Utc::now());
    }
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

/// Toutes les N minutes, calé sur minuit.
pub fn interval_matches(minutes: u32, now: NaiveTime) -> bool {
    minutes > 0 && (now.hour() * 60 + now.minute()).is_multiple_of(minutes)
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
            from,
            to,
            for_s,
        } if id == entity_id => {
            let value_changed = old.is_none_or(|o| o.value != new.value);
            let from_ok = from
                .as_ref()
                .is_none_or(|f| old.is_some_and(|o| o.value == *f));
            let matches = to.as_ref().is_none_or(|t| *t == new.value);
            match (matches, from_ok && value_changed, for_s) {
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

fn device_hit(t: &Trigger, device_id: &str, event: &str) -> bool {
    matches!(t, Trigger::Device { device_id: d, event: e } if d == device_id && e.as_deref().is_none_or(|e| e.is_empty() || e == event))
}

/// Les événements de présence qu'entraîne ce changement d'une entité `person.*`.
pub fn presence_events(
    entity_id: &str,
    old: Option<&State>,
    new: &State,
    home_now: usize,
) -> Vec<PresenceEvent> {
    if !entity_id.starts_with("person.") {
        return Vec::new();
    }
    let was_home = old.is_some_and(|o| o.value == "home");
    let is_home = new.value == "home";
    match (was_home, is_home) {
        (false, true) if home_now == 1 => vec![PresenceEvent::Arrive, PresenceEvent::FirstHome],
        (false, true) => vec![PresenceEvent::Arrive],
        (true, false) if home_now == 0 => vec![PresenceEvent::Leave, PresenceEvent::LastAway],
        (true, false) => vec![PresenceEvent::Leave],
        _ => Vec::new(),
    }
}

fn people_home(states: &HashMap<EntityId, State>) -> usize {
    states
        .iter()
        .filter(|(id, s)| id.starts_with("person.") && s.value == "home")
        .count()
}

/// Tout ce dont les conditions ont besoin.
pub struct Env<'a> {
    pub states: &'a HashMap<EntityId, State>,
    pub names: &'a HashMap<EntityId, String>,
    pub now: DateTime<Local>,
    pub location: Location,
    /// Le « Quand » qui a lancé l'exécution.
    pub trigger: Option<&'a str>,
    /// Phase de l'alarme (« disarmed », « armed »…).
    pub alarm: Option<&'a str>,
}

pub fn conditions_ok(conditions: &[Condition], env: &Env) -> bool {
    first_failing(conditions, env).is_none()
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

fn alarm_word(phase: &str) -> &str {
    match phase {
        "disarmed" => "désactivée",
        "arming" => "en cours d'activation",
        "armed" => "activée",
        "pending" => "en délai d'entrée",
        "triggered" => "déclenchée",
        other => other,
    }
}

fn alarm_is(phase: Option<&str>, wanted: &str) -> bool {
    match (phase, wanted) {
        (Some(p), "armed") => p == "armed" || p == "pending",
        (Some(p), w) => p == w,
        (None, "disarmed") => true,
        _ => false,
    }
}

/// La première condition fausse, expliquée en français (« il ne faisait pas nuit »).
pub fn first_failing(conditions: &[Condition], env: &Env) -> Option<String> {
    conditions.iter().find_map(|c| condition_fails(c, env))
}

fn name_of(env: &Env, id: &str) -> String {
    env.names.get(id).cloned().unwrap_or_else(|| id.to_string())
}

/// `None` si la condition est vraie ; sinon pourquoi elle est fausse.
fn condition_fails(c: &Condition, env: &Env) -> Option<String> {
    let now_utc = env.now.with_timezone(&Utc);
    let loc = env.location;
    match c {
        Condition::State { entity_id, is } => match env.states.get(entity_id) {
            Some(s) if s.value == *is => None,
            Some(s) => Some(format!(
                "« {} » était {} et non {}",
                name_of(env, entity_id),
                value_word(entity_id, &s.value),
                value_word(entity_id, is)
            )),
            None => Some(format!(
                "« {} » n'a pas encore donné son état",
                name_of(env, entity_id)
            )),
        },
        Condition::Numeric {
            entity_id,
            above,
            below,
        } => match env.states.get(entity_id).and_then(|s| s.value.as_f64()) {
            Some(v) if numeric_inside(v, *above, *below) => None,
            Some(v) => Some(format!("« {} » valait {v}", name_of(env, entity_id))),
            None => Some(format!("« {} » n'a pas de valeur", name_of(env, entity_id))),
        },
        Condition::Time { after, before } => (!in_range(after, before, env.now.time()))
            .then(|| format!("hors de la plage {after} – {before}")),
        Condition::Days { days } => (!day_ok(days, env.now.weekday().number_from_monday() as u8))
            .then(|| "pas un des jours choisis".into()),
        Condition::Dark => (!sun::is_dark(now_utc, loc.latitude, loc.longitude))
            .then(|| "il ne faisait pas nuit".into()),
        Condition::Daylight => {
            sun::is_dark(now_utc, loc.latitude, loc.longitude).then(|| "il faisait nuit".into())
        }
        Condition::Presence { home } => {
            let n = people_home(env.states);
            match (home, n) {
                (true, 0) => Some("personne n'était à la maison".into()),
                (false, n) if n > 0 => Some("quelqu'un était à la maison".into()),
                _ => None,
            }
        }
        Condition::Alarm { is } => (!alarm_is(env.alarm, is)).then(|| {
            format!(
                "l'alarme était {}",
                alarm_word(env.alarm.unwrap_or("disarmed"))
            )
        }),
        Condition::Triggered { ids } => {
            let ok = env.trigger.is_some_and(|t| ids.iter().any(|i| i == t));
            (!ok).then(|| "lancé par un autre « Quand »".into())
        }
        Condition::All { conditions } => first_failing(conditions, env),
        Condition::Any { conditions } => {
            if conditions.is_empty() {
                return None;
            }
            let reasons: Vec<String> = conditions
                .iter()
                .map(|c| condition_fails(c, env))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("aucune n'était vraie ({})", reasons.join(" ; ")))
        }
        Condition::Not { conditions } => {
            if conditions.iter().any(|c| condition_fails(c, env).is_none()) {
                Some("une condition « aucune de » était vraie".into())
            } else {
                None
            }
        }
    }
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

fn check_entity(id: &str) -> Result<(), String> {
    if id.contains('.') {
        Ok(())
    } else if id.is_empty() {
        Err("choisissez un appareil".into())
    } else {
        Err(format!("appareil invalide : {id}"))
    }
}

const ALARM_PHASES: [&str; 5] = ["disarmed", "arming", "armed", "pending", "triggered"];

fn validate_trigger(t: &Trigger) -> Result<(), String> {
    match t {
        Trigger::State { entity_id, .. } => check_entity(entity_id),
        Trigger::Numeric {
            entity_id,
            above,
            below,
        } => {
            check_entity(entity_id)?;
            if above.is_none() && below.is_none() {
                return Err("un seuil doit préciser « au-dessus de » ou « en dessous de »".into());
            }
            Ok(())
        }
        Trigger::Device { device_id, .. } => {
            if device_id.trim().is_empty() {
                Err("choisissez l'appareil (bouton, télécommande…)".into())
            } else {
                Ok(())
            }
        }
        Trigger::Time { at, .. } => parse_hm(at)
            .map(|_| ())
            .ok_or_else(|| format!("heure invalide : {at} (format 07:30)")),
        Trigger::Interval { minutes } => {
            if *minutes == 0 || *minutes > 24 * 60 {
                Err("intervalle invalide (1 min à 24 h)".into())
            } else {
                Ok(())
            }
        }
        Trigger::Sun { offset_min, .. } => {
            if offset_min.abs() > 6 * 60 {
                Err("décalage trop grand (6 h au plus)".into())
            } else {
                Ok(())
            }
        }
        Trigger::Alarm { event } => {
            if ALARM_PHASES.contains(&event.as_str()) {
                Ok(())
            } else {
                Err(format!("état d'alarme inconnu : {event}"))
            }
        }
        Trigger::Presence { .. } | Trigger::Startup => Ok(()),
    }
}

fn validate_conditions(
    list: &[Condition],
    ids: &HashSet<&str>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("trop de groupes imbriqués".into());
    }
    for c in list {
        match c {
            Condition::Time { after, before } => {
                parse_hm(after).ok_or_else(|| format!("heure invalide : {after}"))?;
                parse_hm(before).ok_or_else(|| format!("heure invalide : {before}"))?;
            }
            Condition::State { entity_id, .. } => check_entity(entity_id)?,
            Condition::Numeric {
                entity_id,
                above,
                below,
            } => {
                check_entity(entity_id)?;
                if above.is_none() && below.is_none() {
                    return Err("une mesure doit préciser un seuil".into());
                }
            }
            Condition::Alarm { is } => {
                if !ALARM_PHASES.contains(&is.as_str()) {
                    return Err(format!("état d'alarme inconnu : {is}"));
                }
            }
            Condition::Triggered { ids: wanted } => {
                if wanted.is_empty() {
                    return Err("« lancé par » : choisissez au moins un « Quand »".into());
                }
                if let Some(bad) = wanted.iter().find(|w| !ids.contains(w.as_str())) {
                    return Err(format!("« Quand » inconnu : {bad}"));
                }
            }
            Condition::All { conditions }
            | Condition::Any { conditions }
            | Condition::Not { conditions } => validate_conditions(conditions, ids, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}

fn valid_hex(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())
}

fn validate_steps(steps: &[Step], ids: &HashSet<&str>, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("trop de blocs imbriqués".into());
    }
    for a in steps {
        match a {
            Step::Command {
                entity_id,
                action,
                color,
                transition,
                ..
            } => {
                check_entity(entity_id)?;
                if !["turn_on", "turn_off", "toggle"].contains(&action.as_str()) {
                    return Err(format!("action inconnue : {action}"));
                }
                if color.as_deref().is_some_and(|c| !valid_hex(c)) {
                    return Err("couleur invalide (#rrggbb attendu)".into());
                }
                if transition.is_some_and(|t| !(0.0..=3600.0).contains(&t)) {
                    return Err("fondu invalide (1 h au plus)".into());
                }
            }
            Step::TurnOnFor { entity_id, minutes } => {
                check_entity(entity_id)?;
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
            Step::Scene { scene_id, .. } if scene_id.trim().is_empty() => {
                return Err("choisissez la scène".into());
            }
            Step::Effect {
                entity_id, colors, ..
            } => {
                check_entity(entity_id)?;
                if colors.iter().any(|c| !valid_hex(c)) {
                    return Err("couleur invalide (#rrggbb attendu)".into());
                }
            }
            Step::Alarm { op } => {
                if !["arm_away", "arm_night", "disarm"].contains(&op.as_str()) {
                    return Err(format!("action d'alarme inconnue : {op}"));
                }
            }
            Step::Choose {
                branches, default, ..
            } => {
                for b in branches {
                    if let Some(bad) = b.when.iter().find(|w| !ids.contains(w.as_str())) {
                        return Err(format!("« Quand » inconnu : {bad}"));
                    }
                    validate_conditions(&b.conditions, ids, depth + 1)?;
                    validate_steps(&b.steps, ids, depth + 1)?;
                }
                validate_steps(default, ids, depth + 1)?;
            }
            Step::If {
                conditions,
                then,
                otherwise,
            } => {
                if conditions.is_empty() {
                    return Err("« si… sinon » : ajoutez une condition".into());
                }
                validate_conditions(conditions, ids, depth + 1)?;
                validate_steps(then, ids, depth + 1)?;
                validate_steps(otherwise, ids, depth + 1)?;
            }
            Step::Repeat {
                times,
                while_,
                steps,
            } => {
                if *times > 1000 || (*times == 0 && while_.is_empty()) {
                    return Err("répéter : de 1 à 1000 fois".into());
                }
                validate_conditions(while_, ids, depth + 1)?;
                validate_steps(steps, ids, depth + 1)?;
            }
            Step::WaitUntil {
                conditions,
                timeout_s,
                ..
            } => {
                if conditions.is_empty() {
                    return Err("« attendre que » : ajoutez une condition".into());
                }
                if *timeout_s > MAX_DELAY_S {
                    return Err("attente trop longue (24 h au plus)".into());
                }
                validate_conditions(conditions, ids, depth + 1)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Donne un identifiant (« t1 », « t2 »…) aux « Quand » qui n'en ont pas.
fn assign_ids(triggers: &mut [When]) {
    let mut used: HashSet<String> = triggers
        .iter()
        .filter(|w| !w.id.is_empty())
        .map(|w| w.id.clone())
        .collect();
    let mut n = 1;
    for w in triggers.iter_mut().filter(|w| w.id.is_empty()) {
        while used.contains(&format!("t{n}")) {
            n += 1;
        }
        w.id = format!("t{n}");
        used.insert(w.id.clone());
    }
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
    let mut ids = HashSet::new();
    for w in &input.triggers {
        if !w.id.is_empty() && !ids.insert(w.id.as_str()) {
            return Err(format!("deux « Quand » ont le même identifiant : {}", w.id));
        }
        validate_trigger(&w.trigger)?;
    }
    for t in &input.rearm.triggers {
        validate_trigger(t).map_err(|e| format!("réarmement : {e}"))?;
    }
    if input
        .rearm
        .after_min
        .is_some_and(|m| m == 0 || m > 31 * 24 * 60)
    {
        return Err("délai de réarmement invalide".into());
    }
    validate_conditions(&input.conditions, &ids, 0)?;
    validate_conditions(&input.rearm.conditions, &ids, 0)?;
    validate_steps(&input.actions, &ids, 0)
}

// ---- Phrases pour l'historique ---------------------------------------------------

fn minutes_word(s: u64) -> String {
    if s >= 3600 && s.is_multiple_of(3600) {
        format!("{} h", s / 3600)
    } else if s >= 60 && s.is_multiple_of(60) {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

fn describe_trigger(t: &Trigger, names: &HashMap<EntityId, String>) -> String {
    let n = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    match t {
        Trigger::State { entity_id, to, .. } => match to {
            Some(v) => format!("« {} » devient {}", n(entity_id), value_word(entity_id, v)),
            None => format!("« {} » change", n(entity_id)),
        },
        Trigger::Numeric { entity_id, .. } => format!("« {} » franchit le seuil", n(entity_id)),
        Trigger::Device { event, .. } => match event {
            Some(e) if !e.is_empty() => format!("action « {e} »"),
            _ => "action d'un appareil".into(),
        },
        Trigger::Time { at, .. } => format!("à {at}"),
        Trigger::Interval { minutes } => format!("toutes les {minutes} min"),
        Trigger::Sun { event, .. } => match event {
            SunEvent::Sunrise => "lever du soleil".into(),
            SunEvent::Sunset => "coucher du soleil".into(),
        },
        Trigger::Presence { event } => match event {
            PresenceEvent::FirstHome => "le premier rentre".into(),
            PresenceEvent::LastAway => "tout le monde est parti".into(),
            PresenceEvent::Arrive => "quelqu'un rentre".into(),
            PresenceEvent::Leave => "quelqu'un part".into(),
        },
        Trigger::Alarm { event } => format!("alarme {}", alarm_word(event)),
        Trigger::Startup => "démarrage de Heim".into(),
    }
}

fn step_label(step: &Step, names: &HashMap<EntityId, String>) -> String {
    let n = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    match step {
        Step::Command {
            entity_id, action, ..
        } => {
            let verb = match action.as_str() {
                "turn_on" => "allumer",
                "turn_off" => "éteindre",
                _ => "basculer",
            };
            format!("{verb} « {} »", n(entity_id))
        }
        Step::TurnOnFor { entity_id, minutes } => {
            format!("allumer « {} » pendant {minutes} min", n(entity_id))
        }
        Step::Scene { scene_id, on } => format!(
            "{} la scène « {} »",
            if *on { "activer" } else { "désactiver" },
            n(&format!("scene.{scene_id}"))
        ),
        Step::Notify { .. } => "notification".into(),
        Step::Delay { seconds } => format!("attendre {}", minutes_word(*seconds)),
        Step::Automation { automation_id, op } => {
            let verb = match op {
                AutomationOp::Rearm => "réarmer",
                AutomationOp::Enable => "activer",
                AutomationOp::Disable => "désactiver",
                AutomationOp::Run => "lancer",
            };
            format!("{verb} « {} »", n(&format!("automation.{automation_id}")))
        }
        Step::Effect {
            entity_id, effect, ..
        } => {
            if effect.is_empty() {
                format!("arrêter l'effet de « {} »", n(entity_id))
            } else {
                format!("effet « {effect} » sur « {} »", n(entity_id))
            }
        }
        Step::Alarm { op } => match op.as_str() {
            "arm_away" => "activer l'alarme (absent)".into(),
            "arm_night" => "activer l'alarme (nuit)".into(),
            _ => "désactiver l'alarme".into(),
        },
        Step::Choose { by_trigger, .. } => {
            if *by_trigger {
                "une action par déclencheur".into()
            } else {
                "selon le cas".into()
            }
        }
        Step::If { .. } => "si… sinon".into(),
        Step::Repeat { times, .. } => format!("répéter ({times} fois au plus)"),
        Step::WaitUntil { .. } => "attendre que".into(),
        Step::Stop { .. } => "arrêter".into(),
    }
}

/// Remplace `{trigger}`, `{entity}`, `{value}`, `{from}`, `{device}`, `{event}`, `{time}`,
/// `{date}` dans un message.
pub fn fill(text: &str, ctx: &Ctx, names: &HashMap<EntityId, String>) -> String {
    if !text.contains('{') {
        return text.to_string();
    }
    let now = Local::now();
    let entity = ctx.entity_id.as_deref().unwrap_or("");
    let word = |v: &Option<Value>| {
        v.as_ref()
            .map(|v| value_word(entity, v))
            .unwrap_or_default()
    };
    let pairs = [
        ("{trigger}", ctx.trigger.clone().unwrap_or_default()),
        (
            "{entity}",
            names
                .get(entity)
                .cloned()
                .unwrap_or_else(|| entity.to_string()),
        ),
        ("{value}", word(&ctx.to)),
        ("{from}", word(&ctx.from)),
        ("{device}", ctx.device.clone().unwrap_or_default()),
        ("{event}", ctx.event.clone().unwrap_or_default()),
        ("{time}", now.format("%H:%M").to_string()),
        ("{date}", now.format("%d/%m/%Y").to_string()),
    ];
    let mut out = text.to_string();
    for (k, v) in pairs {
        out = out.replace(k, &v);
    }
    out
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
    pub effects: crate::effects::Effects,
    pub security: crate::security::SecurityHandle,
}

/// Un appareil qui a déjà envoyé des actions (boutons, télécommandes).
#[derive(Debug, Clone, Default, Serialize)]
pub struct SeenDevice {
    pub integration: String,
    pub name: String,
    pub events: Vec<String>,
}

/// La dernière action reçue, tous appareils confondus (bouton « Détecter »).
#[derive(Debug, Clone, Serialize)]
pub struct LastEvent {
    pub device_id: String,
    pub integration: String,
    pub name: String,
    pub event: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceEvents {
    pub seen: BTreeMap<String, SeenDevice>,
    pub last: Option<LastEvent>,
}

enum Msg {
    List(oneshot::Sender<Vec<Automation>>),
    Upsert(
        String,
        AutomationInput,
        oneshot::Sender<Result<Automation, String>>,
    ),
    Delete(String, oneshot::Sender<bool>),
    /// Lancer à la main, éventuellement « comme si » tel Quand l'avait déclenché.
    Run(String, Option<String>, oneshot::Sender<Result<(), String>>),
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
        run: u64,
        error: Option<String>,
        stopped: Option<String>,
    },
    Startup,
    Traces(String, oneshot::Sender<Vec<Trace>>),
    Devices(oneshot::Sender<DeviceEvents>),
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
    pub async fn run(&self, id: &str, trigger: Option<String>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Run(id.to_string(), trigger, tx))
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

    /// Les dernières exécutions, la plus récente d'abord.
    pub async fn traces(&self, id: &str) -> Vec<Trace> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Traces(id.to_string(), tx)).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Les actions déjà reçues des boutons et télécommandes.
    pub async fn device_events(&self) -> DeviceEvents {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Devices(tx)).await.is_err() {
            return DeviceEvents::default();
        }
        rx.await.unwrap_or_default()
    }
}

struct Slot {
    run: u64,
    handle: JoinHandle<()>,
    trace: Arc<Mutex<Trace>>,
}

struct Engine {
    automations: BTreeMap<String, Automation>,
    states: HashMap<EntityId, State>,
    names: HashMap<EntityId, String>,
    waiting: HashMap<(String, usize), JoinHandle<()>>,
    running: HashMap<String, Vec<Slot>>,
    queue: HashMap<String, VecDeque<Ctx>>,
    traces: HashMap<String, VecDeque<Trace>>,
    devices: DeviceEvents,
    alarm: Option<String>,
    next_run: u64,
    deps: Deps,
    tx: mpsc::Sender<Msg>,
    publish: mpsc::UnboundedSender<Automation>,
    last_minute: Option<(u32, u32)>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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
    let mut list = automations;
    for a in &mut list {
        // Anciens automatismes : leurs « Quand » reçoivent un identifiant.
        if a.triggers.iter().any(|w| w.id.is_empty()) {
            assign_ids(&mut a.triggers);
            deps.store.put(Table::Automations, &a.id, &*a);
        }
    }
    let engine = Engine {
        automations: list.into_iter().map(|a| (a.id.clone(), a)).collect(),
        states: HashMap::new(),
        names: HashMap::new(),
        waiting: HashMap::new(),
        running: HashMap::new(),
        queue: HashMap::new(),
        traces: HashMap::new(),
        devices: DeviceEvents::default(),
        alarm: None,
        next_run: 0,
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
        Trigger::Interval { minutes } => interval_matches(*minutes, now.time()),
        Trigger::Sun { event, offset_min } => {
            sun_minute(*event, *offset_min, now, loc) == Some((now.hour(), now.minute()))
        }
        _ => false,
    }
}

fn phase_of(status: &Value) -> Option<String> {
    status
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_string)
}

impl Engine {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Msg>,
        mut bus_rx: tokio::sync::broadcast::Receiver<Event>,
    ) {
        self.resync().await;
        self.alarm =
            phase_of(&serde_json::to_value(self.deps.security.status().await).unwrap_or_default());
        for a in self.automations.values() {
            let _ = self.publish.send(a.clone());
        }
        // « Au démarrage de Heim » : on laisse aux intégrations le temps de donner leurs états.
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let _ = tx.send(Msg::Startup).await;
        });
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
                    Ok(Event::DeviceEvent { integration, device_id, name, event }) => {
                        self.on_device_event(integration, device_id, name, event);
                    }
                    Ok(Event::Security { status }) => {
                        let phase = phase_of(&status);
                        if phase.is_some() && phase != self.alarm {
                            self.alarm = phase.clone();
                            let p = phase.unwrap_or_default();
                            let ctx = Ctx { event: Some(p.clone()), ..Default::default() };
                            self.dispatch(|t| matches!(t, Trigger::Alarm { event } if *event == p), ctx);
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

    /// Lance les automatismes dont un « Quand » répond à `hit`, réarme ceux dont une règle de
    /// réarmement y répond.
    fn dispatch(&mut self, hit: impl Fn(&Trigger) -> bool, base: Ctx) {
        let mut fire = Vec::new();
        let mut rearm = Vec::new();
        for a in self.automations.values() {
            if a.enabled
                && let Some(w) = a.triggers.iter().find(|w| hit(&w.trigger))
            {
                fire.push((a.id.clone(), w.clone()));
            }
            if a.fired() && a.rearm.triggers.iter().any(&hit) {
                rearm.push(a.id.clone());
            }
        }
        for (id, w) in fire {
            let ctx = self.ctx_for(&w, base.clone());
            self.trigger(&id, ctx, false);
        }
        for id in rearm {
            self.rearm_checked(&id);
        }
    }

    fn ctx_for(&self, w: &When, mut base: Ctx) -> Ctx {
        base.trigger_id = Some(w.id.clone());
        base.trigger = Some(if w.name.trim().is_empty() {
            describe_trigger(&w.trigger, &self.names)
        } else {
            w.name.clone()
        });
        base
    }

    fn on_device_event(
        &mut self,
        integration: String,
        device_id: String,
        name: String,
        event: String,
    ) {
        if event.is_empty() {
            return;
        }
        let seen = self.devices.seen.entry(device_id.clone()).or_default();
        seen.integration = integration.clone();
        seen.name = name.clone();
        if !seen.events.contains(&event) && seen.events.len() < 100 {
            seen.events.push(event.clone());
        }
        self.devices.last = Some(LastEvent {
            device_id: device_id.clone(),
            integration,
            name: name.clone(),
            event: event.clone(),
            at: Utc::now(),
        });
        let ctx = Ctx {
            device_id: Some(device_id.clone()),
            device: Some(name),
            event: Some(event.clone()),
            ..Default::default()
        };
        self.dispatch(|t| device_hit(t, &device_id, &event), ctx);
    }

    fn on_state(&mut self, entity_id: &str, old: Option<&State>, new: &State) {
        let mut fire: Vec<(String, When)> = Vec::new();
        let mut rearm = Vec::new();
        let mut waits = Vec::new();
        let mut cancels = Vec::new();
        for a in self.automations.values() {
            let mut fired_here = false;
            if a.enabled {
                for (i, w) in a.triggers.iter().enumerate() {
                    match on_state_change(&w.trigger, entity_id, old, new) {
                        Hit::Now if !fired_here => {
                            fired_here = true;
                            fire.push((a.id.clone(), w.clone()));
                        }
                        Hit::After(secs) => waits.push(((a.id.clone(), i), secs)),
                        Hit::Cancel => cancels.push((a.id.clone(), i)),
                        _ => {}
                    }
                }
            }
            if a.fired() {
                for (i, t) in a.rearm.triggers.iter().enumerate() {
                    let key = (a.id.clone(), REARM_BASE + i);
                    match on_state_change(t, entity_id, old, new) {
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
        let base = Ctx {
            entity_id: Some(entity_id.to_string()),
            from: old.map(|o| o.value.clone()),
            to: Some(new.value.clone()),
            ..Default::default()
        };
        for (id, w) in fire {
            let ctx = self.ctx_for(&w, base.clone());
            self.trigger(&id, ctx, false);
        }
        for id in rearm {
            self.rearm_checked(&id);
        }
        // Présence : le premier qui rentre, le dernier qui part…
        let events = presence_events(entity_id, old, new, people_home(&self.states));
        if !events.is_empty() {
            self.dispatch(
                |t| matches!(t, Trigger::Presence { event } if events.contains(event)),
                base,
            );
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
        self.dispatch(|t| trigger_at_minute(t, now, loc), Ctx::default());
        let utc = Utc::now();
        let rearm: Vec<String> = self
            .automations
            .values()
            .filter(|a| a.fired())
            .filter(|a| {
                a.rearm
                    .after_min
                    .zip(a.fired_at)
                    .is_some_and(|(m, at)| utc >= at + TimeDelta::minutes(m as i64))
            })
            .map(|a| a.id.clone())
            .collect();
        for id in rearm {
            self.rearm_checked(&id);
        }
    }

    fn env<'a>(&'a self, trigger: Option<&'a str>) -> Env<'a> {
        Env {
            states: &self.states,
            names: &self.names,
            now: Local::now(),
            location: self.location(),
            trigger,
            alarm: self.alarm.as_deref(),
        }
    }

    /// Réarmement par ses propres règles : seulement si ses conditions sont vraies.
    fn rearm_checked(&mut self, id: &str) {
        let Some(a) = self.automations.get(id) else {
            return;
        };
        if !a.fired() || !conditions_ok(&a.rearm.conditions, &self.env(None)) {
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
            let ctx = Ctx {
                trigger: Some(format!("lancé par {source}")),
                ..Default::default()
            };
            if self.trigger(id, ctx, true) {
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

    fn remember(&mut self, id: &str, trace: Trace) {
        let list = self.traces.entry(id.to_string()).or_default();
        list.push_front(trace);
        list.truncate(MAX_TRACES);
    }

    /// Vérifie les conditions et le mode de relance, puis lance les actions.
    fn trigger(&mut self, id: &str, ctx: Ctx, force: bool) -> bool {
        let Some(a) = self.automations.get(id) else {
            return false;
        };
        let reason = if force {
            None
        } else {
            first_failing(&a.conditions, &self.env(ctx.trigger_id.as_deref()))
        };
        if let Some(reason) = reason {
            tracing::debug!("automatisme {id} : pas lancé, {reason}");
            let a = self.automations.get_mut(id).expect("présent");
            a.last_check = Some(Check {
                at: Utc::now(),
                ran: false,
                reason: Some(reason.clone()),
            });
            let name = a.name.clone();
            self.save(id);
            let mut trace = Trace::new(ctx);
            trace.end("skipped", Some(reason.clone()));
            self.remember(id, trace);
            self.deps.bus.publish(Event::AutomationSkipped {
                id: id.to_string(),
                name,
                reason,
            });
            return false;
        }
        let mode = a.mode;
        let slots = self.running.entry(id.to_string()).or_default();
        slots.retain(|s| !s.handle.is_finished());
        let busy = !slots.is_empty();
        match mode {
            RunMode::Single if busy => {
                let mut trace = Trace::new(ctx);
                trace.end("skipped", Some("déjà en cours".into()));
                self.remember(id, trace);
                return true;
            }
            RunMode::Queued if busy => {
                let q = self.queue.entry(id.to_string()).or_default();
                if q.len() < MAX_RUNS {
                    q.push_back(ctx);
                }
                return true;
            }
            RunMode::Parallel if slots.len() >= MAX_RUNS => {
                let mut trace = Trace::new(ctx);
                trace.end("skipped", Some("trop d'exécutions en même temps".into()));
                self.remember(id, trace);
                return true;
            }
            RunMode::Restart if busy => {
                let old: Vec<Slot> = std::mem::take(slots);
                for s in old {
                    s.handle.abort();
                    let mut t = lock(&s.trace).clone();
                    t.end("aborted", Some("relancé avant la fin".into()));
                    self.remember(id, t);
                }
            }
            _ => {}
        }
        self.start(id, ctx);
        true
    }

    fn start(&mut self, id: &str, ctx: Ctx) {
        let Some(a) = self.automations.get_mut(id) else {
            return;
        };
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
        self.save(id);
        self.next_run += 1;
        let run = self.next_run;
        let trace = Arc::new(Mutex::new(Trace::new(ctx.clone())));
        let runner = Runner {
            deps: self.deps.clone(),
            engine: self.tx.clone(),
            source: format!("automation:{id}"),
            ctx,
            trace: trace.clone(),
            names: self.names.clone(),
            errors: Vec::new(),
        };
        let tx = self.tx.clone();
        let id_owned = id.to_string();
        let handle = tokio::spawn(async move {
            let mut runner = runner;
            let flow = runner.steps(&steps, String::new(), 0).await;
            let error = (!runner.errors.is_empty()).then(|| runner.errors.join(" ; "));
            let stopped = match flow {
                Flow::Stop(r) => Some(r),
                Flow::Go => None,
            };
            let _ = tx
                .send(Msg::Finished {
                    id: id_owned,
                    run,
                    error,
                    stopped,
                })
                .await;
        });
        self.running
            .entry(id.to_string())
            .or_default()
            .push(Slot { run, handle, trace });
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
                    for s in self.running.remove(&id).unwrap_or_default() {
                        s.handle.abort();
                    }
                    self.queue.remove(&id);
                    self.traces.remove(&id);
                }
                let _ = reply.send(removed.is_some());
            }
            Msg::Run(id, trigger, reply) => {
                let when = trigger.and_then(|t| {
                    self.automations
                        .get(&id)?
                        .triggers
                        .iter()
                        .find(|w| w.id == t)
                        .cloned()
                });
                let manual = Ctx {
                    trigger: Some("lancé à la main".into()),
                    ..Default::default()
                };
                let ctx = match when {
                    Some(w) => {
                        let mut c = self.ctx_for(&w, manual);
                        c.trigger = c.trigger.map(|t| format!("{t} (essai à la main)"));
                        c
                    }
                    None => manual,
                };
                let ok = self.automations.contains_key(&id) && self.trigger(&id, ctx, true);
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
                let (rearm, index) = match trigger.checked_sub(REARM_BASE) {
                    Some(i) => (true, i),
                    None => (false, trigger),
                };
                let Some(a) = self.automations.get(&id) else {
                    return;
                };
                let (t, when) = if rearm {
                    (a.rearm.triggers.get(index).cloned(), None)
                } else {
                    let w = a.triggers.get(index).cloned();
                    (w.as_ref().map(|w| w.trigger.clone()), w)
                };
                let still = t.is_some_and(|t| match t {
                    Trigger::State { entity_id, to, .. } => to.as_ref().is_none_or(|to| {
                        self.states.get(&entity_id).is_some_and(|s| s.value == *to)
                    }),
                    _ => false,
                });
                if still {
                    if rearm {
                        self.rearm_checked(&id);
                    } else if let Some(w) = when {
                        let entity = match &w.trigger {
                            Trigger::State { entity_id, .. } => Some(entity_id.clone()),
                            _ => None,
                        };
                        let base = Ctx {
                            to: entity
                                .as_ref()
                                .and_then(|e| self.states.get(e))
                                .map(|s| s.value.clone()),
                            entity_id: entity,
                            ..Default::default()
                        };
                        let ctx = self.ctx_for(&w, base);
                        self.trigger(&id, ctx, false);
                    }
                }
            }
            Msg::Finished {
                id,
                run,
                error,
                stopped,
            } => {
                let slot = self.running.get_mut(&id).and_then(|slots| {
                    let i = slots.iter().position(|s| s.run == run)?;
                    Some(slots.remove(i))
                });
                if let Some(slot) = slot {
                    let mut t = lock(&slot.trace).clone();
                    match (&error, &stopped) {
                        (Some(e), _) => t.end("error", Some(e.clone())),
                        (None, Some(r)) => t.end("stopped", Some(r.clone())),
                        _ => t.end("ok", None),
                    }
                    self.remember(&id, t);
                }
                if let Some(a) = self.automations.get_mut(&id) {
                    a.last_run = Some(Utc::now());
                    a.last_error = error.clone();
                }
                self.save(&id);
                if let Some(e) = &error {
                    tracing::warn!("automatisme {id} : {e}");
                }
                self.deps.bus.publish(Event::AutomationRan {
                    id: id.clone(),
                    ok: error.is_none(),
                    detail: error,
                });
                // En file : au suivant.
                if let Some(next) = self.queue.get_mut(&id).and_then(VecDeque::pop_front) {
                    self.start(&id, next);
                }
            }
            Msg::Startup => {
                self.dispatch(|t| matches!(t, Trigger::Startup), Ctx::default());
            }
            Msg::Traces(id, reply) => {
                let mut list: Vec<Trace> = self
                    .running
                    .get(&id)
                    .map(|slots| {
                        slots
                            .iter()
                            .filter(|s| !s.handle.is_finished())
                            .map(|s| lock(&s.trace).clone())
                            .collect()
                    })
                    .unwrap_or_default();
                list.extend(self.traces.get(&id).into_iter().flatten().cloned());
                list.sort_by_key(|b| std::cmp::Reverse(b.at));
                let _ = reply.send(list);
            }
            Msg::Devices(reply) => {
                let _ = reply.send(self.devices.clone());
            }
        }
    }

    fn upsert(&mut self, raw_id: String, input: AutomationInput) -> Result<Automation, String> {
        let mut input = input;
        assign_ids(&mut input.triggers);
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
            mode: input.mode,
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
        self.queue.remove(&id);
        self.automations.insert(id.clone(), automation.clone());
        self.save(&id);
        Ok(automation)
    }
}

// ---- Exécution des actions ---------------------------------------------------------

enum Flow {
    Go,
    Stop(String),
}

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Une exécution : déroule les actions, note chaque pas dans l'historique.
struct Runner {
    deps: Deps,
    engine: mpsc::Sender<Msg>,
    source: String,
    ctx: Ctx,
    trace: Arc<Mutex<Trace>>,
    names: HashMap<EntityId, String>,
    errors: Vec<String>,
}

fn join(path: &str, part: impl std::fmt::Display) -> String {
    if path.is_empty() {
        part.to_string()
    } else {
        format!("{path}.{part}")
    }
}

impl Runner {
    fn note(&self, path: &str, label: String, ok: bool, detail: Option<String>) {
        let mut t = lock(&self.trace);
        if t.steps.len() < MAX_TRACE_STEPS {
            t.steps.push(TraceStep {
                at: Utc::now(),
                path: path.to_string(),
                label,
                ok,
                detail,
            });
        }
    }

    /// États, noms et alarme à cet instant, pour les conditions en cours d'exécution.
    async fn snapshot(&mut self) -> (HashMap<EntityId, State>, Option<String>) {
        let all = self.deps.registry.snapshot().await;
        let mut states = HashMap::new();
        for e in all {
            self.names
                .insert(e.meta.entity_id.clone(), e.meta.name.clone());
            if let Some(s) = e.state {
                states.insert(e.meta.entity_id, s);
            }
        }
        let alarm =
            phase_of(&serde_json::to_value(self.deps.security.status().await).unwrap_or_default());
        (states, alarm)
    }

    async fn check(&mut self, conditions: &[Condition]) -> Option<String> {
        if conditions.is_empty() {
            return None;
        }
        let (states, alarm) = self.snapshot().await;
        let location = self
            .deps
            .settings
            .get_or_default::<Location>(settings::LOCATION);
        let env = Env {
            states: &states,
            names: &self.names,
            now: Local::now(),
            location,
            trigger: self.ctx.trigger_id.as_deref(),
            alarm: alarm.as_deref(),
        };
        first_failing(conditions, &env)
    }

    fn steps<'a>(&'a mut self, steps: &'a [Step], path: String, depth: usize) -> Boxed<'a, Flow> {
        Box::pin(async move {
            for (i, step) in steps.iter().enumerate() {
                let here = join(&path, i + 1);
                if let Flow::Stop(r) = self.step(step, here, depth).await {
                    return Flow::Stop(r);
                }
            }
            Flow::Go
        })
    }

    async fn step(&mut self, step: &Step, path: String, depth: usize) -> Flow {
        let label = step_label(step, &self.names);
        match step {
            Step::Choose {
                branches,
                default,
                by_trigger,
            } => {
                for (i, b) in branches.iter().enumerate() {
                    let for_us = b.when.is_empty()
                        || self
                            .ctx
                            .trigger_id
                            .as_ref()
                            .is_some_and(|t| b.when.contains(t));
                    if !for_us {
                        continue;
                    }
                    if self.check(&b.conditions).await.is_some() {
                        continue;
                    }
                    let name = if b.name.is_empty() && *by_trigger && b.when.len() == 1 {
                        format!("Alors {}", i + 1)
                    } else if b.name.is_empty() {
                        format!("branche {}", i + 1)
                    } else {
                        format!("« {} »", b.name)
                    };
                    self.note(&path, format!("{label} → {name}"), true, None);
                    return self
                        .steps(&b.steps, join(&path, format!("b{}", i + 1)), depth + 1)
                        .await;
                }
                if default.is_empty() {
                    self.note(&path, format!("{label} → aucune branche"), true, None);
                    return Flow::Go;
                }
                self.note(&path, format!("{label} → sinon"), true, None);
                self.steps(default, join(&path, "sinon"), depth + 1).await
            }
            Step::If {
                conditions,
                then,
                otherwise,
            } => match self.check(conditions).await {
                None => {
                    self.note(&path, "si : vrai → alors".into(), true, None);
                    self.steps(then, join(&path, "alors"), depth + 1).await
                }
                Some(why) => {
                    self.note(&path, "si : faux → sinon".into(), true, Some(why));
                    self.steps(otherwise, join(&path, "sinon"), depth + 1).await
                }
            },
            Step::Repeat {
                times,
                while_,
                steps,
            } => {
                let max = if *times == 0 {
                    1000
                } else {
                    (*times).min(1000)
                };
                let mut done = 0;
                for n in 0..max {
                    if let Some(why) = self.check(while_).await {
                        self.note(
                            &path,
                            format!("répéter : fin après {n} tour(s)"),
                            true,
                            Some(why),
                        );
                        return Flow::Go;
                    }
                    if let Flow::Stop(r) = self
                        .steps(steps, join(&path, format!("r{}", n + 1)), depth + 1)
                        .await
                    {
                        return Flow::Stop(r);
                    }
                    done = n + 1;
                }
                self.note(&path, format!("répéter : {done} tour(s)"), true, None);
                Flow::Go
            }
            Step::WaitUntil {
                conditions,
                timeout_s,
                stop_on_timeout,
            } => {
                let started = std::time::Instant::now();
                match self.wait_until(conditions, *timeout_s).await {
                    true => {
                        self.note(
                            &path,
                            label,
                            true,
                            Some(format!(
                                "vrai après {}",
                                minutes_word(started.elapsed().as_secs())
                            )),
                        );
                        Flow::Go
                    }
                    false if *stop_on_timeout => {
                        self.note(&path, label, false, Some("délai dépassé : arrêt".into()));
                        Flow::Stop("délai dépassé".into())
                    }
                    false => {
                        self.note(
                            &path,
                            label,
                            true,
                            Some("délai dépassé : on continue".into()),
                        );
                        Flow::Go
                    }
                }
            }
            Step::Stop { reason } => {
                let r = if reason.trim().is_empty() {
                    "arrêté par l'action « arrêter »".to_string()
                } else {
                    fill(reason, &self.ctx, &self.names)
                };
                self.note(&path, label, true, Some(r.clone()));
                Flow::Stop(r)
            }
            _ => {
                let result = self.simple(step).await;
                match result {
                    Ok(detail) => self.note(&path, label, true, detail),
                    Err(e) => {
                        // Une lampe injoignable ne doit pas empêcher la notification qui suit.
                        self.note(&path, label, false, Some(e.clone()));
                        self.errors.push(e);
                    }
                }
                Flow::Go
            }
        }
    }

    /// Attend que les conditions soient vraies (`true`), ou l'échéance (`false`).
    async fn wait_until(&mut self, conditions: &[Condition], timeout_s: u64) -> bool {
        let mut rx = self.deps.bus.subscribe();
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(if timeout_s == 0 {
                MAX_DELAY_S
            } else {
                timeout_s
            });
        loop {
            if self.check(conditions).await.is_none() {
                return true;
            }
            // On revérifie à chaque changement, et chaque minute (heures, nuit…).
            let tick = tokio::time::Instant::now() + Duration::from_secs(60);
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Ok(Event::StateChanged { .. }) | Ok(Event::Security { .. }) | Err(RecvError::Lagged(_)) => break,
                        Ok(_) => {}
                        Err(RecvError::Closed) => return false,
                    },
                    _ = tokio::time::sleep_until(tick) => break,
                    _ = tokio::time::sleep_until(deadline) => return false,
                }
            }
        }
    }

    /// Les actions simples. `Ok(Some(détail))` pour l'historique.
    async fn simple(&mut self, step: &Step) -> Result<Option<String>, String> {
        let deps = self.deps.clone();
        let source = self.source.as_str();
        match step {
            Step::Command {
                entity_id,
                action,
                brightness,
                color,
                color_temp,
                transition,
            } => {
                if action == "turn_on"
                    && (color.is_some() || transition.is_some())
                    && let Some(done) = self
                        .direct_light(
                            entity_id,
                            *brightness,
                            color.as_deref(),
                            *color_temp,
                            *transition,
                        )
                        .await
                {
                    return Ok(Some(done));
                }
                let action = match action.as_str() {
                    "turn_on" => Action::TurnOn {
                        brightness: *brightness,
                        color_temp: *color_temp,
                    },
                    "turn_off" => Action::TurnOff,
                    _ => Action::Toggle,
                };
                deps.registry
                    .command(entity_id, action, source)
                    .await
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            Step::TurnOnFor { entity_id, minutes } => deps
                .timers
                .run_for(entity_id, minutes * 60)
                .await
                .map(|_| None),
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
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            Step::Notify {
                message,
                title,
                to,
                except,
            } => {
                let message = fill(message, &self.ctx, &self.names);
                let title = title.as_ref().map(|t| fill(t, &self.ctx, &self.names));
                deps.notify
                    .send(NotifyRequest {
                        title,
                        message: message.clone(),
                        level: Level::Important,
                        to: to.clone(),
                        except: except.clone(),
                        action: None,
                        kind: Some(crate::center::Kind::Automation),
                        ..Default::default()
                    })
                    .map(|_| Some(message))
                    .map_err(|e| {
                        if e.contains("aucun destinataire") {
                            "notification non envoyée : aucun téléphone n'a encore ouvert Heim"
                                .to_string()
                        } else {
                            e
                        }
                    })
            }
            Step::Delay { seconds } => {
                tokio::time::sleep(Duration::from_secs(*seconds)).await;
                Ok(None)
            }
            Step::Automation { automation_id, op } => {
                let op = match op {
                    AutomationOp::Rearm => OpKind::State(None),
                    AutomationOp::Enable => OpKind::Enabled(true),
                    AutomationOp::Disable => OpKind::Enabled(false),
                    AutomationOp::Run => OpKind::Run,
                };
                // Pas de réponse attendue : le moteur est peut-être en train de nous attendre.
                self.engine
                    .send(Msg::Op {
                        id: automation_id.clone(),
                        op,
                        source: source.to_string(),
                        reply: None,
                    })
                    .await
                    .map(|_| None)
                    .map_err(|_| STOPPED.to_string())
            }
            Step::Effect {
                entity_id,
                effect,
                colors,
                minutes,
            } => {
                if effect.is_empty() || effect == "stop" {
                    deps.effects.stop(entity_id);
                    return Ok(None);
                }
                let e = deps
                    .registry
                    .get(entity_id)
                    .await
                    .ok_or_else(|| format!("appareil inconnu : {entity_id}"))?;
                deps.effects
                    .start(
                        &e,
                        crate::effects::Request {
                            effect: effect.clone(),
                            colors: colors.clone(),
                            minutes: *minutes,
                        },
                    )
                    .map(|_| None)
            }
            Step::Alarm { op } => {
                use crate::security::Mode;
                match op.as_str() {
                    "arm_away" | "arm_night" => {
                        let mode = if op == "arm_away" {
                            Mode::Away
                        } else {
                            Mode::Night
                        };
                        deps.security
                            .arm(mode, false, None)
                            .await
                            .map(|_| None)
                            .map_err(|e| format!("alarme non activée : {}", e.message()))
                    }
                    _ => deps
                        .security
                        .disarm(None)
                        .await
                        .map(|_| None)
                        .map_err(|e| format!("alarme non désactivée : {e}")),
                }
            }
            // Les blocs sont traités par `step`.
            _ => Ok(None),
        }
    }

    /// Couleur ou fondu : envoyé directement à la lampe Zigbee ou Tuya. `None` : impossible
    /// (autre intégration), on se rabat sur une commande simple.
    async fn direct_light(
        &self,
        entity_id: &str,
        brightness: Option<u8>,
        color: Option<&str>,
        color_temp: Option<u16>,
        transition: Option<f64>,
    ) -> Option<String> {
        let e = self.deps.registry.get(entity_id).await?;
        let integration = e.meta.integration.clone();
        if !matches!(integration.as_str(), "zigbee2mqtt" | "tuya")
            || !entity_id.starts_with("light.")
        {
            return None;
        }
        let device = e.meta.device_id.clone()?;
        let mut v = Map::new();
        v.insert("state".into(), json!("ON"));
        if let Some(b) = brightness {
            let b = if integration == "zigbee2mqtt" {
                (b as f64 * 254.0 / 255.0).round().max(1.0) as u64
            } else {
                b.max(1) as u64
            };
            v.insert("brightness".into(), json!(b));
        }
        if let Some(c) = color {
            v.insert("color".into(), json!({ "hex": c }));
        } else if let Some(ct) = color_temp {
            v.insert("color_temp".into(), json!(ct));
        }
        if let Some(t) = transition {
            v.insert("transition".into(), json!(t));
        }
        // Pour le journal, et pour arrêter un effet en cours sur la lampe.
        self.deps.bus.publish(Event::CommandNoted {
            entity_id: entity_id.to_string(),
            source: self.source.clone(),
        });
        self.deps.bus.publish(Event::DeviceSet {
            integration,
            device_id: device,
            values: v,
        });
        Some(match color {
            Some(c) => format!("couleur {c}"),
            None => "fondu".into(),
        })
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
            from: None,
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
            from: None,
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

    #[test]
    fn named_triggers_and_branches_parse() {
        let mut input: AutomationInput = serde_json::from_value(json!({
            "name": "Télécommande",
            "mode": "queued",
            "triggers": [
                {"type": "device", "device_id": "0x01", "event": "1_single", "name": "Bouton 1"},
                {"type": "device", "device_id": "0x01", "event": "2_single", "id": "b2"},
                {"type": "startup"},
                {"type": "interval", "minutes": 15}
            ],
            "actions": [{
                "type": "choose", "by_trigger": true,
                "branches": [
                    {"when": ["t1"], "steps": [{"type": "command", "entity_id": "light.salon", "action": "toggle"}]},
                    {"when": ["b2"], "steps": [{"type": "command", "entity_id": "light.cuisine", "action": "turn_on", "color": "#ff8800", "transition": 2}]}
                ],
                "default": [{"type": "stop"}]
            }, {"type": "if", "conditions": [{"type": "any", "conditions": [{"type": "dark"}, {"type": "presence", "home": false}]}],
                "then": [{"type": "notify", "message": "{trigger} à {time}"}], "else": []},
               {"type": "repeat", "times": 3, "steps": [{"type": "delay", "seconds": 1}]},
               {"type": "wait_until", "conditions": [{"type": "triggered", "ids": ["b2"]}], "timeout_s": 60}]
        }))
        .unwrap();
        assign_ids(&mut input.triggers);
        let ids: Vec<&str> = input.triggers.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["t1", "b2", "t2", "t3"]);
        assert_eq!(input.mode, RunMode::Queued);
        assert!(validate(&input).is_ok(), "{:?}", validate(&input));
        // Retour en JSON : les « Quand » gardent leur forme à plat.
        let back = serde_json::to_value(&input.triggers[0]).unwrap();
        assert_eq!(back["type"], "device");
        assert_eq!(back["name"], "Bouton 1");
        assert_eq!(
            serde_json::to_value(&input.triggers[2]).unwrap(),
            json!({"type": "startup", "id": "t2"})
        );
        // Une branche vers un « Quand » inconnu est refusée.
        input.actions[0] = serde_json::from_value(
            json!({"type": "choose", "branches": [{"when": ["zz"], "steps": []}]}),
        )
        .unwrap();
        assert!(validate(&input).is_err());
    }

    #[test]
    fn old_automations_still_load() {
        let a: Automation = serde_json::from_value(json!({
            "id": "x", "name": "x",
            "triggers": [{"type": "state", "entity_id": "binary_sensor.porte", "to": "on"}],
            "actions": [{"type": "command", "entity_id": "light.a", "action": "turn_on", "brightness": 100}]
        }))
        .unwrap();
        assert_eq!(a.mode, RunMode::Restart);
        assert!(a.triggers[0].id.is_empty());
        assert!(matches!(a.triggers[0].trigger, Trigger::State { .. }));
    }

    #[test]
    fn conditions_groups_and_trigger() {
        let mut states = HashMap::new();
        states.insert("person.a".to_string(), st(json!("away")));
        states.insert("sensor.t".to_string(), st(json!(21.5)));
        let names = HashMap::new();
        let env = Env {
            states: &states,
            names: &names,
            now: Local::now(),
            location: Location::default(),
            trigger: Some("t2"),
            alarm: Some("pending"),
        };
        let c = |v: Value| -> Condition { serde_json::from_value(v).unwrap() };
        assert!(conditions_ok(
            &[c(json!({"type": "triggered", "ids": ["t1", "t2"]}))],
            &env
        ));
        assert!(!conditions_ok(
            &[c(json!({"type": "triggered", "ids": ["t1"]}))],
            &env
        ));
        assert!(conditions_ok(
            &[c(json!({"type": "presence", "home": false}))],
            &env
        ));
        assert!(conditions_ok(
            &[c(json!({"type": "alarm", "is": "armed"}))],
            &env
        ));
        assert!(conditions_ok(
            &[c(
                json!({"type": "numeric", "entity_id": "sensor.t", "above": 20})
            )],
            &env
        ));
        assert!(conditions_ok(
            &[c(json!({"type": "any", "conditions": [
                {"type": "presence", "home": true}, {"type": "numeric", "entity_id": "sensor.t", "below": 22}
            ]}))],
            &env
        ));
        assert!(!conditions_ok(
            &[c(
                json!({"type": "not", "conditions": [{"type": "presence", "home": false}]})
            )],
            &env
        ));
    }

    #[test]
    fn presence_and_device_and_interval() {
        let ev = presence_events("person.a", Some(&st(json!("away"))), &st(json!("home")), 1);
        assert_eq!(ev, vec![PresenceEvent::Arrive, PresenceEvent::FirstHome]);
        let ev = presence_events("person.a", Some(&st(json!("home"))), &st(json!("away")), 0);
        assert_eq!(ev, vec![PresenceEvent::Leave, PresenceEvent::LastAway]);
        let any = Trigger::Device {
            device_id: "0x01".into(),
            event: None,
        };
        let one = Trigger::Device {
            device_id: "0x01".into(),
            event: Some("1_single".into()),
        };
        assert!(device_hit(&any, "0x01", "2_double"));
        assert!(device_hit(&one, "0x01", "1_single"));
        assert!(!device_hit(&one, "0x01", "1_double"));
        assert!(!device_hit(&one, "0x02", "1_single"));
        assert!(interval_matches(15, t(10, 45)));
        assert!(!interval_matches(15, t(10, 46)));
    }

    #[test]
    fn from_state_and_variables() {
        let tr = Trigger::State {
            entity_id: "light.a".into(),
            from: Some(json!("off")),
            to: Some(json!("on")),
            for_s: None,
        };
        assert_eq!(
            on_state_change(&tr, "light.a", Some(&st(json!("off"))), &st(json!("on"))),
            Hit::Now
        );
        assert_eq!(
            on_state_change(
                &tr,
                "light.a",
                Some(&st(json!("unavailable"))),
                &st(json!("on"))
            ),
            Hit::No
        );
        let ctx = Ctx {
            trigger: Some("Bouton 1".into()),
            entity_id: Some("light.a".into()),
            to: Some(json!("on")),
            ..Default::default()
        };
        let mut names = HashMap::new();
        names.insert("light.a".to_string(), "Salon".to_string());
        assert_eq!(
            fill("{trigger} : {entity} {value}", &ctx, &names),
            "Bouton 1 : Salon allumé"
        );
    }
}
