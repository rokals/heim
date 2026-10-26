//! Journal et historique léger, dans une base à part (`history.db`).
//!
//! - **Journal** : tout ce qui se passe — une lampe allumée (et par qui), une porte ouverte,
//!   un automatisme lancé ou bloqué (et pourquoi), l'alarme, une intégration qui décroche,
//!   un redémarrage. 30 jours.
//! - **Courbes** : les valeurs numériques des capteurs (température, humidité, puissance…),
//!   un point toutes les 5 minutes au plus par capteur. 10 jours.
//!
//! Pour ménager la carte SD, tout est gardé en mémoire et écrit par paquets (30 s), dans un
//! fichier séparé de la base principale : les sauvegardes restent petites et une base
//! d'historique abîmée n'empêche jamais Heim de démarrer (elle est simplement recréée).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Category, Domain, EntityId, EntityView, State};
use crate::registry::RegistryHandle;

const FLUSH_EVERY: Duration = Duration::from_secs(30);
const SAMPLE_EVERY_S: i64 = 300;
const LOG_DAYS: i64 = 30;
const SAMPLE_DAYS: i64 = 10;
/// Une commande et le changement d'état qui la suit : au plus tant de secondes d'écart.
const ACTOR_WINDOW_S: i64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    #[serde(default)]
    pub id: i64,
    pub ts: DateTime<Utc>,
    /// state, automation, security, integration, system, account
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Nouvelle valeur pour un changement d'état (l'interface la met en mots).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Qui ou quoi : « Alex », « automatisme Soir », « minuteur »…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    pub entity_id: Option<String>,
    pub kind: Option<String>,
    pub q: Option<String>,
    /// Plus ancien que cet identifiant (pagination).
    pub before: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Point {
    pub t: i64,
    pub v: f64,
}

struct Pending {
    logs: Vec<LogEntry>,
    samples: Vec<(String, i64, f64)>,
}

/// Lecture (API) et écriture (tâche de fond) partagent la même connexion.
#[derive(Clone)]
pub struct History {
    conn: Arc<Mutex<Option<Connection>>>,
    pending: Arc<Mutex<Pending>>,
    path: PathBuf,
}

fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS log (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             ts INTEGER NOT NULL,
             kind TEXT NOT NULL,
             entity_id TEXT,
             name TEXT,
             value TEXT,
             message TEXT,
             actor TEXT
         );
         CREATE INDEX IF NOT EXISTS log_ts ON log(ts);
         CREATE INDEX IF NOT EXISTS log_entity ON log(entity_id, ts);
         CREATE TABLE IF NOT EXISTS samples (
             entity_id TEXT NOT NULL,
             ts INTEGER NOT NULL,
             v REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS samples_entity ON samples(entity_id, ts);",
    )?;
    Ok(conn)
}

impl History {
    pub fn open(path: &Path) -> Self {
        let conn = match open_db(path) {
            Ok(c) => Some(c),
            Err(e) => {
                // Base d'historique abîmée : on la met de côté et on repart à neuf.
                tracing::warn!("historique illisible ({e}) : nouvelle base");
                let _ = std::fs::rename(path, path.with_extension("db.broken"));
                open_db(path)
                    .map_err(|e| tracing::error!("historique indisponible : {e}"))
                    .ok()
            }
        };
        Self {
            conn: Arc::new(Mutex::new(conn)),
            pending: Arc::new(Mutex::new(Pending {
                logs: Vec::new(),
                samples: Vec::new(),
            })),
            path: path.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn push_log(&self, entry: LogEntry) {
        if let Ok(mut p) = self.pending.lock() {
            p.logs.push(entry);
        }
    }

    fn push_sample(&self, entity_id: &str, ts: i64, v: f64) {
        if let Ok(mut p) = self.pending.lock() {
            p.samples.push((entity_id.to_string(), ts, v));
        }
    }

    /// Écrit ce qui attend. Bloquant (appelé depuis spawn_blocking ou à l'arrêt).
    pub fn flush(&self) {
        let (logs, samples) = {
            let Ok(mut p) = self.pending.lock() else {
                return;
            };
            (std::mem::take(&mut p.logs), std::mem::take(&mut p.samples))
        };
        if logs.is_empty() && samples.is_empty() {
            return;
        }
        let Ok(mut guard) = self.conn.lock() else {
            return;
        };
        let Some(conn) = guard.as_mut() else {
            return;
        };
        let result = (|| -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            {
                let mut ins = tx.prepare_cached(
                    "INSERT INTO log (ts, kind, entity_id, name, value, message, actor)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for l in &logs {
                    ins.execute(params![
                        l.ts.timestamp_millis(),
                        l.kind,
                        l.entity_id,
                        l.name,
                        l.value.as_ref().map(Value::to_string),
                        l.message,
                        l.actor
                    ])?;
                }
                let mut ins = tx
                    .prepare_cached("INSERT INTO samples (entity_id, ts, v) VALUES (?1, ?2, ?3)")?;
                for (id, ts, v) in &samples {
                    ins.execute(params![id, ts, v])?;
                }
            }
            tx.commit()
        })();
        if let Err(e) = result {
            tracing::warn!("historique : écriture impossible : {e}");
        }
    }

    fn prune(&self) {
        let Ok(guard) = self.conn.lock() else {
            return;
        };
        let Some(conn) = guard.as_ref() else {
            return;
        };
        let now = Utc::now();
        let log_limit = (now - TimeDelta::days(LOG_DAYS)).timestamp_millis();
        let sample_limit = (now - TimeDelta::days(SAMPLE_DAYS)).timestamp();
        let _ = conn.execute("DELETE FROM log WHERE ts < ?1", params![log_limit]);
        let _ = conn.execute("DELETE FROM samples WHERE ts < ?1", params![sample_limit]);
    }

    /// Les entrées les plus récentes d'abord, y compris celles pas encore écrites.
    pub fn query(&self, q: &LogQuery) -> Vec<LogEntry> {
        self.flush();
        let limit = q.limit.unwrap_or(100).clamp(1, 500);
        let Ok(guard) = self.conn.lock() else {
            return Vec::new();
        };
        let Some(conn) = guard.as_ref() else {
            return Vec::new();
        };
        let mut sql = String::from(
            "SELECT id, ts, kind, entity_id, name, value, message, actor FROM log WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(e) = &q.entity_id {
            sql.push_str(" AND entity_id = ?");
            args.push(Box::new(e.clone()));
        }
        if let Some(k) = q.kind.as_ref().filter(|k| !k.is_empty()) {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(k.clone()));
        }
        if let Some(text) = q.q.as_ref().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            sql.push_str(
                " AND (name LIKE ? OR message LIKE ? OR actor LIKE ? OR entity_id LIKE ?)",
            );
            let like = format!("%{text}%");
            for _ in 0..4 {
                args.push(Box::new(like.clone()));
            }
        }
        if let Some(b) = q.before {
            sql.push_str(" AND id < ?");
            args.push(Box::new(b));
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        args.push(Box::new(limit as i64));
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return Vec::new();
        };
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        stmt.query_map(refs.as_slice(), |r| {
            let ts: i64 = r.get(1)?;
            let value: Option<String> = r.get(5)?;
            Ok(LogEntry {
                id: r.get(0)?,
                ts: DateTime::from_timestamp_millis(ts).unwrap_or_default(),
                kind: r.get(2)?,
                entity_id: r.get(3)?,
                name: r.get(4)?,
                value: value.and_then(|v| serde_json::from_str(&v).ok()),
                message: r.get(6)?,
                actor: r.get(7)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Points d'une courbe sur les `hours` dernières heures, ramenés à `max_points` au plus
    /// (moyenne par tranche) pour qu'un téléphone les dessine sans peine.
    pub fn series(&self, entity_id: &str, hours: i64, max_points: usize) -> Vec<Point> {
        self.flush();
        let since = (Utc::now() - TimeDelta::hours(hours.clamp(1, 24 * SAMPLE_DAYS))).timestamp();
        let Ok(guard) = self.conn.lock() else {
            return Vec::new();
        };
        let Some(conn) = guard.as_ref() else {
            return Vec::new();
        };
        let Ok(mut stmt) =
            conn.prepare("SELECT ts, v FROM samples WHERE entity_id = ?1 AND ts >= ?2 ORDER BY ts")
        else {
            return Vec::new();
        };
        let raw: Vec<Point> = stmt
            .query_map(params![entity_id, since], |r| {
                Ok(Point {
                    t: r.get(0)?,
                    v: r.get(1)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        downsample(&raw, max_points.max(2))
    }
}

pub fn downsample(points: &[Point], max: usize) -> Vec<Point> {
    if points.len() <= max {
        return points.to_vec();
    }
    let chunk = points.len().div_ceil(max);
    points
        .chunks(chunk)
        .map(|c| Point {
            t: c[c.len() / 2].t,
            v: (c.iter().map(|p| p.v).sum::<f64>() / c.len() as f64 * 100.0).round() / 100.0,
        })
        .collect()
}

/// Les entités dont chaque changement mérite une ligne de journal.
fn logged(domain: Domain) -> bool {
    !matches!(domain, Domain::Sensor | Domain::Timer)
}

/// Ce qui alimente les courbes : un capteur numérique principal, ou la pile.
fn sampled(view: &Meta, value: &Value) -> Option<f64> {
    if view.domain != Domain::Sensor {
        return None;
    }
    value.as_f64()
}

struct Meta {
    name: String,
    domain: Domain,
    category: Category,
}

struct Recorder {
    history: History,
    meta: HashMap<EntityId, Meta>,
    /// Dernière commande reçue par entité : qui, et quand.
    actors: HashMap<EntityId, (String, DateTime<Utc>)>,
    /// Dernier point enregistré et valeur en attente, par capteur.
    last_sample: HashMap<EntityId, (i64, Option<f64>)>,
    integrations: HashMap<String, IntegrationState>,
    automations: HashMap<String, String>,
    security: Option<String>,
}

/// Démarre l'enregistreur : il écoute le bus et écrit par paquets.
pub fn spawn(history: History, bus: &Bus, registry: RegistryHandle) {
    let mut rx = bus.subscribe();
    let writer = history.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(FLUSH_EVERY);
        let mut prune = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let h = writer.clone();
                    let _ = tokio::task::spawn_blocking(move || h.flush()).await;
                }
                _ = prune.tick() => {
                    let h = writer.clone();
                    let _ = tokio::task::spawn_blocking(move || h.prune()).await;
                }
            }
        }
    });
    tokio::spawn(async move {
        let mut rec = Recorder {
            history,
            meta: HashMap::new(),
            actors: HashMap::new(),
            last_sample: HashMap::new(),
            integrations: HashMap::new(),
            automations: HashMap::new(),
            security: None,
        };
        for e in registry.snapshot().await {
            rec.learn(&e);
        }
        loop {
            match rx.recv().await {
                Ok(ev) => rec.on_event(ev),
                Err(RecvError::Lagged(n)) => tracing::debug!("journal : {n} événements perdus"),
                Err(RecvError::Closed) => break,
            }
        }
    });
}

impl Recorder {
    fn learn(&mut self, e: &EntityView) {
        if e.meta.domain == Domain::Automation
            && let Some(id) = e.meta.entity_id.strip_prefix("automation.")
        {
            self.automations.insert(id.to_string(), e.meta.name.clone());
        }
        self.meta.insert(
            e.meta.entity_id.clone(),
            Meta {
                name: e.meta.name.clone(),
                domain: e.meta.domain,
                category: e.meta.category,
            },
        );
    }

    /// « automation:soir » → « automatisme Soir », « api » → rien de parlant.
    fn actor_name(&self, source: &str) -> Option<String> {
        if let Some(id) = source.strip_prefix("automation:") {
            let name = self
                .automations
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string());
            return Some(format!("automatisme {name}"));
        }
        if let Some(id) = source.strip_prefix("timer:") {
            return Some(format!("minuteur {id}"));
        }
        if let Some(id) = source.strip_prefix("scene:") {
            let name = self
                .meta
                .get(&format!("scene.{id}"))
                .map(|m| m.name.clone())
                .unwrap_or_else(|| id.to_string());
            return Some(format!("scène {name}"));
        }
        if let Some(name) = source.strip_prefix("user:") {
            return Some(name.to_string());
        }
        match source {
            "api" | "ws" | "" => None,
            other => Some(other.to_string()),
        }
    }

    fn entry(&self, kind: &str, entity_id: Option<&str>) -> LogEntry {
        LogEntry {
            id: 0,
            ts: Utc::now(),
            kind: kind.to_string(),
            entity_id: entity_id.map(str::to_string),
            name: entity_id.and_then(|id| self.meta.get(id).map(|m| m.name.clone())),
            value: None,
            message: None,
            actor: None,
        }
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::EntityUpdated { entity } => self.learn(&entity),
            Event::EntityRemoved { entity_id } => {
                self.meta.remove(&entity_id);
            }
            Event::CommandRequested {
                entity_id, source, ..
            }
            | Event::CommandNoted { entity_id, source } => {
                self.actors.insert(entity_id, (source, Utc::now()));
            }
            Event::StateChanged {
                entity_id,
                old,
                new,
            } => self.on_state(&entity_id, old.as_ref(), &new),
            Event::AutomationRan { id, ok, detail } => {
                let name = self.automations.get(&id).cloned().unwrap_or(id.clone());
                let mut e = self.entry("automation", Some(&format!("automation.{id}")));
                e.name = Some(name);
                e.message = Some(match (ok, detail) {
                    (true, _) => "lancé".to_string(),
                    (false, Some(d)) => format!("lancé, avec une erreur : {d}"),
                    (false, None) => "lancé, avec une erreur".to_string(),
                });
                self.history.push_log(e);
            }
            Event::AutomationSkipped { id, name, reason } => {
                let mut e = self.entry("automation", Some(&format!("automation.{id}")));
                e.name = Some(name);
                e.message = Some(format!("pas lancé : {reason}"));
                self.history.push_log(e);
            }
            Event::Security { status } => {
                let mode = status.get("mode").and_then(Value::as_str).unwrap_or("");
                let phase = status.get("phase").and_then(Value::as_str).unwrap_or("");
                let key = format!("{phase}:{mode}");
                if self.security.as_deref() == Some(key.as_str()) {
                    return;
                }
                let first = self.security.is_none();
                self.security = Some(key.clone());
                if first {
                    return;
                }
                let mut e = self.entry("security", None);
                e.name = Some("Alarme".into());
                e.value = Some(Value::String(key));
                e.message = status
                    .get("cause")
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str)
                    .map(|n| format!("par « {n} »"));
                self.history.push_log(e);
            }
            Event::IntegrationStatus {
                name,
                status,
                detail,
            } => {
                // Seulement les vrais changements (pas « Connexion… » à chaque essai).
                if status == IntegrationState::Starting
                    || self.integrations.insert(name.clone(), status) == Some(status)
                {
                    return;
                }
                let mut e = self.entry("integration", None);
                e.name = Some(name);
                e.value = serde_json::to_value(status).ok();
                e.message = detail;
                self.history.push_log(e);
            }
            Event::Log {
                kind,
                message,
                entity_id,
                actor,
            } => {
                let mut e = self.entry(&kind, entity_id.as_deref());
                e.message = Some(message);
                e.actor = actor;
                self.history.push_log(e);
            }
            _ => {}
        }
    }

    fn on_state(&mut self, entity_id: &str, old: Option<&State>, new: &State) {
        let Some(meta) = self.meta.get(entity_id) else {
            return;
        };
        let now = Utc::now();
        // Courbe : un point par tranche de 5 min, la dernière valeur de la tranche.
        if let Some(v) = sampled(meta, &new.value) {
            let ts = now.timestamp();
            let entry = self
                .last_sample
                .entry(entity_id.to_string())
                .or_insert((0, None));
            if ts - entry.0 >= SAMPLE_EVERY_S {
                self.history.push_sample(entity_id, ts, v);
                *entry = (ts, None);
            } else {
                entry.1 = Some(v);
            }
        }
        if meta.category == Category::Diagnostic || !logged(meta.domain) {
            return;
        }
        let became_unavailable = old.is_some_and(|o| o.available) && !new.available;
        let came_back = old.is_some_and(|o| !o.available) && new.available;
        let value_changed = old.is_none_or(|o| o.value != new.value);
        if !(value_changed || became_unavailable || came_back) || new.value.is_null() {
            return;
        }
        // Premier état connu après le démarrage : pas une nouvelle.
        if old.is_none() {
            return;
        }
        let mut e = self.entry("state", Some(entity_id));
        if became_unavailable {
            e.message = Some("ne répond plus".into());
        } else if came_back && !value_changed {
            e.message = Some("répond de nouveau".into());
        } else {
            e.value = Some(new.value.clone());
        }
        if let Some((source, at)) = self.actors.remove(entity_id)
            && (now - at).num_seconds() <= ACTOR_WINDOW_S
        {
            e.actor = self.actor_name(&source);
        }
        self.history.push_log(e);
    }
}

/// Termine les tranches de courbe en attente (à l'arrêt) et écrit tout.
pub fn shutdown(history: &History) {
    history.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("heim-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let h = History::open(&dir.join("history.db"));
        for i in 0..3 {
            h.push_log(LogEntry {
                id: 0,
                ts: Utc::now(),
                kind: "state".into(),
                entity_id: Some("light.salon".into()),
                name: Some("Salon".into()),
                value: Some(Value::String(if i % 2 == 0 { "on" } else { "off" }.into())),
                message: None,
                actor: Some("Alex".into()),
            });
        }
        h.push_sample("sensor.t", Utc::now().timestamp() - 60, 21.5);
        h.push_sample("sensor.t", Utc::now().timestamp(), 22.5);
        let all = h.query(&LogQuery::default());
        assert_eq!(all.len(), 3);
        assert!(all[0].id > all[1].id, "le plus récent d'abord");
        let found = h.query(&LogQuery {
            q: Some("ale".into()),
            ..Default::default()
        });
        assert_eq!(found.len(), 3);
        let page = h.query(&LogQuery {
            before: Some(all[0].id),
            ..Default::default()
        });
        assert_eq!(page.len(), 2);
        let pts = h.series("sensor.t", 24, 100);
        assert_eq!(pts.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn downsample_keeps_shape() {
        let pts: Vec<Point> = (0..1000).map(|i| Point { t: i, v: i as f64 }).collect();
        let d = downsample(&pts, 100);
        assert!(d.len() <= 100);
        assert!(d.first().unwrap().v < 10.0 && d.last().unwrap().v > 990.0);
    }
}
