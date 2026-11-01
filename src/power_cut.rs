//! Retour du courant après une coupure.
//!
//! Certaines ampoules Zigbee se réinitialisent quand le courant revient par à-coups (la même
//! séquence que « allumer/éteindre 5 fois » pour les remettre à zéro) : elles quittent le réseau
//! et clignotent, prêtes à être appairées. Au retour du courant, Heim ouvre donc l'appairage
//! quelques minutes : elles reviennent seules, avec le même identifiant, donc le même nom, la
//! même pièce et leurs automatismes. Ensuite Heim dit lesquelles sont revenues.
//!
//! Coupure ou redémarrage voulu ? Heim pose une marque avant chaque redémarrage qu'il lance
//! (bouton, mise à jour) ; une machine qui vient de démarrer sans cette marque a subi une
//! coupure (ou un débranchement).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event};
use crate::notify::{Level, Notifier, NotifyRequest};
use crate::settings::Settings;

/// Posée juste avant un redémarrage voulu (voir `system::power`).
pub const PLANNED: &str = "reboot.planned";
/// Durée de la fenêtre d'appairage.
const MINUTES: u32 = 4;

pub fn yes() -> bool {
    true
}

fn uptime_s() -> Option<f64> {
    std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// La machine vient-elle de redémarrer après une coupure ? (Consomme la marque.)
pub fn after_power_cut(data_dir: &Path) -> bool {
    let mark = data_dir.join(PLANNED);
    let planned = mark.exists();
    let _ = std::fs::remove_file(&mark);
    let fresh_boot = uptime_s().is_some_and(|s| s < 15.0 * 60.0);
    fresh_boot && !planned
}

pub fn spawn(data_dir: &Path, bus: Bus, settings: Arc<Settings>, notify: Arc<Notifier>) {
    if !after_power_cut(data_dir) {
        return;
    }
    let s: crate::api_controls::PowerOnSettings = settings
        .get(crate::api_controls::POWER_ON_KEY)
        .unwrap_or_default();
    tracing::warn!("démarrage après une coupure de courant");
    if !s.rejoin {
        return;
    }
    let minutes = MINUTES;
    // Abonné tout de suite : on ne veut rater ni l'arrivée du pont ni les appareils.
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        // Attendre que Zigbee2MQTT soit en ligne (3 min au plus, puis on essaie quand même).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(Event::ZigbeeBridgeOnline { online: true }) => break,
                    Err(RecvError::Closed) => return,
                    _ => {}
                },
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        bus.publish(Event::Log {
            kind: "system".into(),
            message: format!(
                "Retour du courant : appairage Zigbee ouvert {minutes} min pour récupérer les appareils réinitialisés"
            ),
            entity_id: None,
            actor: None,
        });
        bus.publish(Event::ZigbeePermitJoin {
            seconds: minutes * 60,
        });
        // Qui revient pendant la fenêtre ?
        let mut back: Vec<String> = Vec::new();
        let end = tokio::time::Instant::now() + Duration::from_secs(minutes as u64 * 60 + 30);
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(Event::ZigbeeDevice { event, friendly_name, .. })
                        if (event == "device_joined" || event == "device_announce")
                            && !friendly_name.is_empty()
                            && !back.contains(&friendly_name) =>
                    {
                        back.push(friendly_name);
                    }
                    Err(RecvError::Closed) => return,
                    _ => {}
                },
                _ = tokio::time::sleep_until(end) => break,
            }
        }
        if back.is_empty() {
            return;
        }
        let message = format!(
            "Après la coupure de courant, {} appareil(s) Zigbee sont revenus d'eux-mêmes : {}",
            back.len(),
            back.join(", ")
        );
        bus.publish(Event::Log {
            kind: "system".into(),
            message: message.clone(),
            entity_id: None,
            actor: None,
        });
        let _ = notify.send(NotifyRequest {
            title: Some("Retour du courant".into()),
            message,
            level: Level::Info,
            to: Vec::new(),
            except: Vec::new(),
            action: None,
        });
    });
}
