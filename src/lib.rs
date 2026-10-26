//! Heim — noyau domotique léger.
//!
//! Organisation :
//! - [`registry`] : propriétaire unique des entités et de leurs états ;
//! - [`bus`] : événements internes ;
//! - [`scenes`], [`timers`], [`notify`], [`automations`], [`alerts`] : les fonctions qui font la différence ;
//! - [`settings`] : réglages modifiables depuis l'interface ;
//! - [`integrations`] : passerelles vers les appareils (Zigbee2MQTT, virtuels...) ;
//! - [`store`] : persistance SQLite ; [`api`] : REST + WebSocket.

pub mod accounts;
pub mod alerts;
pub mod api;
pub mod api_accounts;
pub mod api_controls;
pub mod api_devices;
pub mod api_life;
pub mod api_more;
pub mod api_presence;
pub mod api_security;
pub mod automations;
pub mod bus;
pub mod devices;
pub mod groups;
pub mod history;
pub mod integrations;
pub mod model;
pub mod netsetup;
pub mod notify;
pub mod presence;
pub mod push;
pub mod registry;
pub mod scenes;
pub mod security;
pub mod settings;
pub mod store;
pub mod sun;
pub mod system;
pub mod timers;
pub mod update;
pub mod websec;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use crate::automations::AutomationHandle;
use crate::bus::Bus;
use crate::notify::Notifier;
use crate::registry::RegistryHandle;
use crate::scenes::SceneHandle;
use crate::settings::{Settings, Zigbee2MqttSettings};
use crate::store::{StoreHandle, Table};
use crate::timers::TimerHandle;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub http_addr: SocketAddr,
    pub token: Option<String>,
    pub demo: bool,
    pub zigbee2mqtt: Option<integrations::zigbee2mqtt::Config>,
}

impl Config {
    /// Lecture depuis les variables d'environnement (voir README).
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let http_addr = var("HEIM_HTTP")
            .unwrap_or_else(|| "0.0.0.0:8090".into())
            .parse()
            .context("HEIM_HTTP doit ressembler à 0.0.0.0:8090")?;
        let zigbee2mqtt = match var("HEIM_MQTT_HOST") {
            Some(raw) => {
                let (host, port_in_host) = clean_mqtt_host(&raw);
                let port = var("HEIM_MQTT_PORT")
                    .map(|p| p.parse())
                    .transpose()
                    .context("HEIM_MQTT_PORT invalide")?
                    .or(port_in_host)
                    .unwrap_or(1883);
                Some(integrations::zigbee2mqtt::Config {
                    host,
                    port,
                    username: var("HEIM_MQTT_USER"),
                    password: var("HEIM_MQTT_PASSWORD"),
                    base_topic: var("HEIM_Z2M_TOPIC").unwrap_or_else(|| "zigbee2mqtt".into()),
                })
            }
            None => None,
        };
        Ok(Self {
            data_dir: var("HEIM_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./data")),
            http_addr,
            token: var("HEIM_TOKEN"),
            demo: matches!(
                var("HEIM_DEMO").as_deref(),
                Some("1" | "true" | "oui" | "yes")
            ),
            zigbee2mqtt,
        })
    }
}

/// Tolère une adresse copiée depuis le navigateur : `http://192.168.1.50:8123/` -> `192.168.1.50`.
/// Le port 8123 (interface web de Home Assistant) est ignoré : MQTT n'y est jamais.
pub fn clean_mqtt_host(raw: &str) -> (String, Option<u16>) {
    let mut s = raw.trim();
    for scheme in ["mqtt://", "tcp://", "http://", "https://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
        }
    }
    let s = s.split('/').next().unwrap_or_default();
    let (host, port) = match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse::<u16>() {
            Ok(port) => (h, Some(port)),
            Err(_) => (s, None),
        },
        _ => (s, None),
    };
    let port = match port {
        Some(8123) => {
            tracing::warn!(
                "HEIM_MQTT_HOST : 8123 est le port de l'interface de Home Assistant, pas de MQTT ; port 1883 utilisé"
            );
            None
        }
        p => p,
    };
    if host != raw.trim() {
        tracing::warn!(
            "HEIM_MQTT_HOST corrigé : « {raw} » -> « {host} » (mettez seulement l'adresse IP ou le nom)"
        );
    }
    (host.to_string(), port)
}

/// Le cœur de Heim, sans le serveur HTTP. Clonable à volonté (ce ne sont que des poignées).
#[derive(Clone)]
pub struct Core {
    pub bus: Bus,
    pub registry: RegistryHandle,
    pub scenes: SceneHandle,
    pub timers: TimerHandle,
    pub notify: Arc<Notifier>,
    pub store: StoreHandle,
    pub automations: AutomationHandle,
    pub settings: Arc<Settings>,
    pub accounts: Arc<accounts::Accounts>,
    pub security: security::SecurityHandle,
    pub presence: presence::PresenceHandle,
    pub updater: Arc<update::Updater>,
    /// Réglages complets de chaque appareil (couleur, après coupure...), par identifiant.
    pub controls: devices::Controls,
    pub history: history::History,
    pub netsetup: Arc<netsetup::NetSetup>,
    /// Appairage Zigbee : ouvert jusqu'à quand, appareils arrivés récemment.
    pub pairing: Arc<std::sync::Mutex<Pairing>>,
    /// Dernier état connu de chaque intégration (Zigbee connecté, en échec...).
    pub integrations: Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
    pub data_dir: PathBuf,
    pub started: std::time::Instant,
    zigbee: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    tuya: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Appareils Tuya entendus sur le réseau (pendant que l'intégration tourne).
    pub tuya_seen: integrations::tuya::Seen,
    matter: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Core {
    /// Ouvre la base, recharge tout et démarre les moteurs. Doit tourner dans un runtime Tokio.
    pub fn start(config: &Config) -> anyhow::Result<Self> {
        let (store, mut loaded) = store::open(&config.data_dir.join("heim.db"))?;
        let bus = Bus::new(1024);
        let registry = registry::spawn(
            std::mem::take(&mut loaded.entities),
            bus.clone(),
            store.clone(),
        );
        // Journal et courbes : branchés tout de suite pour ne rien manquer du démarrage.
        let history = history::History::open(&config.data_dir.join("history.db"));
        history::spawn(history.clone(), &bus, registry.clone());
        let controls: devices::Controls = Default::default();
        let scenes = scenes::spawn(
            loaded.take(Table::Scenes),
            registry.clone(),
            bus.clone(),
            store.clone(),
            controls.clone(),
        );
        let timers = timers::spawn(
            loaded.take(Table::Timers),
            registry.clone(),
            bus.clone(),
            store.clone(),
        );
        let notify = Notifier::new(
            loaded.take(Table::NotifyTargets),
            bus.clone(),
            store.clone(),
        );
        let settings = Settings::new(loaded.take(Table::Settings), store.clone(), bus.clone());
        // Clés VAPID : créées une fois, elles identifient ce Heim auprès d'Apple et Google.
        let vapid = match settings.get::<push::Vapid>(push::SETTINGS_KEY) {
            Some(v) => v,
            None => {
                let v = push::Vapid::generate();
                let _ = settings.set(push::SETTINGS_KEY, &v);
                v
            }
        };
        notify.set_vapid(vapid);
        spawn_push_cleanup(&bus, notify.clone());
        let accounts = accounts::Accounts::new(
            loaded.take(Table::Users),
            loaded.take(Table::Sessions),
            store.clone(),
        );
        let automations = automations::spawn(
            loaded.take(Table::Automations),
            automations::Deps {
                registry: registry.clone(),
                timers: timers.clone(),
                notify: notify.clone(),
                settings: settings.clone(),
                bus: bus.clone(),
                store: store.clone(),
            },
        );
        alerts::spawn(registry.clone(), notify.clone(), settings.clone(), &bus);
        groups::spawn(registry.clone(), &bus, settings.clone());
        system::spawn_watchdog(registry.clone());
        system::spawn_nightly_backup(store.clone(), config.data_dir.clone(), bus.clone());
        bus.publish(bus::Event::Log {
            kind: "system".into(),
            message: format!("Heim {} démarré", env!("CARGO_PKG_VERSION")),
            entity_id: None,
            actor: None,
        });
        let security = security::spawn(security::Deps {
            registry: registry.clone(),
            notify: notify.clone(),
            settings: settings.clone(),
            bus: bus.clone(),
            data_dir: config.data_dir.clone(),
        });
        let presence = presence::spawn(presence::Deps {
            registry: registry.clone(),
            notify: notify.clone(),
            settings: settings.clone(),
            accounts: accounts.clone(),
            security: security.clone(),
            bus: bus.clone(),
        });
        let netsetup = netsetup::NetSetup::new();
        netsetup.spawn_watchdog();
        let updater = update::Updater::new(config.data_dir.clone());
        updater.confirm_after_start();
        let pairing = Arc::new(std::sync::Mutex::new(Pairing::default()));
        spawn_pairing_tracker(&bus, pairing.clone());
        let integrations = Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
        spawn_integration_tracker(&bus, integrations.clone());
        spawn_timer_announcer(&bus, registry.clone(), notify.clone());

        // Première fois avec des variables d'environnement : on les reprend dans les réglages,
        // ensuite tout se règle depuis l'interface.
        if settings.get_raw(settings::ZIGBEE2MQTT).is_none()
            && let Some(cfg) = &config.zigbee2mqtt
        {
            let saved = Zigbee2MqttSettings {
                enabled: true,
                host: cfg.host.clone(),
                port: cfg.port,
                username: cfg.username.clone(),
                password: cfg.password.clone(),
                base_topic: cfg.base_topic.clone(),
            };
            if settings.set(settings::ZIGBEE2MQTT, &saved).is_ok() {
                tracing::info!(
                    "connexion Zigbee2MQTT reprise des variables d'environnement : elle se règle désormais dans l'interface"
                );
            }
        }

        let core = Core {
            bus,
            registry,
            scenes,
            timers,
            notify,
            store,
            automations,
            settings,
            accounts,
            security,
            presence,
            updater,
            controls,
            history,
            netsetup,
            pairing,
            integrations,
            data_dir: config.data_dir.clone(),
            started: std::time::Instant::now(),
            zigbee: Arc::new(std::sync::Mutex::new(None)),
            tuya: Arc::new(std::sync::Mutex::new(None)),
            tuya_seen: Default::default(),
            matter: Arc::new(std::sync::Mutex::new(None)),
        };
        core.start_integrations(config);
        if !config.demo {
            core.purge_demo_devices();
        }
        core.apply_zigbee_settings();
        core.apply_tuya_settings();
        core.apply_matter_settings();
        core.spawn_update_checker();
        core.spawn_power_on_for_new();
        Ok(core)
    }

    /// (Re)démarre ou arrête Zigbee2MQTT selon les réglages. Appelé au démarrage et à chaque
    /// modification depuis l'interface : pas besoin de redémarrer Heim.
    pub fn apply_zigbee_settings(&self) {
        let mut slot = self.zigbee.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.take() {
            previous.abort();
        }
        let wanted: Option<Zigbee2MqttSettings> = self.settings.get(settings::ZIGBEE2MQTT);
        match wanted {
            Some(s) if s.enabled && !s.host.trim().is_empty() => {
                let (host, port_in_host) = clean_mqtt_host(&s.host);
                let cfg = integrations::zigbee2mqtt::Config {
                    host,
                    port: port_in_host.unwrap_or(s.port),
                    username: s.username.filter(|u| !u.is_empty()),
                    password: s.password.filter(|p| !p.is_empty()),
                    base_topic: if s.base_topic.trim().is_empty() {
                        "zigbee2mqtt".into()
                    } else {
                        s.base_topic
                    },
                };
                let (registry, bus, controls) = (
                    self.registry.clone(),
                    self.bus.clone(),
                    self.controls.clone(),
                );
                *slot = Some(integrations::supervise(
                    integrations::zigbee2mqtt::INTEGRATION,
                    self.bus.clone(),
                    move || {
                        integrations::zigbee2mqtt::run(
                            cfg.clone(),
                            registry.clone(),
                            bus.clone(),
                            controls.clone(),
                        )
                    },
                ));
            }
            Some(_) => self.bus.integration_status(
                integrations::zigbee2mqtt::INTEGRATION,
                bus::IntegrationState::Stopped,
                None,
            ),
            None => {}
        }
    }

    /// (Re)démarre Tuya selon les réglages (appelé au démarrage et à chaque modification).
    pub fn apply_tuya_settings(&self) {
        use integrations::tuya;
        let mut slot = self.tuya.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.take() {
            previous.abort();
        }
        let wanted: tuya::TuyaSettings = self.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
        if wanted.enabled && !wanted.devices.is_empty() {
            let (registry, bus, seen, controls) = (
                self.registry.clone(),
                self.bus.clone(),
                self.tuya_seen.clone(),
                self.controls.clone(),
            );
            *slot = Some(integrations::supervise(
                tuya::INTEGRATION,
                self.bus.clone(),
                move || {
                    tuya::run(
                        wanted.clone(),
                        registry.clone(),
                        bus.clone(),
                        seen.clone(),
                        controls.clone(),
                    )
                },
            ));
        } else if self.settings.get_raw(tuya::SETTINGS_KEY).is_some() {
            self.bus
                .integration_status(tuya::INTEGRATION, bus::IntegrationState::Stopped, None);
        }
    }

    /// Vérifie les mises à jour toutes les 12 h, et prévient les admins une fois par version.
    fn spawn_update_checker(&self) {
        let core = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(120)).await;
            let mut announced: Option<String> = core.settings.get("update.announced");
            loop {
                let s: update::UpdateSettings =
                    core.settings.get(update::SETTINGS_KEY).unwrap_or_default();
                if s.auto_check && s.repo().is_some() {
                    let st = core.updater.check(&s).await;
                    if let (true, Some(r)) = (st.available, st.latest.as_ref())
                        && announced.as_deref() != Some(r.version.as_str())
                    {
                        let admins: Vec<String> = core
                            .accounts
                            .list()
                            .into_iter()
                            .filter(|u| u.role == accounts::Role::Admin)
                            .map(|u| format!("person:{}", u.id))
                            .collect();
                        let _ = core.notify.send(notify::NotifyRequest {
                            title: Some("Mise à jour disponible".into()),
                            message: format!(
                                "Heim {} est disponible : Réglages > Système.",
                                r.version
                            ),
                            level: notify::Level::Info,
                            to: admins,
                            except: Vec::new(),
                            action: Some(serde_json::json!({ "open": "#reglages/systeme" })),
                        });
                        announced = Some(r.version.clone());
                        let _ = core.settings.set("update.announced", &announced);
                    }
                }
                tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
            }
        });
    }

    /// Nouvel appareil Zigbee : on lui donne le comportement après coupure choisi pour tous.
    fn spawn_power_on_for_new(&self) {
        let core = self.clone();
        let mut rx = self.bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(bus::Event::ZigbeeDevice {
                        event,
                        friendly_name,
                        detail,
                    }) if event == "device_interview"
                        && detail
                            .as_deref()
                            .is_some_and(|d| d.starts_with("successful")) =>
                    {
                        let s: api_controls::PowerOnSettings = core
                            .settings
                            .get(api_controls::POWER_ON_KEY)
                            .unwrap_or_default();
                        if let (true, Some(b)) = (s.apply_new, s.behavior) {
                            let core = core.clone();
                            tokio::spawn(async move {
                                // Le temps que Zigbee2MQTT publie la nouvelle liste d'appareils.
                                tokio::time::sleep(Duration::from_secs(8)).await;
                                let (n, _) =
                                    api_controls::apply_power_on(&core, &b, Some(&friendly_name));
                                if n > 0 {
                                    tracing::info!(
                                        "{friendly_name} : après coupure de courant réglé sur « {b} »"
                                    );
                                }
                            });
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub fn tuya_running(&self) -> bool {
        self.tuya
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// (Re)démarre la connexion au serveur Matter selon les réglages.
    pub fn apply_matter_settings(&self) {
        use integrations::matter;
        let mut slot = self.matter.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.take() {
            previous.abort();
        }
        let wanted: matter::MatterSettings =
            self.settings.get(matter::SETTINGS_KEY).unwrap_or_default();
        if wanted.enabled && !wanted.url.trim().is_empty() {
            let (registry, bus) = (self.registry.clone(), self.bus.clone());
            *slot = Some(integrations::supervise(
                matter::INTEGRATION,
                self.bus.clone(),
                move || matter::run(wanted.clone(), registry.clone(), bus.clone()),
            ));
        } else if self.settings.get_raw(matter::SETTINGS_KEY).is_some() {
            self.bus
                .integration_status(matter::INTEGRATION, bus::IntegrationState::Stopped, None);
        }
    }

    /// Sans le mode démo, les appareils virtuels restés en base n'ont plus rien à faire là :
    /// on les retire, avec leurs minuteries de carte.
    fn purge_demo_devices(&self) {
        let (registry, timers) = (self.registry.clone(), self.timers.clone());
        tokio::spawn(async move {
            let mut removed = 0;
            for e in registry.snapshot().await {
                if e.meta.integration == integrations::virtual_dev::INTEGRATION {
                    registry.remove(&e.meta.entity_id).await;
                    timers
                        .delete(&timers::auto_timer_id(&e.meta.entity_id))
                        .await;
                    removed += 1;
                }
            }
            if removed > 0 {
                tracing::info!("{removed} entités de démonstration retirées");
            }
        });
    }

    fn start_integrations(&self, config: &Config) {
        if config.demo {
            let (registry, bus) = (self.registry.clone(), self.bus.clone());
            integrations::supervise(
                integrations::virtual_dev::INTEGRATION,
                self.bus.clone(),
                move || {
                    integrations::virtual_dev::run(
                        registry.clone(),
                        bus.clone(),
                        Duration::from_millis(150),
                    )
                },
            );
        }
    }
}

/// Ce que l'interface affiche pendant l'appairage.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Pairing {
    pub open: bool,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    pub version: Option<String>,
    /// Derniers événements, du plus récent au plus ancien.
    pub events: Vec<PairingEvent>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PairingEvent {
    pub at: chrono::DateTime<chrono::Utc>,
    pub event: String,
    pub friendly_name: String,
    pub detail: Option<String>,
}

fn spawn_pairing_tracker(bus: &Bus, pairing: Arc<std::sync::Mutex<Pairing>>) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bus::Event::ZigbeeBridge {
                    permit_join,
                    until,
                    version,
                }) => {
                    let mut p = pairing.lock().unwrap_or_else(|e| e.into_inner());
                    p.open = permit_join;
                    p.until = until;
                    if version.is_some() {
                        p.version = version;
                    }
                }
                Ok(bus::Event::ZigbeeDevice {
                    event,
                    friendly_name,
                    detail,
                }) => {
                    let mut p = pairing.lock().unwrap_or_else(|e| e.into_inner());
                    p.events.insert(
                        0,
                        PairingEvent {
                            at: chrono::Utc::now(),
                            event,
                            friendly_name,
                            detail,
                        },
                    );
                    p.events.truncate(20);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_push_cleanup(bus: &Bus, notify: Arc<Notifier>) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bus::Event::PushExpired { target_id }) => notify.unsubscribe(&target_id),
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_integration_tracker(
    bus: &Bus,
    map: Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bus::Event::IntegrationStatus {
                    name,
                    status,
                    detail,
                }) => {
                    let entry = serde_json::json!({ "status": status, "detail": detail, "since": chrono::Utc::now() });
                    map.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(name, entry);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Prévient tous les appareils quand un minuteur se termine.
fn spawn_timer_announcer(bus: &Bus, registry: RegistryHandle, notify: Arc<Notifier>) {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bus::Event::StateChanged {
                    entity_id,
                    old,
                    new,
                }) if entity_id.starts_with("timer.") => {
                    let was_running = old.as_ref().is_some_and(|o| o.value == "running");
                    if was_running && new.value == "finished" {
                        let entity = registry.get(&entity_id).await;
                        // « Allumer pour 30 min » : l'appareil s'éteint, pas besoin de prévenir.
                        if entity
                            .as_ref()
                            .is_some_and(|e| e.meta.capabilities.contains_key("auto"))
                        {
                            continue;
                        }
                        let name = match entity {
                            Some(e) => e.meta.name,
                            None => entity_id.trim_start_matches("timer.").replace('_', " "),
                        };
                        let _ = notify.send(notify::NotifyRequest {
                            title: Some("Minuteur terminé".into()),
                            message: format!("« {name} » est terminé."),
                            level: notify::Level::Important,
                            to: Vec::new(),
                            except: Vec::new(),
                            action: None,
                        });
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Démarre Heim et sert l'API jusqu'à Ctrl+C / SIGTERM.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let core = Core::start(&config)?;
    let listener = tokio::net::TcpListener::bind(config.http_addr)
        .await
        .with_context(|| format!("impossible d'écouter sur {}", config.http_addr))?;
    tracing::info!(
        "Heim {} prêt sur http://{}",
        env!("CARGO_PKG_VERSION"),
        config.http_addr
    );
    if config.token.is_none() {
        tracing::warn!("aucun HEIM_TOKEN : l'API est ouverte à tout le réseau local");
    }
    if core.settings.get_raw(settings::ZIGBEE2MQTT).is_none() && !config.demo {
        tracing::warn!(
            "aucune intégration active : ouvrez l'interface pour connecter Zigbee2MQTT (ou HEIM_DEMO=1)"
        );
    }

    let app = api::router(core.clone(), config.token.clone(), config.http_addr.port());
    // Adresses d'écoute en plus (l'image SD : port 80 pour le portail captif et heim.local,
    // et 8090 comme avant). HEIM_HTTP_ALSO=0.0.0.0:8090,0.0.0.0:8091
    let (stop_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let mut extra = Vec::new();
    for addr in std::env::var("HEIM_HTTP_ALSO")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        match addr.parse::<SocketAddr>() {
            Ok(a) => match tokio::net::TcpListener::bind(a).await {
                Ok(l) => {
                    tracing::info!("aussi sur http://{a}");
                    let app = app.clone();
                    let mut stop = stop_tx.subscribe();
                    extra.push(tokio::spawn(async move {
                        let _ = axum::serve(l, app)
                            .with_graceful_shutdown(async move {
                                let _ = stop.recv().await;
                            })
                            .await;
                    }));
                }
                Err(e) => tracing::warn!("impossible d'écouter aussi sur {a} : {e}"),
            },
            Err(_) => tracing::warn!("HEIM_HTTP_ALSO : adresse « {addr} » invalide"),
        }
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    let _ = stop_tx.send(());
    for h in extra {
        let _ = h.await;
    }

    tracing::info!("arrêt : écriture des derniers états");
    let store = core.store.clone();
    let history = core.history.clone();
    tokio::task::spawn_blocking(move || {
        store.flush_blocking();
        history::shutdown(&history);
    })
    .await
    .ok();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::clean_mqtt_host;

    #[test]
    fn mqtt_host_is_forgiving() {
        assert_eq!(
            clean_mqtt_host("192.168.1.50"),
            ("192.168.1.50".into(), None)
        );
        assert_eq!(
            clean_mqtt_host("http://192.168.1.50:8123/"),
            ("192.168.1.50".into(), None)
        );
        assert_eq!(
            clean_mqtt_host("mqtt://broker.local:1884"),
            ("broker.local".into(), Some(1884))
        );
        assert_eq!(
            clean_mqtt_host(" homeassistant.local "),
            ("homeassistant.local".into(), None)
        );
    }
}
