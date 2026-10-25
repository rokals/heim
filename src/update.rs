//! Mises à jour par GitHub Releases, sans reflasher la carte SD.
//!
//! 1. Heim regarde la dernière « release » du dépôt (toutes les 12 h, ou sur demande).
//! 2. « Installer » : téléchargement du binaire pour cette machine, contrôle SHA-256,
//!    sauvegarde de la base, remplacement du binaire (l'ancien est gardé en `heim.prev`).
//! 3. Heim s'arrête proprement ; systemd le relance aussitôt sur la nouvelle version.
//! 4. Si la nouvelle version ne tient pas debout (trois démarrages ratés), le script de garde
//!    lancé par systemd (`heim-guard`) remet l'ancien binaire et l'ancienne base.
//!
//! Fichiers attendus dans la release : `heim-aarch64-linux`, `heim-armv7-linux`,
//! `heim-x86_64-linux`, `heim-x86_64-windows.exe` et `SHA256SUMS`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SETTINGS_KEY: &str = "update";
pub const MARKER: &str = "update.pending";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Dépôt par défaut : celui qui a construit ce binaire (rempli par GitHub Actions).
pub fn default_repo() -> Option<String> {
    option_env!("HEIM_REPO")
        .map(str::to_string)
        .filter(|s| s.contains('/'))
}

fn d_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// `proprietaire/heim`
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default = "d_true")]
    pub auto_check: bool,
    /// Proposer aussi les versions d'essai (« pre-release »).
    #[serde(default)]
    pub prereleases: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            repo: None,
            auto_check: true,
            prereleases: false,
        }
    }
}

impl UpdateSettings {
    pub fn repo(&self) -> Option<String> {
        self.repo
            .clone()
            .filter(|r| !r.trim().is_empty())
            .or_else(default_repo)
    }
}

/// « 0.6.0 », « v0.6.1-rc.1 » -> (0, 6, 1, Some("rc.1")).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    let mut it = core.split('.').map(|x| x.parse::<u64>());
    let major = it.next()?.ok()?;
    let minor = it.next().unwrap_or(Ok(0)).ok()?;
    let patch = it.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch, pre))
}

/// `a` est-elle plus récente que `b` ? Une version finale passe devant ses « rc ».
pub fn newer(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Some(x), Some(y)) => {
            let (xa, ya) = ((x.0, x.1, x.2), (y.0, y.1, y.2));
            if xa != ya {
                return xa > ya;
            }
            match (&x.3, &y.3) {
                (None, Some(_)) => true,
                (Some(p), Some(q)) => p > q,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Le nom du fichier de la release qui correspond à cette machine.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => Some("heim-aarch64-linux"),
        ("linux", "arm") => Some("heim-armv7-linux"),
        ("linux", "x86_64") => Some("heim-x86_64-linux"),
        ("windows", "x86_64") => Some("heim-x86_64-windows.exe"),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub notes: String,
    pub published: Option<DateTime<Utc>>,
    pub url: String,
    pub prerelease: bool,
    #[serde(skip)]
    pub asset_url: Option<String>,
    #[serde(skip)]
    pub sums_url: Option<String>,
    pub has_asset: bool,
}

/// La release la plus récente parmi la réponse de l'API GitHub.
pub fn pick_release(list: &Value, prereleases: bool) -> Option<Release> {
    let asset = asset_name();
    list.as_array()?
        .iter()
        .filter(|r| r.get("draft").and_then(Value::as_bool) != Some(true))
        .filter(|r| prereleases || r.get("prerelease").and_then(Value::as_bool) != Some(true))
        .filter_map(|r| {
            let tag = r.get("tag_name").and_then(Value::as_str)?;
            parse_version(tag)?;
            let assets = r
                .get("assets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let find = |name: &str| {
                assets
                    .iter()
                    .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
                    .and_then(|a| a.get("browser_download_url").and_then(Value::as_str))
                    .map(str::to_string)
            };
            let asset_url = asset.and_then(find);
            Some(Release {
                version: tag.trim_start_matches('v').to_string(),
                name: r
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(tag)
                    .to_string(),
                notes: r
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .chars()
                    .take(4000)
                    .collect(),
                published: r
                    .get("published_at")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok()),
                url: r
                    .get("html_url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                prerelease: r
                    .get("prerelease")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                has_asset: asset_url.is_some(),
                asset_url,
                sums_url: find("SHA256SUMS"),
            })
        })
        .max_by(|a, b| {
            if newer(&a.version, &b.version) {
                std::cmp::Ordering::Greater
            } else if newer(&b.version, &a.version) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
}

/// Cherche l'empreinte d'un fichier dans un SHA256SUMS (« <hex>  <nom> »).
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub current: String,
    pub repo: Option<String>,
    pub latest: Option<Release>,
    pub available: bool,
    pub checked: Option<DateTime<Utc>>,
    pub error: Option<String>,
    /// Heim peut-il s'installer tout seul ici ? Sinon, pourquoi.
    pub can_install: bool,
    pub install_blocker: Option<String>,
    pub installing: bool,
    /// Mise à jour en attente de confirmation (Heim vient de redémarrer dessus).
    pub pending: Option<String>,
}

/// Lancé par systemd ? (alors un arrêt = un redémarrage automatique)
pub fn supervised() -> bool {
    std::env::var_os("INVOCATION_ID").is_some() || std::env::var_os("HEIM_SELF_UPDATE").is_some()
}

fn install_blocker(exe: &Path) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return Some("installation automatique seulement sur Raspberry Pi / Linux : téléchargez la nouvelle version".into());
    }
    if asset_name().is_none() {
        return Some("pas de version publiée pour ce processeur".into());
    }
    if !supervised() {
        return Some("Heim doit tourner comme service (systemd) pour redémarrer tout seul".into());
    }
    let dir = exe.parent()?;
    let probe = dir.join(".heim-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(probe);
            None
        }
        Err(_) => Some(format!("pas le droit d'écrire dans {}", dir.display())),
    }
}

pub struct Updater {
    status: Mutex<Status>,
    data_dir: PathBuf,
    #[cfg(feature = "webhook")]
    http: reqwest::Client,
}

impl Updater {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let exe = std::env::current_exe().unwrap_or_default();
        let blocker = install_blocker(&exe);
        let pending = std::fs::read_to_string(data_dir.join(MARKER))
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("to=").map(str::to_string))
            });
        // Le script de garde a remis l'ancienne version : on le dit une fois.
        let failed = data_dir.join("update.failed");
        let error = std::fs::read_to_string(&failed).ok().map(|t| {
            let _ = std::fs::remove_file(&failed);
            let to = t.lines().find_map(|l| l.strip_prefix("to=")).unwrap_or("?").to_string();
            tracing::warn!("la mise à jour vers {to} n'a pas démarré : retour à {CURRENT}");
            format!("la version {to} n'a pas réussi à démarrer : Heim est revenu à la {CURRENT}, avec vos données d'avant")
        });
        Arc::new(Updater {
            status: Mutex::new(Status {
                current: CURRENT.into(),
                can_install: blocker.is_none(),
                install_blocker: blocker,
                pending,
                error,
                ..Default::default()
            }),
            data_dir,
            #[cfg(feature = "webhook")]
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .user_agent(concat!("heim/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> Status {
        self.lock().clone()
    }

    /// Après 90 s sans plantage sur une nouvelle version : elle est adoptée.
    pub fn confirm_after_start(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(90)).await;
            let marker = me.data_dir.join(MARKER);
            if marker.exists() {
                let _ = std::fs::remove_file(&marker);
                let _ = std::fs::remove_file(
                    std::env::current_exe()
                        .unwrap_or_default()
                        .with_extension("prev"),
                );
                tracing::info!("mise à jour vers {CURRENT} confirmée");
                me.lock().pending = None;
            }
        });
    }

    #[cfg(feature = "webhook")]
    pub async fn check(&self, settings: &UpdateSettings) -> Status {
        let Some(repo) = settings.repo() else {
            let mut s = self.lock();
            s.repo = None;
            s.error = Some("indiquez le dépôt GitHub (exemple : alex/heim)".into());
            return s.clone();
        };
        let url = format!("https://api.github.com/repos/{repo}/releases?per_page=15");
        let result: Result<Value, String> = async {
            let resp = self
                .http
                .get(&url)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|e| format!("GitHub injoignable : {e}"))?;
            match resp.status().as_u16() {
                200 => resp.json().await.map_err(|e| e.to_string()),
                404 => Err(format!(
                    "dépôt « {repo} » introuvable (il doit être public)"
                )),
                403 | 429 => {
                    Err("GitHub limite les vérifications : réessayez dans une heure".into())
                }
                c => Err(format!("GitHub répond {c}")),
            }
        }
        .await;
        let mut s = self.lock();
        s.repo = Some(repo);
        s.checked = Some(Utc::now());
        match result {
            Ok(list) => {
                s.error = None;
                s.latest = pick_release(&list, settings.prereleases);
                s.available = s
                    .latest
                    .as_ref()
                    .is_some_and(|r| newer(&r.version, CURRENT));
                if s.latest.is_none() {
                    s.error = Some("aucune version publiée pour l'instant".into());
                }
            }
            Err(e) => s.error = Some(e),
        }
        s.clone()
    }

    #[cfg(not(feature = "webhook"))]
    pub async fn check(&self, _settings: &UpdateSettings) -> Status {
        let mut s = self.lock();
        s.error = Some("Heim compilé sans accès HTTP".into());
        s.clone()
    }

    /// Télécharge, vérifie et met en place la nouvelle version. Renvoie la version installée ;
    /// l'appelant arrête ensuite Heim pour que systemd le relance.
    #[cfg(feature = "webhook")]
    pub async fn install(&self, store: crate::store::StoreHandle) -> Result<String, String> {
        let (release, blocker) = {
            let s = self.lock();
            (s.latest.clone(), s.install_blocker.clone())
        };
        if let Some(b) = blocker {
            return Err(b);
        }
        let release = release
            .filter(|r| newer(&r.version, CURRENT))
            .ok_or("aucune nouvelle version")?;
        let asset_url = release
            .asset_url
            .clone()
            .ok_or("pas de fichier pour cette machine dans la release")?;
        let sums_url = release
            .sums_url
            .clone()
            .ok_or("fichier SHA256SUMS absent de la release")?;
        self.lock().installing = true;
        let result = self
            .do_install(&release, &asset_url, &sums_url, store)
            .await;
        self.lock().installing = false;
        result
    }

    #[cfg(feature = "webhook")]
    async fn do_install(
        &self,
        release: &Release,
        asset_url: &str,
        sums_url: &str,
        store: crate::store::StoreHandle,
    ) -> Result<String, String> {
        use sha2::Digest;
        let name = asset_name().ok_or("processeur non pris en charge")?;
        let get = |url: String| async move {
            let r = self
                .http
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("téléchargement impossible : {e}"))?;
            if !r.status().is_success() {
                return Err(format!("téléchargement : réponse {}", r.status()));
            }
            r.bytes().await.map_err(|e| e.to_string())
        };
        let sums = get(sums_url.to_string()).await?;
        let expected = sum_for(&String::from_utf8_lossy(&sums), name)
            .ok_or("empreinte absente de SHA256SUMS")?;
        tracing::info!(
            "mise à jour : téléchargement de {} ({name})",
            release.version
        );
        let bin = get(asset_url.to_string()).await?;
        let got: String = sha2::Sha256::digest(&bin)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != expected {
            return Err(
                "le fichier téléchargé est abîmé (empreinte différente) : rien n'a été changé"
                    .into(),
            );
        }
        if bin.len() < 1_000_000 || &bin[..4] != b"\x7fELF" {
            return Err("le fichier téléchargé n'est pas un programme Linux".into());
        }

        // Sauvegarde de la base : en cas de retour arrière, l'ancienne version la retrouve.
        let backup = self
            .data_dir
            .join(format!("heim-avant-{}.db", release.version));
        let b = backup.clone();
        tokio::task::spawn_blocking(move || store.backup_blocking(&b))
            .await
            .map_err(|e| e.to_string())??;

        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let new = exe.with_extension("new");
        let prev = exe.with_extension("prev");
        std::fs::write(&new, &bin).map_err(|e| format!("écriture impossible : {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        std::fs::write(
            self.data_dir.join(MARKER),
            format!(
                "from={CURRENT}\nto={}\nattempts=0\nbackup={}\nexe={}\n",
                release.version,
                backup.display(),
                exe.display()
            ),
        )
        .map_err(|e| e.to_string())?;
        // Remplacement : sous Linux on peut renommer un programme en cours d'exécution.
        std::fs::rename(&exe, &prev).map_err(|e| format!("remplacement impossible : {e}"))?;
        if let Err(e) = std::fs::rename(&new, &exe) {
            let _ = std::fs::rename(&prev, &exe);
            return Err(format!("remplacement impossible : {e}"));
        }
        tracing::warn!("mise à jour {} installée : redémarrage", release.version);
        Ok(release.version.clone())
    }

    #[cfg(not(feature = "webhook"))]
    pub async fn install(&self, _store: crate::store::StoreHandle) -> Result<String, String> {
        Err("Heim compilé sans accès HTTP".into())
    }
}

/// Arrête Heim proprement : systemd le relance aussitôt.
pub fn restart_soon(store: crate::store::StoreHandle) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let _ = tokio::task::spawn_blocking(move || store.flush_blocking()).await;
        tracing::warn!("redémarrage de Heim");
        std::process::exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compares_versions() {
        assert!(newer("0.6.1", "0.6.0"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(!newer("0.6.0", "0.6.0"));
        assert!(newer("0.7.0", "0.7.0-rc.1"));
        assert!(!newer("0.7.0-rc.1", "0.7.0"));
        assert!(newer("0.7.0-rc.2", "0.7.0-rc.1"));
        assert!(!newer("pas-une-version", "0.1.0"));
    }

    #[test]
    fn picks_the_newest_published_release() {
        let name = asset_name().unwrap_or("heim-x86_64-linux");
        let list = json!([
            {"tag_name": "v0.7.0-rc.1", "prerelease": true, "draft": false, "assets": []},
            {"tag_name": "v0.6.1", "prerelease": false, "draft": false, "name": "Heim 0.6.1", "body": "Corrections",
             "html_url": "https://github.com/x/heim/releases/tag/v0.6.1", "published_at": "2026-10-30T10:00:00Z",
             "assets": [{"name": name, "browser_download_url": "https://example.com/bin"},
                        {"name": "SHA256SUMS", "browser_download_url": "https://example.com/sums"}]},
            {"tag_name": "v0.6.0", "prerelease": false, "draft": false, "assets": []},
            {"tag_name": "v9.9.9", "prerelease": false, "draft": true, "assets": []}
        ]);
        let r = pick_release(&list, false).unwrap();
        assert_eq!(r.version, "0.6.1");
        assert_eq!(r.notes, "Corrections");
        assert_eq!(r.sums_url.as_deref(), Some("https://example.com/sums"));
        assert_eq!(pick_release(&list, true).unwrap().version, "0.7.0-rc.1");
    }

    #[test]
    fn reads_sha256sums() {
        let sums = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  heim-aarch64-linux\n\
                    BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB *heim-armv7-linux\n";
        assert_eq!(sum_for(sums, "heim-armv7-linux").unwrap(), "b".repeat(64));
        assert!(sum_for(sums, "heim-x86_64-linux").is_none());
    }
}
