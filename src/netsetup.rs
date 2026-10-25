//! Réseau du Raspberry Pi : le Wi-Fi « Heim-XXXX » du premier démarrage, puis le Wi-Fi de la
//! maison. S'appuie sur NetworkManager (`nmcli`), présent sur Raspberry Pi OS Bookworm.
//!
//! - Au démarrage, si le Pi n'est relié à rien (ni câble ni Wi-Fi), il ouvre le réseau
//!   « Heim-XXXX » (sans mot de passe). Le téléphone qui s'y connecte voit s'ouvrir l'assistant :
//!   toutes les adresses mènent à Heim (portail captif, comme le Wi-Fi d'un hôtel).
//! - L'assistant donne le nom et le mot de passe du Wi-Fi de la maison : Heim ferme son réseau
//!   et rejoint celui-là. Si ça échoue (mot de passe faux), « Heim-XXXX » revient.
//! - Plus tard, si la maison perd son Wi-Fi plus de 15 minutes, « Heim-XXXX » revient aussi,
//!   pour pouvoir changer de réseau. Il se referme dès que le Wi-Fi connu est de retour.
//!
//! Actif seulement avec HEIM_NETSETUP=1 (l'image SD le met).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::Command;

pub const AP_CONNECTION: &str = "heim-setup";
pub const AP_ADDRESS: &str = "10.42.0.1";

pub fn enabled() -> bool {
    cfg!(target_os = "linux") && std::env::var("HEIM_NETSETUP").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetState {
    /// « ethernet », « wifi », « hotspot », « none »
    pub mode: String,
    pub ssid: Option<String>,
    pub ip: Option<String>,
    pub hotspot_ssid: String,
    pub networks: Vec<WifiNetwork>,
    pub last_error: Option<String>,
    pub busy: bool,
}

/// `nmcli -t` sépare par « : » et échappe « \: ».
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// `nmcli -t -f SSID,SIGNAL,SECURITY device wifi list`
pub fn parse_wifi_list(text: &str) -> Vec<WifiNetwork> {
    let mut best: Vec<WifiNetwork> = Vec::new();
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 3 || f[0].trim().is_empty() {
            continue;
        }
        let net = WifiNetwork {
            ssid: f[0].clone(),
            signal: f[1].parse().unwrap_or(0),
            secure: !f[2].trim().is_empty() && f[2].trim() != "--",
        };
        if net.ssid.starts_with("Heim-") {
            continue;
        }
        match best.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(n) if n.signal < net.signal => *n = net,
            Some(_) => {}
            None => best.push(net),
        }
    }
    best.sort_by_key(|n| std::cmp::Reverse(n.signal));
    best
}

/// `nmcli -t -f TYPE,STATE,CONNECTION device` -> (mode, nom du réseau).
pub fn parse_devices(text: &str) -> (String, Option<String>) {
    let mut wifi = None;
    let mut hotspot = false;
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 3 || f[1] != "connected" {
            continue;
        }
        match f[0].as_str() {
            "ethernet" => return ("ethernet".into(), None),
            "wifi" if f[2] == AP_CONNECTION => hotspot = true,
            "wifi" => wifi = Some(f[2].clone()),
            _ => {}
        }
    }
    match (wifi, hotspot) {
        (Some(s), _) => ("wifi".into(), Some(s)),
        (None, true) => ("hotspot".into(), None),
        _ => ("none".into(), None),
    }
}

async fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("nmcli introuvable : {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("nmcli : code {}", out.status)
        } else {
            err
        })
    }
}

/// « Heim-3F2A » d'après la fin de l'adresse Wi-Fi : deux Heim voisins ne se confondent pas.
async fn hotspot_name() -> String {
    let mac = std::fs::read_to_string("/sys/class/net/wlan0/address").unwrap_or_default();
    let tail: String = mac
        .trim()
        .replace(':', "")
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        "Heim-{}",
        if tail.is_empty() {
            "0000".into()
        } else {
            tail.to_uppercase()
        }
    )
}

pub struct NetSetup {
    state: Mutex<NetState>,
    last_activity: Mutex<Option<Instant>>,
}

impl NetSetup {
    pub fn new() -> Arc<Self> {
        Arc::new(NetSetup {
            state: Mutex::new(NetState::default()),
            last_activity: Mutex::new(None),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, NetState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Quelqu'un utilise l'assistant : on ne coupe pas le réseau Heim sous ses pieds.
    pub fn touch(&self) {
        *self.last_activity.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    fn idle_for(&self) -> Duration {
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|t| t.elapsed())
            .unwrap_or(Duration::MAX)
    }

    pub fn state(&self) -> NetState {
        self.lock().clone()
    }

    pub async fn refresh(&self) -> NetState {
        if !enabled() {
            return NetState {
                mode: "unmanaged".into(),
                ..Default::default()
            };
        }
        let (mode, ssid) = match nmcli(&["-t", "-f", "TYPE,STATE,CONNECTION", "device"]).await {
            Ok(t) => parse_devices(&t),
            Err(_) => ("none".into(), None),
        };
        let ip = nmcli(&["-g", "IP4.ADDRESS", "device", "show", "wlan0"])
            .await
            .ok()
            .and_then(|t| {
                t.lines()
                    .next()
                    .map(|l| l.split('/').next().unwrap_or_default().to_string())
            })
            .filter(|s| !s.is_empty());
        let name = hotspot_name().await;
        let mut s = self.lock();
        s.mode = mode;
        s.ssid = ssid;
        s.ip = ip;
        s.hotspot_ssid = name;
        s.clone()
    }

    /// Liste des Wi-Fi visibles. En mode « Heim-XXXX », la puce ne peut pas chercher :
    /// on renvoie la liste vue juste avant de l'ouvrir.
    pub async fn scan(&self) -> Vec<WifiNetwork> {
        if !enabled() {
            return Vec::new();
        }
        if self.lock().mode != "hotspot"
            && let Ok(t) = nmcli(&[
                "-t",
                "-f",
                "SSID,SIGNAL,SECURITY",
                "device",
                "wifi",
                "list",
                "--rescan",
                "yes",
            ])
            .await
        {
            let list = parse_wifi_list(&t);
            if !list.is_empty() {
                self.lock().networks = list;
            }
        }
        self.lock().networks.clone()
    }

    pub async fn start_hotspot(&self) -> Result<(), String> {
        self.scan().await;
        let ssid = hotspot_name().await;
        let _ = nmcli(&["connection", "delete", AP_CONNECTION]).await;
        nmcli(&[
            "connection",
            "add",
            "type",
            "wifi",
            "ifname",
            "wlan0",
            "con-name",
            AP_CONNECTION,
            "autoconnect",
            "no",
            "ssid",
            &ssid,
            "802-11-wireless.mode",
            "ap",
            "802-11-wireless.band",
            "bg",
            "ipv4.method",
            "shared",
            "ipv4.addresses",
            &format!("{AP_ADDRESS}/24"),
            "ipv6.method",
            "disabled",
        ])
        .await?;
        nmcli(&["connection", "up", AP_CONNECTION]).await?;
        tracing::warn!("réseau Wi-Fi « {ssid} » ouvert : connectez-vous-y pour configurer Heim");
        self.refresh().await;
        Ok(())
    }

    pub async fn stop_hotspot(&self) {
        let _ = nmcli(&["connection", "down", AP_CONNECTION]).await;
    }

    /// Rejoint un Wi-Fi. En cas d'échec, rouvre « Heim-XXXX » et garde l'erreur pour l'assistant.
    pub async fn connect(&self, ssid: &str, password: &str) -> Result<(), String> {
        self.lock().busy = true;
        let was_hotspot = self.lock().mode == "hotspot";
        self.stop_hotspot().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let _ = nmcli(&["device", "wifi", "rescan", "ifname", "wlan0"]).await;
        tokio::time::sleep(Duration::from_secs(4)).await;
        let mut args = vec!["--wait", "45", "device", "wifi", "connect", ssid];
        if !password.is_empty() {
            args.extend(["password", password]);
        }
        args.extend(["ifname", "wlan0"]);
        let result = nmcli(&args).await;
        let outcome = match result {
            Ok(_) => {
                tracing::info!("Wi-Fi « {ssid} » rejoint");
                self.lock().last_error = None;
                Ok(())
            }
            Err(e) => {
                let msg = if e.contains("Secrets were required")
                    || e.contains("802-1X")
                    || e.contains("password")
                {
                    format!("mot de passe refusé par « {ssid} »")
                } else if e.contains("No network with SSID") {
                    format!("réseau « {ssid} » introuvable")
                } else {
                    format!("connexion à « {ssid} » impossible : {e}")
                };
                tracing::warn!("{msg}");
                // On ne garde pas un profil qui ne marche pas.
                let _ = nmcli(&["connection", "delete", ssid]).await;
                self.lock().last_error = Some(msg.clone());
                if was_hotspot || self.refresh().await.mode == "none" {
                    let _ = self.start_hotspot().await;
                }
                Err(msg)
            }
        };
        self.lock().busy = false;
        self.refresh().await;
        outcome
    }

    /// Surveille le réseau : ouvre « Heim-XXXX » quand il n'y a rien, le referme quand le Wi-Fi
    /// connu revient (en laissant l'assistant tranquille tant qu'il est utilisé).
    pub fn spawn_watchdog(self: &Arc<Self>) {
        if !enabled() {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            // Laisser NetworkManager rejoindre les réseaux connus au démarrage.
            tokio::time::sleep(Duration::from_secs(40)).await;
            let mut offline_since: Option<Instant> = None;
            let mut hotspot_since: Option<Instant> = None;
            let mut first = true;
            loop {
                let st = me.refresh().await;
                if st.busy {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
                match st.mode.as_str() {
                    "ethernet" | "wifi" => {
                        offline_since = None;
                        hotspot_since = None;
                    }
                    "none" => {
                        let since = *offline_since.get_or_insert_with(Instant::now);
                        if first || since.elapsed() > Duration::from_secs(15 * 60) {
                            if let Err(e) = me.start_hotspot().await {
                                tracing::warn!("réseau Heim impossible à ouvrir : {e}");
                            }
                            hotspot_since = Some(Instant::now());
                        }
                    }
                    "hotspot" => {
                        // Toutes les 10 min sans personne dans l'assistant : on retente le Wi-Fi connu.
                        let since = *hotspot_since.get_or_insert_with(Instant::now);
                        let has_saved = nmcli(&["-t", "-f", "TYPE,NAME", "connection", "show"])
                            .await
                            .map(|t| {
                                t.lines().any(|l| {
                                    l.starts_with("802-11-wireless:") && !l.ends_with(AP_CONNECTION)
                                })
                            })
                            .unwrap_or(false);
                        if has_saved
                            && since.elapsed() > Duration::from_secs(600)
                            && me.idle_for() > Duration::from_secs(600)
                        {
                            tracing::info!("nouvel essai du Wi-Fi connu");
                            me.stop_hotspot().await;
                            tokio::time::sleep(Duration::from_secs(90)).await;
                            if me.refresh().await.mode == "none" {
                                let _ = me.start_hotspot().await;
                            }
                            hotspot_since = Some(Instant::now());
                        }
                    }
                    _ => {}
                }
                first = false;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nmcli_output() {
        let list = "Livebox-3A2F:72:WPA2\nLivebox-3A2F:40:WPA2\nCafé\\:Wifi:55:\n:30:WPA2\nHeim-1234:90:\nFreebox-X:81:WPA2 WPA3\n";
        let nets = parse_wifi_list(list);
        assert_eq!(nets.len(), 3);
        assert_eq!(nets[0].ssid, "Freebox-X");
        assert_eq!(nets[1].signal, 72);
        assert_eq!(nets[2].ssid, "Café:Wifi");
        assert!(!nets[2].secure);

        assert_eq!(
            parse_devices("wifi:connected:Livebox-3A2F\nloopback:connected (externally):lo\n"),
            ("wifi".into(), Some("Livebox-3A2F".into()))
        );
        assert_eq!(
            parse_devices("wifi:connected:heim-setup\n"),
            ("hotspot".into(), None)
        );
        assert_eq!(
            parse_devices("ethernet:connected:Wired connection 1\nwifi:disconnected:\n").0,
            "ethernet"
        );
        assert_eq!(parse_devices("wifi:disconnected:\n").0, "none");
    }
}
