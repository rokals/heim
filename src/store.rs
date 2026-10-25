//! Persistance SQLite.
//!
//! Lecture une seule fois au démarrage, puis écriture seule depuis un thread dédié.
//! Les changements d'état sont regroupés et écrits par lots (toutes les 3 s au plus),
//! pour ménager la carte SD. La configuration (scènes, timers...) est écrite tout de suite.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::model::{EntityId, EntityMeta, State};

const SCHEMA_VERSION: i64 = 3;
const FLUSH_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Scenes,
    Timers,
    NotifyTargets,
    /// Réglages modifiables depuis l'interface (connexions, lieu, tableau de bord...).
    Settings,
    Automations,
    Users,
    Sessions,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Scenes => "scenes",
            Table::Timers => "timers",
            Table::NotifyTargets => "notify_targets",
            Table::Settings => "settings",
            Table::Automations => "automations",
            Table::Users => "users",
            Table::Sessions => "sessions",
        }
    }

    const ALL: [Table; 7] = [
        Table::Scenes,
        Table::Timers,
        Table::NotifyTargets,
        Table::Settings,
        Table::Automations,
        Table::Users,
        Table::Sessions,
    ];
}

enum StoreMsg {
    Meta(EntityMeta),
    State(EntityId, State),
    DeleteEntity(EntityId),
    Put(Table, String, String),
    Delete(Table, String),
    Flush(mpsc::SyncSender<()>),
    Backup(std::path::PathBuf, mpsc::SyncSender<Result<(), String>>),
}

#[derive(Clone)]
pub struct StoreHandle {
    tx: mpsc::Sender<StoreMsg>,
}

impl StoreHandle {
    pub fn save_meta(&self, meta: EntityMeta) {
        let _ = self.tx.send(StoreMsg::Meta(meta));
    }

    pub fn save_state(&self, id: EntityId, state: State) {
        let _ = self.tx.send(StoreMsg::State(id, state));
    }

    pub fn delete_entity(&self, id: EntityId) {
        let _ = self.tx.send(StoreMsg::DeleteEntity(id));
    }

    pub fn put<T: Serialize>(&self, table: Table, id: &str, value: &T) {
        match serde_json::to_string(value) {
            Ok(json) => {
                let _ = self.tx.send(StoreMsg::Put(table, id.to_string(), json));
            }
            Err(e) => tracing::error!("store: sérialisation impossible pour {id}: {e}"),
        }
    }

    pub fn delete(&self, table: Table, id: &str) {
        let _ = self.tx.send(StoreMsg::Delete(table, id.to_string()));
    }

    /// Copie cohérente de la base dans `dest` (sauvegarde à chaud). Bloquant.
    pub fn backup_blocking(&self, dest: &Path) -> Result<(), String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.tx
            .send(StoreMsg::Backup(dest.to_path_buf(), ack_tx))
            .map_err(|_| "stockage arrêté".to_string())?;
        ack_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "sauvegarde trop longue".to_string())?
    }

    /// Force l'écriture de tout ce qui est en attente (arrêt propre). Bloquant.
    pub fn flush_blocking(&self) {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        if self.tx.send(StoreMsg::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

/// Ce qui a été lu au démarrage.
pub struct Loaded {
    pub entities: Vec<(EntityMeta, Option<State>)>,
    tables: HashMap<Table, Vec<String>>,
}

impl Loaded {
    /// Récupère les objets d'une table ; une ligne illisible est ignorée (et signalée), jamais fatale.
    pub fn take<T: DeserializeOwned>(&mut self, table: Table) -> Vec<T> {
        self.tables
            .remove(&table)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|json| match serde_json::from_str(&json) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("store: ligne illisible dans {} ignorée: {e}", table.name());
                    None
                }
            })
            .collect()
    }
}

/// Fichier déposé par une restauration : il remplace la base au prochain démarrage.
pub fn restore_path(db: &Path) -> std::path::PathBuf {
    db.with_extension("db.restore")
}

/// Une sauvegarde est-elle bien une base SQLite ? (en-tête « SQLite format 3 »)
pub fn looks_like_sqlite(bytes: &[u8]) -> bool {
    bytes.starts_with(b"SQLite format 3\0")
}

fn apply_pending_restore(path: &Path) -> anyhow::Result<()> {
    let restore = restore_path(path);
    if !restore.exists() {
        return Ok(());
    }
    if path.exists() {
        std::fs::rename(path, path.with_extension("db.bak"))?;
    }
    for ext in ["db-wal", "db-shm"] {
        let _ = std::fs::remove_file(path.with_extension(ext));
    }
    std::fs::rename(&restore, path)?;
    tracing::warn!("sauvegarde restaurée (l'ancienne base est gardée en heim.db.bak)");
    Ok(())
}

/// Le dossier des données (mots de passe hachés, clés Tuya, clés push, sauvegardes) n'est
/// lisible que par l'utilisateur qui fait tourner Heim. Rien à faire sous Windows.
pub fn private_dir(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            tracing::warn!("droits du dossier {} : {e}", dir.display());
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
}

pub fn open(path: &Path) -> anyhow::Result<(StoreHandle, Loaded)> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
        private_dir(dir);
    }
    apply_pending_restore(path)?;
    let conn =
        Connection::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    // Le délai d'attente d'abord : passer en WAL demande un verrou exclusif.
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -2000;",
    )?;
    migrate(&conn)?;
    let loaded = load(&conn)?;

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("heim-store".into())
        .stack_size(256 * 1024)
        .spawn(move || writer(conn, rx))?;
    Ok((StoreHandle { tx }, loaded))
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        anyhow::bail!("base créée par une version plus récente de Heim (schéma {version})");
    }
    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entities (
                entity_id TEXT PRIMARY KEY,
                meta      TEXT NOT NULL,
                state     TEXT
             );
             CREATE TABLE IF NOT EXISTS scenes (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS timers (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS notify_targets (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             PRAGMA user_version = 1;",
        )?;
    }
    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS automations (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             PRAGMA user_version = 2;",
        )?;
    }
    if version < 3 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             PRAGMA user_version = 3;",
        )?;
    }
    Ok(())
}

fn load(conn: &Connection) -> anyhow::Result<Loaded> {
    let mut entities = Vec::new();
    let mut stmt = conn.prepare("SELECT entity_id, meta, state FROM entities")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, meta, state) = row?;
        let meta: EntityMeta = match serde_json::from_str(&meta) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("store: entité {id} illisible, ignorée: {e}");
                continue;
            }
        };
        let state = state.and_then(|s| serde_json::from_str::<State>(&s).ok());
        entities.push((meta, state));
    }

    let mut tables = HashMap::new();
    for table in Table::ALL {
        let mut stmt = conn.prepare(&format!("SELECT data FROM {}", table.name()))?;
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        tables.insert(table, rows);
    }
    Ok(Loaded { entities, tables })
}

#[derive(Default)]
struct Pending {
    metas: HashMap<EntityId, EntityMeta>,
    states: HashMap<EntityId, State>,
    deleted: Vec<EntityId>,
    config: Vec<(Table, String, Option<String>)>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.metas.is_empty()
            && self.states.is_empty()
            && self.deleted.is_empty()
            && self.config.is_empty()
    }
}

fn writer(mut conn: Connection, rx: mpsc::Receiver<StoreMsg>) {
    let mut pending = Pending::default();
    let mut last_flush = Instant::now();
    loop {
        let wait = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
        let mut urgent = false;
        match rx.recv_timeout(wait) {
            Ok(StoreMsg::Meta(m)) => {
                pending.metas.insert(m.entity_id.clone(), m);
            }
            Ok(StoreMsg::State(id, s)) => {
                pending.states.insert(id, s);
            }
            Ok(StoreMsg::DeleteEntity(id)) => {
                pending.metas.remove(&id);
                pending.states.remove(&id);
                pending.deleted.push(id);
                urgent = true;
            }
            Ok(StoreMsg::Put(t, id, json)) => {
                pending.config.push((t, id, Some(json)));
                urgent = true;
            }
            Ok(StoreMsg::Delete(t, id)) => {
                pending.config.push((t, id, None));
                urgent = true;
            }
            Ok(StoreMsg::Backup(dest, ack)) => {
                flush(&mut conn, &mut pending);
                last_flush = Instant::now();
                let _ = std::fs::remove_file(&dest);
                let result = conn
                    .execute("VACUUM INTO ?1", params![dest.to_string_lossy()])
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                let _ = ack.send(result);
                continue;
            }
            Ok(StoreMsg::Flush(ack)) => {
                flush(&mut conn, &mut pending);
                last_flush = Instant::now();
                let _ = ack.send(());
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut conn, &mut pending);
                break;
            }
        }
        if urgent || last_flush.elapsed() >= FLUSH_INTERVAL {
            flush(&mut conn, &mut pending);
            last_flush = Instant::now();
        }
    }
}

fn flush(conn: &mut Connection, pending: &mut Pending) {
    if pending.is_empty() {
        return;
    }
    let batch = std::mem::take(pending);
    if let Err(e) = write_batch(conn, &batch) {
        tracing::error!("store: échec d'écriture: {e:#}");
    }
}

fn write_batch(conn: &mut Connection, batch: &Pending) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut upsert_meta = tx.prepare_cached(
            "INSERT INTO entities (entity_id, meta) VALUES (?1, ?2)
             ON CONFLICT(entity_id) DO UPDATE SET meta = excluded.meta",
        )?;
        for (id, meta) in &batch.metas {
            upsert_meta.execute(params![id, serde_json::to_string(meta)?])?;
        }
        let mut update_state =
            tx.prepare_cached("UPDATE entities SET state = ?2 WHERE entity_id = ?1")?;
        for (id, state) in &batch.states {
            update_state.execute(params![id, serde_json::to_string(state)?])?;
        }
        for id in &batch.deleted {
            tx.execute("DELETE FROM entities WHERE entity_id = ?1", params![id])?;
        }
        for (table, id, data) in &batch.config {
            match data {
                Some(json) => tx.execute(
                    &format!(
                        "INSERT INTO {t} (id, data) VALUES (?1, ?2)
                         ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                        t = table.name()
                    ),
                    params![id, json],
                )?,
                None => tx.execute(
                    &format!("DELETE FROM {} WHERE id = ?1", table.name()),
                    params![id],
                )?,
            };
        }
    }
    tx.commit()?;
    Ok(())
}

/// Petit utilitaire pour les tests et les outils : lit une valeur de configuration brute.
pub fn read_raw(path: &Path, table: Table, id: &str) -> anyhow::Result<Option<String>> {
    let conn = Connection::open(path)?;
    Ok(conn
        .query_row(
            &format!("SELECT data FROM {} WHERE id = ?1", table.name()),
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}
