//! Alertes prêtes à l'emploi, sans rien configurer :
//! - pile faible (une fois par appareil, jusqu'au changement de pile) ;
//! - appareil muet depuis longtemps (probablement mort ou hors de portée) ;
//! - fuite d'eau (tout de suite, en urgent) ;
//! - Zigbee2MQTT (ou une autre intégration) injoignable depuis 5 minutes, puis de retour.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{TimeDelta, Utc};
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::EntityView;
use crate::notify::{Level, Notifier, NotifyRequest};
use crate::registry::RegistryHandle;
use crate::settings::{self, AlertSettings, Settings};

const SWEEP_EVERY: Duration = Duration::from_secs(15 * 60);
const INTEGRATION_GRACE: Duration = Duration::from_secs(5 * 60);

fn label(integration: &str) -> &str {
    match integration {
        "zigbee2mqtt" => "Zigbee2MQTT",
        "tuya" => "Tuya",
        "matter" => "Le serveur Matter",
        other => other,
    }
}

fn kind(e: &EntityView) -> &str {
    e.meta
        .capabilities
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

fn device_name(e: &EntityView) -> String {
    if e.meta.user_edited {
        return e.meta.name.clone();
    }
    e.meta
        .capabilities
        .get("device_name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| e.meta.name.clone())
}

fn device_key(e: &EntityView) -> String {
    e.meta
        .device_id
        .clone()
        .unwrap_or_else(|| e.meta.entity_id.clone())
}

pub fn spawn(registry: RegistryHandle, notify: Arc<Notifier>, settings: Arc<Settings>, bus: &Bus) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        let mut low_battery: HashSet<String> = HashSet::new();
        let mut silent: HashSet<String> = HashSet::new();
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        sweep.tick().await;
        // Intégrations décrochées : depuis quand, et prévenu ou pas encore.
        let mut down: HashMap<String, (Instant, bool)> = HashMap::new();
        let mut minute = tokio::time::interval(Duration::from_secs(60));
        let send = |title: &str, message: String, level: Level| {
            let _ = notify.send(NotifyRequest {
                title: Some(title.into()),
                message,
                level,
                to: Vec::new(),
                except: Vec::new(),
                action: None,
            });
        };
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, new, .. }) => {
                        let cfg: AlertSettings = settings.get_or_default(settings::ALERTS);
                        let Some(e) = registry.get(&entity_id).await else { continue };
                        if e.meta.hidden {
                            continue;
                        }
                        match kind(&e) {
                            "battery" if cfg.battery => {
                                // « Pile changée » : niveau effacé, l'alerte est réarmée.
                                if new.value.is_null() {
                                    low_battery.remove(&device_key(&e));
                                    continue;
                                }
                                let Some(level) = new.value.as_f64() else { continue };
                                let key = device_key(&e);
                                if level <= cfg.battery_threshold as f64 {
                                    if low_battery.insert(key) {
                                        send("Pile faible", format!("{} : {} % de pile. Pensez à la changer.", device_name(&e), level.round()), Level::Info);
                                    }
                                } else if level > cfg.battery_threshold as f64 + 10.0 {
                                    // Pile changée : on pourra prévenir à nouveau la prochaine fois.
                                    low_battery.remove(&key);
                                }
                            }
                            "water_leak" if cfg.leak && new.value == "on" => {
                                send("Fuite d'eau", format!("{} détecte de l'eau !", device_name(&e)), Level::Urgent);
                            }
                            _ => {}
                        }
                        silent.remove(&device_key(&e));
                    }
                    Ok(Event::IntegrationStatus { name, status, .. }) => match status {
                        IntegrationState::Disconnected | IntegrationState::Failed => {
                            down.entry(name).or_insert((Instant::now(), false));
                        }
                        IntegrationState::Connected => {
                            if let Some((_, true)) = down.remove(&name) {
                                send(&format!("{} de retour", label(&name)), "La connexion est revenue.".into(), Level::Info);
                            }
                        }
                        // Désactivée exprès : rien à signaler.
                        IntegrationState::Stopped => {
                            down.remove(&name);
                        }
                        IntegrationState::Starting => {}
                    },
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = minute.tick() => {
                    let cfg: AlertSettings = settings.get_or_default(settings::ALERTS);
                    if !cfg.integrations {
                        continue;
                    }
                    for (name, (since, warned)) in down.iter_mut() {
                        if !*warned && since.elapsed() >= INTEGRATION_GRACE {
                            *warned = true;
                            send(
                                &format!("{} injoignable", label(name)),
                                format!("Heim n'arrive plus à joindre {} depuis 5 minutes : les appareils concernés ne répondent plus.", label(name)),
                                Level::Important,
                            );
                        }
                    }
                }
                _ = sweep.tick() => {
                    let cfg: AlertSettings = settings.get_or_default(settings::ALERTS);
                    if !cfg.silent {
                        continue;
                    }
                    let limit = Utc::now() - TimeDelta::hours(cfg.silent_hours as i64);
                    for e in registry.snapshot().await {
                        // Seuls les vrais appareils, et ceux qu'on a déjà entendus au moins une fois.
                        if e.meta.hidden || !e.meta.domain.physical() && !matches!(e.meta.domain, crate::model::Domain::Sensor | crate::model::Domain::BinarySensor) {
                            continue;
                        }
                        if e.meta.integration != crate::integrations::zigbee2mqtt::INTEGRATION {
                            continue;
                        }
                        let Some(seen) = e.state.as_ref().and_then(|s| s.last_seen) else { continue };
                        let key = device_key(&e);
                        if seen < limit {
                            if silent.insert(key) {
                                send(
                                    "Appareil muet",
                                    format!("{} ne donne plus de nouvelles depuis plus de {} h (pile vide ou trop loin ?).", device_name(&e), cfg.silent_hours),
                                    Level::Info,
                                );
                            }
                        } else {
                            silent.remove(&key);
                        }
                    }
                }
            }
        }
    });
}
