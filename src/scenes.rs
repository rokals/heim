//! Scènes avec état.
//!
//! Une scène est un état voulu pour un ensemble de lumières, de prises et de groupes :
//! allumé ou éteint, et pour une lampe sa luminosité, son blanc ou sa couleur, son effet.
//! Seuls les réglages choisis comptent (une lampe « juste allumée » garde sa couleur).
//!
//! - Le moteur sait en permanence si la scène est **active** en comparant l'état voulu à l'état
//!   réel, avec une petite marge. Une lampe changée à la main la rend inactive toute seule.
//! - **Désactiver** éteint tout ce qu'elle contient, ou remet chaque appareil comme avant,
//!   au choix (`off_mode`).
//! - **Activer pour un temps** (« Ciné pendant 2 h ») : au bout du temps, retour à l'avant.
//! - **Aperçu en direct** pendant qu'on règle une scène : chaque changement s'applique sur la
//!   vraie lampe, puis tout revient comme avant à la sortie de l'éditeur.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};

use crate::bus::{Bus, Event};
use crate::devices::{self, Controls};
use crate::model::{Attributes, Domain, EntityId, EntityMeta, State, Target, slugify};
use crate::registry::RegistryHandle;
use crate::store::{StoreHandle, Table};

pub const INTEGRATION: &str = "scenes";
/// Un aperçu oublié (onglet fermé) se termine tout seul : les lampes reviennent comme avant.
const PREVIEW_IDLE: Duration = Duration::from_secs(10 * 60);
const MAX_TRANSITION_S: f64 = 60.0;
const MAX_FOR_MIN: u64 = 24 * 60;
/// Écart de couleur toléré (dans l'espace CIE xy) pour dire qu'une lampe « est » à sa couleur.
const COLOR_TOLERANCE: f64 = 0.03;

/// L'état voulu d'un appareil dans la scène. Un réglage absent (`None`) ne compte pas :
/// la scène n'y touche pas, et ne le regarde pas pour savoir si elle est active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneTarget {
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Blanc, en mireds (comme Zigbee2MQTT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temp: Option<u16>,
    /// Couleur, « #rrggbb ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
}

impl SceneTarget {
    pub fn on() -> Self {
        Self {
            on: true,
            brightness: None,
            color_temp: None,
            color: None,
            effect: None,
        }
    }

    pub fn off() -> Self {
        Self {
            on: false,
            ..Self::on()
        }
    }

    /// La partie que toutes les intégrations comprennent (allumé, luminosité, blanc).
    pub fn basic(&self) -> Target {
        Target {
            on: self.on,
            brightness: if self.on { self.brightness } else { None },
            color_temp: if self.on && self.color.is_none() {
                self.color_temp
            } else {
                None
            },
        }
    }
}

impl From<Target> for SceneTarget {
    fn from(t: Target) -> Self {
        Self {
            on: t.on,
            brightness: t.brightness,
            color_temp: t.color_temp,
            color: None,
            effect: None,
        }
    }
}

/// Que fait « désactiver » ?
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffMode {
    /// Tout ce qui est dans la scène s'éteint.
    #[default]
    TurnOff,
    /// Chaque appareil retrouve l'état qu'il avait avant la scène.
    Restore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    pub targets: BTreeMap<EntityId, SceneTarget>,
    /// Icône et couleur de la scène sur l'accueil.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Fondu à l'activation, en secondes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_s: Option<f64>,
    #[serde(default)]
    pub off_mode: OffMode,
    /// État d'avant l'activation, pour « revenir comme avant ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<BTreeMap<EntityId, SceneTarget>>,
    /// Activée pour un temps : retour à l'avant à cette heure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<DateTime<Utc>>,
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
    pub targets: BTreeMap<EntityId, SceneTarget>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub transition_s: Option<f64>,
    #[serde(default)]
    pub off_mode: OffMode,
}

impl SceneInput {
    pub fn simple(name: &str, targets: BTreeMap<EntityId, SceneTarget>) -> Self {
        Self {
            name: name.into(),
            room: None,
            targets,
            icon: None,
            color: None,
            transition_s: None,
            off_mode: OffMode::default(),
        }
    }
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
    /// L'état actuel de ces appareils, sans rien enregistrer (« Reprendre l'état actuel »).
    Snapshot(
        Vec<EntityId>,
        oneshot::Sender<BTreeMap<EntityId, SceneTarget>>,
    ),
    Delete(String, oneshot::Sender<bool>),
    Duplicate(String, oneshot::Sender<Result<SceneView, String>>),
    ActivateFor(String, Option<u64>, oneshot::Sender<Result<(), String>>),
    Preview(
        BTreeMap<EntityId, SceneTarget>,
        Option<f64>,
        oneshot::Sender<Result<(), String>>,
    ),
    PreviewEnd(oneshot::Sender<usize>),
}

#[derive(Clone)]
pub struct SceneHandle {
    tx: mpsc::Sender<Msg>,
}

const STOPPED: &str = "moteur de scènes arrêté";

impl SceneHandle {
    async fn call<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Result<T, String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(make(tx)).await.map_err(|_| STOPPED)?;
        rx.await.map_err(|_| STOPPED.to_string())
    }

    pub async fn list(&self) -> Vec<SceneView> {
        self.call(Msg::List).await.unwrap_or_default()
    }

    pub async fn upsert(&self, id: &str, input: SceneInput) -> Result<SceneView, String> {
        let id = id.to_string();
        self.call(|tx| Msg::Upsert(id, input, tx)).await?
    }

    pub async fn capture(&self, id: &str, input: CaptureInput) -> Result<SceneView, String> {
        let id = id.to_string();
        self.call(|tx| Msg::Capture(id, input, tx)).await?
    }

    pub async fn snapshot(&self, entities: Vec<EntityId>) -> BTreeMap<EntityId, SceneTarget> {
        self.call(|tx| Msg::Snapshot(entities, tx))
            .await
            .unwrap_or_default()
    }

    pub async fn delete(&self, id: &str) -> bool {
        let id = id.to_string();
        self.call(|tx| Msg::Delete(id, tx)).await.unwrap_or(false)
    }

    pub async fn duplicate(&self, id: &str) -> Result<SceneView, String> {
        let id = id.to_string();
        self.call(|tx| Msg::Duplicate(id, tx)).await?
    }

    /// Active la scène, éventuellement pour `minutes` seulement.
    pub async fn activate_for(&self, id: &str, minutes: Option<u64>) -> Result<(), String> {
        let id = id.to_string();
        self.call(|tx| Msg::ActivateFor(id, minutes, tx)).await?
    }

    pub async fn preview(
        &self,
        targets: BTreeMap<EntityId, SceneTarget>,
        transition_s: Option<f64>,
    ) -> Result<(), String> {
        self.call(|tx| Msg::Preview(targets, transition_s, tx))
            .await?
    }

    /// Fin de l'aperçu : chaque appareil touché revient comme avant. Renvoie leur nombre.
    pub async fn preview_end(&self) -> usize {
        self.call(Msg::PreviewEnd).await.unwrap_or(0)
    }
}

struct Preview {
    /// État de chaque appareil avant son premier réglage en aperçu.
    before: BTreeMap<EntityId, SceneTarget>,
    last: Instant,
}

struct Engine {
    scenes: BTreeMap<String, Scene>,
    status: HashMap<String, SceneStatus>,
    /// Dernier état connu des entités (léger : quelques dizaines d'appareils).
    cache: HashMap<EntityId, State>,
    meta: HashMap<EntityId, (String, Option<String>)>,
    registry: RegistryHandle,
    store: StoreHandle,
    bus: Bus,
    controls: Controls,
    preview: Option<Preview>,
}

pub fn spawn(
    scenes: Vec<Scene>,
    registry: RegistryHandle,
    bus: Bus,
    store: StoreHandle,
    controls: Controls,
) -> SceneHandle {
    let (tx, rx) = mpsc::channel(64);
    // On s'abonne avant de lire l'instantané : aucun changement ne peut passer entre les deux.
    let bus_rx = bus.subscribe();
    let engine = Engine {
        scenes: scenes.into_iter().map(|s| (s.id.clone(), s)).collect(),
        status: HashMap::new(),
        cache: HashMap::new(),
        meta: HashMap::new(),
        registry,
        store,
        bus,
        controls,
        preview: None,
    };
    tokio::spawn(engine.run(rx, bus_rx));
    SceneHandle { tx }
}

// ---- Couleurs : « #rrggbb » <-> CIE xy (ce que parlent les ampoules Zigbee) ---------------

fn hex_rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.0)
    };
    Some((p(0)?, p(2)?, p(4)?))
}

pub fn valid_hex(hex: &str) -> bool {
    hex.starts_with('#') && hex_rgb(hex).is_some()
}

/// sRGB -> CIE xy (gamut large, D65), comme le font les ponts Hue et Zigbee2MQTT.
pub fn hex_to_xy(hex: &str) -> Option<(f64, f64)> {
    let (r, g, b) = hex_rgb(hex)?;
    let lin = |c: f64| {
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let x = r * 0.664_511 + g * 0.154_324 + b * 0.162_028;
    let y = r * 0.283_881 + g * 0.668_433 + b * 0.047_685;
    let z = r * 0.000_088 + g * 0.072_310 + b * 0.986_039;
    let sum = x + y + z;
    if sum <= 0.0 {
        return None;
    }
    Some((x / sum, y / sum))
}

/// CIE xy -> « #rrggbb » (couleur la plus vive, sans la luminosité).
pub fn xy_to_hex(x: f64, y: f64) -> Option<String> {
    if x <= 0.0 || y <= 0.0 {
        return None;
    }
    let z = 1.0 - x - y;
    let big_y = 1.0;
    let big_x = (big_y / y) * x;
    let big_z = (big_y / y) * z;
    let r = big_x * 1.656_492 - big_y * 0.354_851 - big_z * 0.255_038;
    let g = -big_x * 0.707_196 + big_y * 1.655_397 + big_z * 0.036_152;
    let b = big_x * 0.051_713 - big_y * 0.121_364 + big_z * 1.011_530;
    let m = r.max(g).max(b).max(1e-6);
    let gamma = |c: f64| {
        let c = (c / m).max(0.0);
        let v = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (v.min(1.0) * 255.0).round() as u8
    };
    Some(format!("#{:02x}{:02x}{:02x}", gamma(r), gamma(g), gamma(b)))
}

fn hs_to_hex(hue: f64, saturation: f64) -> String {
    let s = (saturation / 100.0).clamp(0.0, 1.0);
    let f = |n: f64| {
        let k = (n + hue / 60.0) % 6.0;
        1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    let c = |v: f64| (v * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(f(5.0)), c(f(3.0)), c(f(1.0)))
}

/// La couleur actuelle d'une lampe, d'après ce que l'appareil a dit (xy ou teinte/saturation),
/// seulement si elle est en mode couleur (et non en blanc).
fn current_color(values: &Map<String, Value>) -> Option<String> {
    let mode = values.get("color_mode").and_then(Value::as_str);
    if mode == Some("color_temp") {
        return None;
    }
    let c = values.get("color")?;
    if let (Some(x), Some(y)) = (
        c.get("x").and_then(Value::as_f64),
        c.get("y").and_then(Value::as_f64),
    ) {
        return xy_to_hex(x, y);
    }
    if let Some(h) = c.get("hue").and_then(Value::as_f64) {
        let s = c.get("saturation").and_then(Value::as_f64).unwrap_or(100.0);
        return Some(hs_to_hex(h, s));
    }
    None
}

fn color_distance(a: &str, b: &str) -> Option<f64> {
    let (ax, ay) = hex_to_xy(a)?;
    let (bx, by) = hex_to_xy(b)?;
    Some(((ax - bx).powi(2) + (ay - by).powi(2)).sqrt())
}

/// L'appareil correspond-il à ce que veut la scène ? Seuls les réglages choisis comptent,
/// avec une marge (une ampoule renvoie rarement exactement la valeur demandée).
pub fn target_matches(t: &SceneTarget, state: &State, values: Option<&Map<String, Value>>) -> bool {
    if !t.basic().matches(state) {
        return false;
    }
    if !t.on {
        return true;
    }
    if let (Some(want), Some(values)) = (&t.color, values) {
        match values.get("color_mode").and_then(Value::as_str) {
            // On voulait une couleur, la lampe est passée en blanc : plus la scène.
            Some("color_temp") => return false,
            _ => {
                if let Some(have) = current_color(values)
                    && color_distance(want, &have).is_some_and(|d| d > COLOR_TOLERANCE)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn compute_status(
    scene: &Scene,
    cache: &HashMap<EntityId, State>,
    values: &dyn Fn(&str) -> Option<Map<String, Value>>,
) -> (SceneStatus, usize) {
    let (mut matched, mut mismatched, mut unavailable) = (0usize, 0usize, 0usize);
    for (id, target) in &scene.targets {
        match cache.get(id) {
            Some(state) if state.available => {
                if target_matches(target, state, values(id).as_ref()) {
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

fn validate(input: &SceneInput) -> Result<(), String> {
    if input.targets.is_empty() {
        return Err("une scène doit contenir au moins un appareil".into());
    }
    if input.targets.len() > 60 {
        return Err("60 appareils au plus dans une scène".into());
    }
    validate_targets(&input.targets)?;
    if let Some(t) = input.transition_s
        && !(0.0..=MAX_TRANSITION_S).contains(&t)
    {
        return Err("fondu invalide (60 secondes au plus)".into());
    }
    if input.color.as_deref().is_some_and(|c| !valid_hex(c)) {
        return Err("couleur de la scène invalide".into());
    }
    if input.icon.as_deref().is_some_and(|i| i.len() > 32) {
        return Err("icône invalide".into());
    }
    Ok(())
}

fn validate_targets(targets: &BTreeMap<EntityId, SceneTarget>) -> Result<(), String> {
    for (id, t) in targets {
        if !(id.starts_with("light.") || id.starts_with("switch.") || id.starts_with("group.")) {
            return Err(format!(
                "{id} : seules les lumières, les prises et les groupes peuvent faire partie d'une scène"
            ));
        }
        if t.color.as_deref().is_some_and(|c| !valid_hex(c)) {
            return Err(format!("{id} : couleur invalide (attendu #rrggbb)"));
        }
        if t.effect.as_deref().is_some_and(|e| e.len() > 40) {
            return Err(format!("{id} : effet invalide"));
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
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => break,
                },
                _ = tick.tick() => self.on_tick().await,
                ev = bus_rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, new, .. }) => {
                        if entity_id.starts_with("scene.") {
                            continue;
                        }
                        self.cache.insert(entity_id.clone(), new);
                        self.refresh_members(&entity_id).await;
                    }
                    Ok(Event::EntityUpdated { entity }) => {
                        self.meta.insert(
                            entity.meta.entity_id.clone(),
                            (entity.meta.integration.clone(), entity.meta.device_id.clone()),
                        );
                    }
                    Ok(Event::EntityRemoved { entity_id }) => {
                        self.cache.remove(&entity_id);
                    }
                    Ok(Event::CommandRequested { integration, entity_id, target, source }) if integration == INTEGRATION => {
                        if let Some(id) = entity_id.strip_prefix("scene.") {
                            if target.on {
                                self.activate(id, None, &source).await;
                            } else {
                                self.deactivate(id, false).await;
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
        let all = self.registry.snapshot().await;
        self.meta = all
            .iter()
            .map(|e| {
                (
                    e.meta.entity_id.clone(),
                    (e.meta.integration.clone(), e.meta.device_id.clone()),
                )
            })
            .collect();
        self.cache = all
            .into_iter()
            .filter_map(|e| e.state.map(|s| (e.meta.entity_id, s)))
            .collect();
    }

    /// Réglages complets connus de l'appareil d'une entité (couleur, mode…).
    fn device_values(&self, entity_id: &str) -> Option<Map<String, Value>> {
        let (_, device) = self.meta.get(entity_id)?;
        let device = device.as_ref()?;
        devices::lock(&self.controls)
            .get(device)
            .map(|d| d.values.clone())
    }

    async fn refresh_members(&mut self, entity_id: &str) {
        let touched: Vec<String> = self
            .scenes
            .values()
            .filter(|s| s.targets.contains_key(entity_id))
            .map(|s| s.id.clone())
            .collect();
        for id in touched {
            self.refresh(&id, false).await;
        }
    }

    async fn on_tick(&mut self) {
        // Fin d'un « activer pour X min » : retour à l'avant.
        let now = Utc::now();
        let due: Vec<String> = self
            .scenes
            .values()
            .filter(|s| s.until.is_some_and(|u| u <= now))
            .map(|s| s.id.clone())
            .collect();
        for id in due {
            tracing::info!("scène {id} : fin du temps, retour à l'avant");
            self.deactivate(&id, true).await;
        }
        // Aperçu oublié.
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.last.elapsed() > PREVIEW_IDLE)
        {
            tracing::info!("aperçu de scène abandonné : retour à l'avant");
            self.end_preview().await;
        }
        // La couleur n'arrive pas par le registre : on recalcule de temps en temps.
        let ids: Vec<String> = self.scenes.keys().cloned().collect();
        for id in ids {
            self.refresh(&id, false).await;
        }
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
        if let Some(icon) = &scene.icon {
            capabilities.insert("icon".into(), json!(icon));
        }
        if let Some(color) = &scene.color {
            capabilities.insert("color".into(), json!(color));
        }
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
        let values = |eid: &str| self.device_values(eid);
        let (status, unavailable) = compute_status(scene, &self.cache, &values);
        let previous = self.status.insert(id.to_string(), status);
        if !force && previous == Some(status) {
            return;
        }
        // La scène vient de se « défaire » d'elle-même : l'état à restaurer et la minuterie
        // n'ont plus de sens.
        if status == SceneStatus::Off
            && matches!(previous, Some(SceneStatus::On | SceneStatus::Partial))
            && let Some(scene) = self.scenes.get_mut(id)
            && (scene.restore.is_some() || scene.until.is_some())
        {
            scene.restore = None;
            scene.until = None;
            self.store.put(Table::Scenes, id, scene);
        }
        let Some(scene) = self.scenes.get(id) else {
            return;
        };
        let mut attrs = Attributes::new();
        attrs.insert("partial".into(), json!(status == SceneStatus::Partial));
        attrs.insert("unavailable_members".into(), json!(unavailable));
        attrs.insert("until".into(), json!(scene.until));
        let value = if status == SceneStatus::Off {
            "off"
        } else {
            "on"
        };
        self.registry
            .report(scene.entity_id(), json!(value), attrs, true)
            .await;
    }

    /// L'état actuel d'un appareil, comme une cible de scène (avec sa couleur si on la connaît).
    fn target_from_state(&self, entity_id: &str) -> Option<SceneTarget> {
        let state = self.cache.get(entity_id).filter(|s| s.available)?;
        let mut t = SceneTarget::from(Target::from_state(state));
        if t.on
            && let Some(values) = self.device_values(entity_id)
            && let Some(color) = current_color(&values)
        {
            t.color = Some(color);
            t.color_temp = None;
        }
        Some(t)
    }

    /// Envoie une cible à un appareil. Une ampoule Zigbee reçoit tout en un seul message
    /// (couleur, effet, fondu compris) ; les autres reçoivent la partie commune.
    async fn send(&self, entity_id: &str, t: &SceneTarget, transition: Option<f64>, source: &str) {
        let zigbee = self.meta.get(entity_id).and_then(|(integration, device)| {
            (integration == crate::integrations::zigbee2mqtt::INTEGRATION)
                .then(|| device.clone())
                .flatten()
        });
        if let Some(device) = zigbee {
            let keys: Vec<String> = devices::lock(&self.controls)
                .get(&device)
                .map(|d| {
                    d.controls
                        .iter()
                        .filter_map(|c| c.get("key").and_then(Value::as_str).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            // Une ampoule simple (une seule commande « state ») : tout part d'un coup.
            if keys.iter().any(|k| k == "state") && entity_id.starts_with("light.") {
                let mut v = Map::new();
                v.insert("state".into(), json!(if t.on { "ON" } else { "OFF" }));
                if t.on {
                    if let Some(b) = t.brightness
                        && keys.iter().any(|k| k == "brightness")
                    {
                        // Heim compte de 0 à 255, Zigbee2MQTT de 0 à 254.
                        let z = (b as f64 * 254.0 / 255.0).round().max(1.0) as u64;
                        v.insert("brightness".into(), json!(z));
                    }
                    if let Some(c) = &t.color
                        && keys.iter().any(|k| k == "color")
                    {
                        v.insert("color".into(), json!({ "hex": c }));
                    } else if let Some(ct) = t.color_temp
                        && keys.iter().any(|k| k == "color_temp")
                    {
                        v.insert("color_temp".into(), json!(ct));
                    }
                    if let Some(e) = &t.effect
                        && keys.iter().any(|k| k == "effect")
                    {
                        v.insert("effect".into(), json!(e));
                    }
                }
                if let Some(tr) = transition.filter(|t| *t > 0.0) {
                    v.insert("transition".into(), json!(tr));
                }
                self.bus.publish(Event::DeviceSet {
                    integration: crate::integrations::zigbee2mqtt::INTEGRATION.into(),
                    device_id: device,
                    values: v,
                });
                // Le registre garde trace de qui a commandé (journal) et de l'état attendu.
                self.registry
                    .note_command(entity_id, t.basic(), source)
                    .await;
                return;
            }
        }
        self.registry.apply(entity_id, t.basic(), source).await;
    }

    async fn activate(&mut self, id: &str, minutes: Option<u64>, source: &str) {
        let already_on = matches!(
            self.status.get(id),
            Some(SceneStatus::On | SceneStatus::Partial)
        );
        if !self.scenes.contains_key(id) {
            return;
        }
        // On photographie l'avant, sauf si la scène était déjà active (on garderait la scène).
        let snapshot: Option<BTreeMap<EntityId, SceneTarget>> = (!already_on).then(|| {
            self.scenes[id]
                .targets
                .keys()
                .filter_map(|eid| self.target_from_state(eid).map(|t| (eid.clone(), t)))
                .collect()
        });
        let scene = self.scenes.get_mut(id).expect("vérifié plus haut");
        if let Some(s) = snapshot {
            scene.restore = Some(s);
        }
        scene.until = minutes.map(|m| Utc::now() + TimeDelta::minutes(m as i64));
        self.store.put(Table::Scenes, id, scene);
        let scene = scene.clone();
        let source = if source.is_empty() || source == "api" {
            format!("scene:{id}")
        } else {
            source.to_string()
        };
        tracing::info!("activation de la scène {id}");
        for (eid, target) in &scene.targets {
            self.send(eid, target, scene.transition_s, &source).await;
        }
        self.refresh(id, true).await;
    }

    /// `back` : revenir à l'avant quoi qu'il arrive (fin d'un « activer pour X min »).
    async fn deactivate(&mut self, id: &str, back: bool) {
        let active = matches!(
            self.status.get(id),
            Some(SceneStatus::On | SceneStatus::Partial)
        );
        let Some(scene) = self.scenes.get_mut(id) else {
            return;
        };
        let restore = scene.restore.take();
        scene.until = None;
        self.store.put(Table::Scenes, id, scene);
        let scene = scene.clone();
        if !active {
            // Scène déjà inactive : on n'éteint rien par surprise.
            self.refresh(id, true).await;
            return;
        }
        let restore_mode = back || scene.off_mode == OffMode::Restore;
        let plan: Vec<(EntityId, SceneTarget)> = match (restore_mode, restore) {
            (true, Some(r)) => r.into_iter().collect(),
            _ => scene
                .targets
                .keys()
                .map(|k| (k.clone(), SceneTarget::off()))
                .collect(),
        };
        tracing::info!("désactivation de la scène {id}");
        let source = format!("scene:{id}");
        for (eid, target) in plan {
            self.send(&eid, &target, scene.transition_s, &source).await;
        }
        self.refresh(id, true).await;
    }

    async fn end_preview(&mut self) -> usize {
        let Some(p) = self.preview.take() else {
            return 0;
        };
        let n = p.before.len();
        for (eid, t) in p.before {
            self.send(&eid, &t, None, "aperçu de scène").await;
        }
        n
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
                let _ = reply.send(self.upsert(raw_id, input).await);
            }
            Msg::Capture(raw_id, input, reply) => {
                let mut targets = BTreeMap::new();
                for eid in input.entities {
                    match self.target_from_state(&eid) {
                        Some(t) => {
                            targets.insert(eid, t);
                        }
                        None => {
                            let _ = reply.send(Err(format!(
                                "{eid} est injoignable ou inconnue, impossible de la capturer"
                            )));
                            return;
                        }
                    }
                }
                let mut scene_input = SceneInput::simple(&input.name, targets);
                scene_input.room = input.room;
                let result = self.upsert(raw_id, scene_input).await;
                let _ = reply.send(result);
            }
            Msg::Snapshot(entities, reply) => {
                let out = entities
                    .into_iter()
                    .filter_map(|e| self.target_from_state(&e).map(|t| (e, t)))
                    .collect();
                let _ = reply.send(out);
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
            Msg::Duplicate(id, reply) => {
                let Some(src) = self.scenes.get(&id).cloned() else {
                    let _ = reply.send(Err(format!("scène inconnue : {id}")));
                    return;
                };
                let mut n = 2;
                let name = loop {
                    let candidate = format!("{} ({n})", src.name);
                    if !self.scenes.contains_key(&slugify(&candidate)) {
                        break candidate;
                    }
                    n += 1;
                };
                let input = SceneInput {
                    name,
                    room: src.room,
                    targets: src.targets,
                    icon: src.icon,
                    color: src.color,
                    transition_s: src.transition_s,
                    off_mode: src.off_mode,
                };
                let _ = reply.send(self.upsert(String::new(), input).await);
            }
            Msg::ActivateFor(id, minutes, reply) => {
                if !self.scenes.contains_key(&id) {
                    let _ = reply.send(Err(format!("scène inconnue : {id}")));
                    return;
                }
                if minutes.is_some_and(|m| m == 0 || m > MAX_FOR_MIN) {
                    let _ = reply.send(Err("durée invalide (24 h au plus)".into()));
                    return;
                }
                self.activate(&id, minutes, "").await;
                let _ = reply.send(Ok(()));
            }
            Msg::Preview(targets, transition, reply) => {
                if let Err(e) = validate_targets(&targets) {
                    let _ = reply.send(Err(e));
                    return;
                }
                // Le premier réglage d'un appareil en aperçu photographie son état d'avant.
                let mut before = self.preview.take().map(|p| p.before).unwrap_or_default();
                for eid in targets.keys() {
                    if !before.contains_key(eid)
                        && let Some(t) = self.target_from_state(eid)
                    {
                        before.insert(eid.clone(), t);
                    }
                }
                self.preview = Some(Preview {
                    before,
                    last: Instant::now(),
                });
                for (eid, t) in &targets {
                    self.send(eid, t, transition.or(Some(0.3)), "aperçu de scène")
                        .await;
                }
                let _ = reply.send(Ok(()));
            }
            Msg::PreviewEnd(reply) => {
                let n = self.end_preview().await;
                let _ = reply.send(n);
            }
        }
    }

    async fn upsert(&mut self, raw_id: String, input: SceneInput) -> Result<SceneView, String> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err("la scène doit avoir un nom".into());
        }
        validate(&input)?;
        let id = scene_id(&raw_id, &name)?;
        let previous = self.scenes.get(&id);
        let scene = Scene {
            id: id.clone(),
            name,
            room: input
                .room
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty()),
            targets: input.targets,
            icon: input.icon.filter(|i| !i.is_empty()),
            color: input.color.filter(|c| !c.is_empty()),
            transition_s: input.transition_s.filter(|t| *t > 0.0),
            off_mode: input.off_mode,
            restore: previous.and_then(|s| s.restore.clone()),
            until: previous.and_then(|s| s.until),
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
                .map(|m| (m.to_string(), SceneTarget::on()))
                .collect(),
            icon: None,
            color: None,
            transition_s: None,
            off_mode: OffMode::TurnOff,
            restore: None,
            until: None,
        }
    }

    #[test]
    fn status_rules() {
        let s = scene(&["light.a", "light.b"]);
        let none = |_: &str| None;
        let mut cache = HashMap::new();
        cache.insert("light.a".to_string(), st(true, true));
        cache.insert("light.b".to_string(), st(true, true));
        assert_eq!(compute_status(&s, &cache, &none).0, SceneStatus::On);

        cache.insert("light.b".to_string(), st(false, true));
        assert_eq!(compute_status(&s, &cache, &none).0, SceneStatus::Off);

        cache.insert("light.b".to_string(), st(true, false));
        assert_eq!(compute_status(&s, &cache, &none).0, SceneStatus::Partial);

        cache.insert("light.a".to_string(), st(true, false));
        assert_eq!(
            compute_status(&s, &cache, &none).0,
            SceneStatus::Off,
            "aucun membre joignable"
        );
    }

    #[test]
    fn colors_round_trip_closely() {
        for hex in ["#ff8800", "#3366ff", "#ff0000", "#00ff00"] {
            let (x, y) = hex_to_xy(hex).unwrap();
            let back = xy_to_hex(x, y).unwrap();
            assert!(
                color_distance(hex, &back).unwrap() < 0.01,
                "{hex} -> {back}"
            );
        }
        assert!(color_distance("#ff0000", "#0000ff").unwrap() > 0.3);
        assert!(!valid_hex("ff0000") && !valid_hex("#ff00") && valid_hex("#A1b2C3"));
    }

    #[test]
    fn color_matching_uses_the_device_values() {
        let mut t = SceneTarget::on();
        t.color = Some("#ff0000".into());
        let on = st(true, true);
        let (x, y) = hex_to_xy("#ff1000").unwrap();
        let mut v = Map::new();
        v.insert("color_mode".into(), json!("xy"));
        v.insert("color".into(), json!({"x": x, "y": y}));
        assert!(target_matches(&t, &on, Some(&v)), "rouge à peu près rouge");
        let (x, y) = hex_to_xy("#0000ff").unwrap();
        v.insert("color".into(), json!({"x": x, "y": y}));
        assert!(!target_matches(&t, &on, Some(&v)), "bleu n'est pas rouge");
        v.insert("color_mode".into(), json!("color_temp"));
        assert!(!target_matches(&t, &on, Some(&v)), "passée en blanc");
        assert!(
            target_matches(&t, &on, None),
            "couleur inconnue : on ne juge pas"
        );
    }

    #[test]
    fn old_scenes_still_load() {
        let old = json!({
            "id": "soir", "name": "Soir",
            "targets": {"light.a": {"on": true, "brightness": 90}},
            "restore": {"light.a": {"on": false}}
        });
        let s: Scene = serde_json::from_value(old).unwrap();
        assert_eq!(s.off_mode, OffMode::TurnOff);
        assert_eq!(s.targets["light.a"].brightness, Some(90));
    }
}
