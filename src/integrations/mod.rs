//! Intégrations : les passerelles vers le monde réel.
//!
//! Chaque intégration tourne dans sa propre tâche, surveillée : si elle plante ou
//! s'arrête sur une erreur, elle est relancée avec un délai croissant (1 s → 60 s),
//! et le reste de Heim continue de tourner.

pub mod matter;
pub mod mqtt_discovery;
pub mod tuya;
pub mod tuya_cloud;
pub mod tuya_light;
pub mod tuya_proto;
pub mod virtual_dev;
pub mod zigbee2mqtt;

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use std::time::{Duration, Instant};

use crate::bus::{Bus, IntegrationState};

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Une intégration qui a tenu ce temps est considérée comme saine : le délai repart du minimum.
const HEALTHY_AFTER: Duration = Duration::from_secs(120);

/// Lance une intégration surveillée. Arrêter la poignée renvoyée (`abort`) arrête
/// l'intégration pour de bon : c'est ainsi qu'on la reconfigure à chaud.
pub fn supervise<F, Fut>(name: &'static str, bus: Bus, mut make: F) -> tokio::task::JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = MIN_BACKOFF;
        loop {
            bus.integration_status(name, IntegrationState::Starting, None);
            let started = Instant::now();
            // Le plantage est rattrapé ici, dans la même tâche : arrêter la tâche arrête tout.
            let detail = match AssertUnwindSafe(make()).catch_unwind().await {
                Ok(Ok(())) => {
                    tracing::info!("intégration {name} arrêtée proprement");
                    return;
                }
                Ok(Err(e)) => format!("{e:#}"),
                Err(_) => "plantage interne (panic)".to_string(),
            };
            if started.elapsed() > HEALTHY_AFTER {
                backoff = MIN_BACKOFF;
            }
            tracing::error!("intégration {name} en échec : {detail} — relance dans {backoff:?}");
            bus.integration_status(name, IntegrationState::Failed, Some(detail));
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    })
}
