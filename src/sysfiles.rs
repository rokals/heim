//! Les fichiers système de la box (services, règles polkit, Zigbee2MQTT…) suivent Heim.
//!
//! Une mise à jour ordinaire ne remplace que le programme. Pour que les nouveautés « système »
//! arrivent aussi sans reflasher la carte, Heim dépose une demande (`system.request`, une
//! simple version) ; le service root `heim-system` télécharge alors l'archive `heim-system.tar.gz`
//! de cette version, depuis la release officielle du dépôt gravé dans l'image
//! (`/etc/heim/repo`), vérifie son empreinte et l'applique. Heim ne choisit que la version : rien
//! de ce qu'il écrit n'est exécuté en root.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

const HELPER: &str = "/usr/local/lib/heim/heim-system";
const REPO_FILE: &str = "/etc/heim/repo";
/// Après un échec (pas d'Internet au premier démarrage…), nouvel essai au plus tôt dans 1 h.
const RETRY_AFTER: Duration = Duration::from_secs(3600);

pub fn result_path(data: &Path) -> PathBuf {
    data.join("system.result")
}

/// `status=ok\nversion=0.15.0\n…` → objet.
pub fn parse(text: &str) -> Value {
    let mut out = serde_json::Map::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=')
            && ["status", "version", "message", "at"].contains(&k)
        {
            out.insert(k.into(), json!(v.trim()));
        }
    }
    Value::Object(out)
}

/// Ce qu'on montre dans Réglages › Système.
pub fn status(data: &Path) -> Value {
    let managed = Path::new(HELPER).exists() && Path::new(REPO_FILE).exists();
    let mut v = std::fs::read_to_string(result_path(data))
        .map(|t| parse(&t))
        .unwrap_or_else(|_| json!({}));
    v["managed"] = json!(managed);
    v["pending"] = json!(data.join("system.request").exists());
    v
}

/// Faut-il demander les fichiers système de `version` ?
pub fn needs_request(data: &Path, version: &str, now: SystemTime) -> bool {
    let done = std::fs::read_to_string(result_path(data))
        .map(|t| parse(&t))
        .unwrap_or_default();
    if done["status"] == "ok" && done["version"] == version {
        return false;
    }
    if data.join("system.request").exists() {
        return false;
    }
    // Déjà demandé pour cette version il y a peu : on laisse respirer.
    let marker = data.join("system.requested");
    match (
        std::fs::read_to_string(&marker),
        std::fs::metadata(&marker).and_then(|m| m.modified()),
    ) {
        (Ok(v), Ok(at)) if v.trim() == version => {
            now.duration_since(at).unwrap_or_default() >= RETRY_AFTER
        }
        _ => true,
    }
}

/// Au démarrage de Heim : demande les fichiers système de sa version, si la box sait le faire.
pub fn request_if_needed(data: &Path) {
    if !Path::new(HELPER).exists() || !Path::new(REPO_FILE).exists() {
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    if !needs_request(data, version, SystemTime::now()) {
        return;
    }
    let _ = std::fs::write(data.join("system.requested"), version);
    match std::fs::write(data.join("system.request"), format!("version={version}\n")) {
        Ok(()) => tracing::info!("fichiers système de la {version} demandés"),
        Err(e) => tracing::warn!("fichiers système : demande impossible : {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_once_per_version() {
        let dir = std::env::temp_dir().join(format!("heim-sys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        assert!(needs_request(&dir, "0.15.0", now));
        std::fs::write(dir.join("system.requested"), "0.15.0").unwrap();
        assert!(!needs_request(&dir, "0.15.0", now));
        assert!(needs_request(
            &dir,
            "0.15.0",
            now + RETRY_AFTER + Duration::from_secs(1)
        ));
        assert!(needs_request(&dir, "0.16.0", now));
        std::fs::write(result_path(&dir), "status=ok\nversion=0.16.0\nat=x\n").unwrap();
        assert!(!needs_request(&dir, "0.16.0", now));
        std::fs::write(dir.join("system.request"), "version=0.17.0\n").unwrap();
        assert!(!needs_request(&dir, "0.17.0", now));
        assert_eq!(status(&dir)["pending"], json!(true));
        assert_eq!(
            parse("status=failed\nmessage=pas d'Internet\n")["message"],
            json!("pas d'Internet")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
