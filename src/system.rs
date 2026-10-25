//! La machine elle-même : chien de garde systemd, sauvegardes de nuit, arrêt et redémarrage.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Local, NaiveDate, Timelike};
use serde::Serialize;

use crate::registry::RegistryHandle;
use crate::store::StoreHandle;

/// Nombre de sauvegardes de nuit gardées.
pub const KEEP_BACKUPS: usize = 7;
/// Heure locale de la sauvegarde de nuit.
const BACKUP_HOUR: u32 = 3;

// ---- Chien de garde ---------------------------------------------------------------

/// Envoie un message au gestionnaire de services (sd_notify), sans bibliothèque :
/// un datagramme sur la socket donnée par systemd dans NOTIFY_SOCKET.
#[cfg(unix)]
fn sd_notify(msg: &str) -> bool {
    use std::os::unix::net::UnixDatagram;
    let Ok(path) = std::env::var("NOTIFY_SOCKET") else {
        return false;
    };
    let Ok(sock) = UnixDatagram::unbound() else {
        return false;
    };
    if let Some(abstract_name) = path.strip_prefix('@') {
        #[cfg(target_os = "linux")]
        {
            use std::os::linux::net::SocketAddrExt;
            return std::os::unix::net::SocketAddr::from_abstract_name(abstract_name.as_bytes())
                .and_then(|addr| sock.send_to_addr(msg.as_bytes(), &addr))
                .is_ok();
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = abstract_name;
            return false;
        }
    }
    sock.send_to(msg.as_bytes(), path).is_ok()
}

#[cfg(not(unix))]
fn sd_notify(_msg: &str) -> bool {
    false
}

/// Intervalle demandé par systemd (WatchdogSec), s'il y en a un.
fn watchdog_interval() -> Option<Duration> {
    let usec: u64 = std::env::var("WATCHDOG_USEC").ok()?.parse().ok()?;
    // Si WATCHDOG_PID est donné, il doit être le nôtre.
    if let Ok(pid) = std::env::var("WATCHDOG_PID")
        && pid.parse::<u32>().ok() != Some(std::process::id())
    {
        return None;
    }
    Some(Duration::from_micros(usec))
}

/// Prévient systemd que Heim est prêt, puis lui dit régulièrement « je vais bien » — mais
/// seulement si le registre répond : un Heim figé (sans avoir planté) est alors relancé.
pub fn spawn_watchdog(registry: RegistryHandle) {
    sd_notify("READY=1");
    let Some(every) = watchdog_interval() else {
        return;
    };
    let period = every / 3;
    tracing::info!(
        "chien de garde systemd : signe de vie toutes les {} s",
        period.as_secs()
    );
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        loop {
            tick.tick().await;
            let alive = tokio::time::timeout(Duration::from_secs(5), registry.snapshot())
                .await
                .is_ok();
            if alive {
                sd_notify("WATCHDOG=1");
            } else {
                tracing::error!("le registre ne répond plus : systemd va relancer Heim");
            }
        }
    });
}

// ---- Sauvegardes de nuit ----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub bytes: u64,
    pub date: String,
}

pub fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// « heim-2026-10-25.db » → la date.
fn backup_date(name: &str) -> Option<NaiveDate> {
    let stem = name.strip_prefix("heim-")?.strip_suffix(".db")?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

pub fn list_backups(data_dir: &Path) -> Vec<BackupFile> {
    let mut out: Vec<BackupFile> = std::fs::read_dir(backups_dir(data_dir))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let date = backup_date(&name)?;
            Some(BackupFile {
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                date: date.to_string(),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Un nom de sauvegarde valide (rien d'autre ne peut être téléchargé par là).
pub fn backup_path(data_dir: &Path, name: &str) -> Option<PathBuf> {
    backup_date(name)?;
    Some(backups_dir(data_dir).join(name))
}

/// Les sauvegardes en trop, des plus anciennes : à supprimer.
pub fn to_prune(names: &[String], keep: usize) -> Vec<String> {
    let mut dated: Vec<&String> = names.iter().filter(|n| backup_date(n).is_some()).collect();
    dated.sort();
    let extra = dated.len().saturating_sub(keep);
    dated.into_iter().take(extra).cloned().collect()
}

/// Fait la sauvegarde du jour (si elle n'existe pas encore) et range les anciennes. Bloquant.
pub fn backup_now(store: &StoreHandle, data_dir: &Path) -> Result<BackupFile, String> {
    let dir = backups_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier des sauvegardes : {e}"))?;
    let name = format!("heim-{}.db", Local::now().format("%Y-%m-%d"));
    let path = dir.join(&name);
    let tmp = dir.join(format!("{name}.tmp"));
    store.backup_blocking(&tmp)?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    let names: Vec<String> = list_backups(data_dir).into_iter().map(|b| b.name).collect();
    for old in to_prune(&names, KEEP_BACKUPS) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(BackupFile {
        name,
        bytes,
        date: Local::now().date_naive().to_string(),
    })
}

/// Chaque nuit vers 3 h ; et au démarrage s'il manque celle du jour (Pi éteint la nuit).
pub fn spawn_nightly_backup(store: StoreHandle, data_dir: PathBuf, bus: crate::bus::Bus) {
    tokio::spawn(async move {
        // Laisser Heim démarrer tranquillement d'abord.
        tokio::time::sleep(Duration::from_secs(120)).await;
        let mut last_day: Option<NaiveDate> = list_backups(&data_dir)
            .first()
            .and_then(|b| NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").ok());
        loop {
            let now = Local::now();
            let today = now.date_naive();
            let due = last_day != Some(today) && (now.hour() >= BACKUP_HOUR || last_day.is_none());
            if due {
                let (s, d) = (store.clone(), data_dir.clone());
                match tokio::task::spawn_blocking(move || backup_now(&s, &d)).await {
                    Ok(Ok(b)) => {
                        tracing::info!("sauvegarde de nuit : {} ({} Ko)", b.name, b.bytes / 1024);
                        last_day = Some(today);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("sauvegarde de nuit impossible : {e}");
                        bus.publish(crate::bus::Event::Log {
                            kind: "system".into(),
                            message: format!("sauvegarde de nuit impossible : {e}"),
                            entity_id: None,
                            actor: None,
                        });
                        // Pas d'acharnement : on réessaie dans une heure.
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                    }
                    Err(_) => {}
                }
            }
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });
}

// ---- Arrêt et redémarrage de la machine -------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Reboot,
    PowerOff,
}

/// `systemctl reboot` / `poweroff`, autorisés à l'utilisateur heim par la règle polkit.
pub async fn power(what: Power) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("seulement sur Raspberry Pi / Linux".into());
    }
    let verb = match what {
        Power::Reboot => "reboot",
        Power::PowerOff => "poweroff",
    };
    let out = tokio::process::Command::new("systemctl")
        .arg(verb)
        .output()
        .await
        .map_err(|e| format!("commande impossible : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "refusé par le système : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_seven() {
        let names: Vec<String> = (1..=9)
            .map(|d| format!("heim-2026-10-{d:02}.db"))
            .chain(["autre.txt".to_string()])
            .collect();
        let gone = to_prune(&names, 7);
        assert_eq!(gone, vec!["heim-2026-10-01.db", "heim-2026-10-02.db"]);
    }

    #[test]
    fn only_backup_names_are_served() {
        let d = Path::new("/data");
        assert!(backup_path(d, "heim-2026-10-25.db").is_some());
        assert!(backup_path(d, "../heim.db").is_none());
        assert!(backup_path(d, "heim-../../etc.db").is_none());
    }
}
