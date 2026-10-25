//! Bus d'événements interne. Rien n'appelle rien directement : tout passe par ici.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::model::{EntityId, EntityView, State, Target};
use crate::notify::Notification;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationState {
    Starting,
    Connected,
    Disconnected,
    Failed,
    /// Désactivée par l'utilisateur.
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Une entité est apparue ou ses métadonnées ont changé (nom, pièce, position...).
    EntityUpdated {
        entity: Arc<EntityView>,
    },
    EntityRemoved {
        entity_id: EntityId,
    },
    StateChanged {
        entity_id: EntityId,
        old: Option<State>,
        new: State,
    },
    /// Le noyau demande à une intégration d'exécuter une commande.
    CommandRequested {
        integration: String,
        entity_id: EntityId,
        target: Target,
        source: String,
    },
    /// Un appareil n'a pas confirmé la commande à temps.
    CommandTimeout {
        entity_id: EntityId,
        target: Target,
    },
    IntegrationStatus {
        name: String,
        status: IntegrationState,
        detail: Option<String>,
    },
    /// Un réglage a changé (connexion, tableau de bord...) : les écrans ouverts se mettent à jour.
    SettingsChanged {
        key: String,
    },
    /// Un automatisme vient de s'exécuter.
    AutomationRan {
        id: String,
        ok: bool,
        detail: Option<String>,
    },
    /// Un déclencheur a réagi mais une condition a empêché l'automatisme de se lancer.
    AutomationSkipped {
        id: String,
        name: String,
        reason: String,
    },
    /// Une ligne pour le journal (ce qui n'est pas déjà un changement d'état).
    Log {
        kind: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        entity_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        actor: Option<String>,
    },
    /// Demande d'ouvrir (secondes > 0) ou de fermer (0) l'appairage Zigbee.
    ZigbeePermitJoin {
        seconds: u32,
    },
    /// État du pont Zigbee2MQTT : appairage ouvert jusqu'à quand, version.
    ZigbeeBridge {
        permit_join: bool,
        until: Option<chrono::DateTime<chrono::Utc>>,
        version: Option<String>,
    },
    /// Un appareil rejoint le réseau, est interrogé, quitte le réseau...
    ZigbeeDevice {
        event: String,
        friendly_name: String,
        detail: Option<String>,
    },
    /// Changer des réglages d'un appareil (couleur, comportement après coupure...).
    DeviceSet {
        integration: String,
        device_id: String,
        values: serde_json::Map<String, serde_json::Value>,
    },
    /// Action sur un appareil : « remove », « ota_check », « ota_update », « interview ».
    DeviceAction {
        integration: String,
        device_id: String,
        action: String,
        force: bool,
    },
    /// Un abonnement push n'est plus valable (application supprimée du téléphone).
    PushExpired {
        target_id: String,
    },
    /// L'alarme a changé d'état.
    Security {
        status: serde_json::Value,
    },
    /// Notification destinée à une seule cible (un téléphone, un écran).
    Notification {
        target_id: String,
        notification: Arc<Notification>,
    },
}

#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Event>,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publie sans jamais bloquer ; personne à l'écoute n'est pas une erreur.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn integration_status(&self, name: &str, status: IntegrationState, detail: Option<String>) {
        self.publish(Event::IntegrationStatus {
            name: name.to_string(),
            status,
            detail,
        });
    }
}
