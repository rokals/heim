//! Appareils virtuels, pour tester Heim sans matériel (HEIM_DEMO=1) et pour les tests automatiques.
//! Ils obéissent aux commandes comme de vraies ampoules, avec un petit délai.

use std::time::Duration;

use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Attributes, Category, Domain, EntityMeta};
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "virtual";

#[derive(Clone)]
enum V {
    N(f64),
    S(&'static str),
}

impl V {
    fn into_value(self) -> serde_json::Value {
        match self {
            V::N(n) => json!(n),
            V::S(s) => json!(s),
        }
    }
}

struct Reading {
    id: &'static str,
    name: &'static str,
    device: &'static str,
    device_name: &'static str,
    domain: Domain,
    room: &'static str,
    kind: &'static str,
    unit: &'static str,
    diagnostic: bool,
    value: V,
}

const READINGS: &[Reading] = &[
    Reading {
        id: "sensor.demo_temperature",
        name: "Température",
        device: "thermo",
        device_name: "Thermomètre salon",
        domain: Domain::Sensor,
        room: "Salon",
        kind: "temperature",
        unit: "°C",
        diagnostic: false,
        value: V::N(20.5),
    },
    Reading {
        id: "sensor.demo_humidite",
        name: "Humidité",
        device: "thermo",
        device_name: "Thermomètre salon",
        domain: Domain::Sensor,
        room: "Salon",
        kind: "humidity",
        unit: "%",
        diagnostic: false,
        value: V::N(48.0),
    },
    Reading {
        id: "sensor.demo_thermo_batterie",
        name: "Batterie",
        device: "thermo",
        device_name: "Thermomètre salon",
        domain: Domain::Sensor,
        room: "Salon",
        kind: "battery",
        unit: "%",
        diagnostic: true,
        value: V::N(87.0),
    },
    Reading {
        id: "binary_sensor.demo_porte",
        name: "Ouverture",
        device: "porte",
        device_name: "Porte d'entrée",
        domain: Domain::BinarySensor,
        room: "Entrée",
        kind: "contact",
        unit: "",
        diagnostic: false,
        value: V::S("off"),
    },
    Reading {
        id: "sensor.demo_porte_batterie",
        name: "Batterie",
        device: "porte",
        device_name: "Porte d'entrée",
        domain: Domain::Sensor,
        room: "Entrée",
        kind: "battery",
        unit: "%",
        diagnostic: true,
        value: V::N(14.0),
    },
    Reading {
        id: "binary_sensor.demo_mouvement",
        name: "Mouvement",
        device: "mouvement",
        device_name: "Détecteur couloir",
        domain: Domain::BinarySensor,
        room: "Couloir",
        kind: "occupancy",
        unit: "",
        diagnostic: false,
        value: V::S("off"),
    },
    Reading {
        id: "sensor.demo_prise_puissance",
        name: "Puissance",
        device: "switch.demo_prise_cafe",
        device_name: "Prise cafetière",
        domain: Domain::Sensor,
        room: "Cuisine",
        kind: "power",
        unit: "W",
        diagnostic: false,
        value: V::N(0.0),
    },
    Reading {
        id: "binary_sensor.ts0601_x7k3_occupancy",
        name: "TS0601_x7k3 occupancy",
        device: "ts0601",
        device_name: "TS0601_x7k3",
        domain: Domain::BinarySensor,
        room: "",
        kind: "occupancy",
        unit: "",
        diagnostic: false,
        value: V::S("off"),
    },
    Reading {
        id: "sensor.ts0601_x7k3_illuminance",
        name: "TS0601_x7k3 illuminance",
        device: "ts0601",
        device_name: "TS0601_x7k3",
        domain: Domain::Sensor,
        room: "",
        kind: "illuminance",
        unit: "lx",
        diagnostic: false,
        value: V::N(132.0),
    },
];

struct Def {
    id: &'static str,
    name: &'static str,
    domain: Domain,
    room: &'static str,
    dimmable: bool,
}

const DEVICES: &[Def] = &[
    Def {
        id: "light.demo_salon",
        name: "Plafonnier salon",
        domain: Domain::Light,
        room: "Salon",
        dimmable: true,
    },
    Def {
        id: "light.demo_lampadaire",
        name: "Lampadaire",
        domain: Domain::Light,
        room: "Salon",
        dimmable: true,
    },
    Def {
        id: "light.demo_chambre",
        name: "Lampe chambre",
        domain: Domain::Light,
        room: "Chambre",
        dimmable: false,
    },
    Def {
        id: "switch.demo_prise_cafe",
        name: "Prise cafetière",
        domain: Domain::Switch,
        room: "Cuisine",
        dimmable: false,
    },
    Def {
        id: "siren.demo_sirene",
        name: "Sirène garage",
        domain: Domain::Siren,
        room: "Garage",
        dimmable: false,
    },
];

pub async fn run(registry: RegistryHandle, bus: Bus, latency: Duration) -> anyhow::Result<()> {
    let mut rx = bus.subscribe();
    for d in DEVICES {
        let mut capabilities = Attributes::new();
        capabilities.insert("device_name".into(), json!(d.name));
        if d.dimmable {
            capabilities.insert("brightness".into(), json!(true));
        }
        registry
            .upsert(EntityMeta {
                entity_id: d.id.into(),
                domain: d.domain,
                name: d.name.into(),
                integration: INTEGRATION.into(),
                device_id: Some(format!("virtual:{}", d.id)),
                room: Some(d.room.into()),
                plan: None,
                capabilities,
                user_edited: false,
                category: Category::Primary,
                hidden: false,
            })
            .await;
        let state = registry.get(d.id).await.and_then(|e| e.state);
        let value = state
            .as_ref()
            .map(|s| s.value.clone())
            .filter(|v| !v.is_null() && d.domain != Domain::Siren)
            .unwrap_or(json!("off"));
        let mut attrs = Attributes::new();
        if d.dimmable {
            let b = state.and_then(|s| s.brightness()).unwrap_or(180);
            attrs.insert("brightness".into(), json!(b));
        }
        registry.report(d.id, value, attrs, true).await;
    }
    // Capteurs : plusieurs entités par appareil, comme un vrai capteur Zigbee.
    for r in READINGS {
        let mut capabilities = Attributes::new();
        capabilities.insert("device_name".into(), json!(r.device_name));
        capabilities.insert("kind".into(), json!(r.kind));
        if !r.unit.is_empty() {
            capabilities.insert("unit".into(), json!(r.unit));
        }
        registry
            .upsert(EntityMeta {
                entity_id: r.id.into(),
                domain: r.domain,
                name: r.name.into(),
                integration: INTEGRATION.into(),
                device_id: Some(format!("virtual:{}", r.device)),
                room: (!r.room.is_empty()).then(|| r.room.into()),
                plan: None,
                capabilities,
                user_edited: false,
                category: if r.diagnostic {
                    Category::Diagnostic
                } else {
                    Category::Primary
                },
                hidden: false,
            })
            .await;
        registry
            .report(r.id, r.value.clone().into_value(), Attributes::new(), true)
            .await;
    }
    bus.integration_status(INTEGRATION, IntegrationState::Connected, None);

    let mut temp: f64 = 20.5;
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                // Petite dérive pseudo-aléatoire, sans dépendance.
                let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
                temp = (temp + (nanos % 5) as f64 / 10.0 - 0.2).clamp(17.0, 25.0);
                let rounded = (temp * 10.0).round() / 10.0;
                registry.report("sensor.demo_temperature", json!(rounded), Attributes::new(), true).await;
            }
            ev = rx.recv() => match ev {
                Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                    let registry = registry.clone();
                    let dimmable = DEVICES.iter().any(|d| d.id == entity_id && d.dimmable);
                    tokio::spawn(async move {
                        tokio::time::sleep(latency).await;
                        let mut attrs = Attributes::new();
                        if dimmable && target.on
                            && let Some(b) = target.brightness {
                                attrs.insert("brightness".into(), json!(b));
                            }
                        if entity_id == "switch.demo_prise_cafe" {
                            let watts = if target.on { 850.0 } else { 0.0 };
                            registry.report("sensor.demo_prise_puissance", json!(watts), Attributes::new(), true).await;
                        }
                        registry.report(entity_id, json!(if target.on { "on" } else { "off" }), attrs, true).await;
                    });
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}
