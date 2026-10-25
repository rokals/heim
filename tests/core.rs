//! Tests de bout en bout du noyau, avec les appareils virtuels (aucun matériel requis).

use std::time::Duration;

use heim::model::{Action, EntityView, Target};
use heim::scenes::SceneInput;
use heim::timers::{FinishAction, TimerInput, TimerOp};
use heim::{Config, Core};
use serde_json::json;

fn config(dir: &std::path::Path, demo: bool) -> Config {
    Config {
        data_dir: dir.to_path_buf(),
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo,
        zigbee2mqtt: None,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("heim-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Attend qu'une entité vérifie une condition (2 s max).
async fn wait_for(
    core: &Core,
    id: &str,
    what: &str,
    cond: impl Fn(&EntityView) -> bool,
) -> EntityView {
    for _ in 0..200 {
        if let Some(e) = core.registry.get(id).await
            && cond(&e)
        {
            return e;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{id} : condition non atteinte ({what}) — état : {:?}",
        core.registry.get(id).await
    );
}

fn value(e: &EntityView) -> String {
    e.state
        .as_ref()
        .and_then(|s| s.value.as_str())
        .unwrap_or_default()
        .to_string()
}

async fn ready(core: &Core) {
    for id in [
        "light.demo_salon",
        "light.demo_chambre",
        "switch.demo_prise_cafe",
    ] {
        wait_for(core, id, "disponible", |e| {
            e.state.as_ref().is_some_and(|s| s.available)
        })
        .await;
    }
}

#[tokio::test]
async fn scene_tracks_real_state_and_restores() {
    let dir = temp_dir("scene");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;

    let scene = core
        .scenes
        .upsert(
            "",
            SceneInput {
                name: "Soirée film".into(),
                room: Some("Salon".into()),
                targets: [
                    (
                        "light.demo_salon".to_string(),
                        Target {
                            on: true,
                            brightness: Some(60),
                            color_temp: None,
                        },
                    ),
                    ("light.demo_chambre".to_string(), Target::on()),
                ]
                .into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(scene.scene.id, "soiree_film");
    wait_for(&core, "scene.soiree_film", "inactive au départ", |e| {
        value(e) == "off"
    })
    .await;

    // Activation : la scène devient active quand les lampes ont réellement obéi.
    core.registry
        .command(
            "scene.soiree_film",
            Action::TurnOn {
                brightness: None,
                color_temp: None,
            },
            "test",
        )
        .await
        .unwrap();
    wait_for(&core, "scene.soiree_film", "active", |e| value(e) == "on").await;
    let salon = core.registry.get("light.demo_salon").await.unwrap();
    assert_eq!(salon.state.unwrap().brightness(), Some(60));

    // Désactivation : retour à l'état d'avant (tout éteint).
    core.registry
        .command("scene.soiree_film", Action::TurnOff, "test")
        .await
        .unwrap();
    wait_for(&core, "light.demo_salon", "restaurée éteinte", |e| {
        value(e) == "off"
    })
    .await;
    wait_for(&core, "light.demo_chambre", "restaurée éteinte", |e| {
        value(e) == "off"
    })
    .await;
    wait_for(&core, "scene.soiree_film", "inactive", |e| {
        value(e) == "off"
    })
    .await;

    // Réactivation, puis quelqu'un éteint une lampe à la main : la scène se désactive seule.
    core.registry
        .command("scene.soiree_film", Action::Toggle, "test")
        .await
        .unwrap();
    wait_for(&core, "scene.soiree_film", "active", |e| value(e) == "on").await;
    core.registry
        .command("light.demo_chambre", Action::TurnOff, "interrupteur")
        .await
        .unwrap();
    wait_for(
        &core,
        "scene.soiree_film",
        "inactive après changement manuel",
        |e| value(e) == "off",
    )
    .await;
    let list = core.scenes.list().await;
    assert!(
        list[0].scene.restore.is_none(),
        "l'état à restaurer doit être oublié"
    );

    // Désactiver une scène déjà inactive n'éteint rien par surprise.
    core.registry
        .command("scene.soiree_film", Action::TurnOff, "test")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        value(&core.registry.get("light.demo_salon").await.unwrap()),
        "on"
    );
}

#[tokio::test]
async fn capture_current_lighting() {
    let dir = temp_dir("capture");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;
    core.registry
        .command(
            "light.demo_salon",
            Action::TurnOn {
                brightness: Some(200),
                color_temp: None,
            },
            "t",
        )
        .await
        .unwrap();
    wait_for(&core, "light.demo_salon", "allumée", |e| value(e) == "on").await;

    let view = core
        .scenes
        .capture(
            "lecture",
            heim::scenes::CaptureInput {
                name: "Lecture".into(),
                room: None,
                entities: vec!["light.demo_salon".into(), "light.demo_chambre".into()],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        view.scene.targets["light.demo_salon"],
        Target {
            on: true,
            brightness: Some(200),
            color_temp: None
        }
    );
    assert_eq!(view.scene.targets["light.demo_chambre"], Target::off());
    wait_for(&core, "scene.lecture", "active dès sa création", |e| {
        value(e) == "on"
    })
    .await;

    // Une scène ne peut pas contenir un capteur.
    let err = core
        .scenes
        .upsert(
            "x",
            SceneInput {
                name: "X".into(),
                room: None,
                targets: [("sensor.demo_temperature".to_string(), Target::on())].into(),
            },
        )
        .await
        .unwrap_err();
    assert!(err.contains("seules les lumières"));
}

#[tokio::test]
async fn timer_counts_down_and_runs_its_action() {
    let dir = temp_dir("timer");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;
    core.registry
        .command(
            "switch.demo_prise_cafe",
            Action::TurnOn {
                brightness: None,
                color_temp: None,
            },
            "t",
        )
        .await
        .unwrap();
    wait_for(&core, "switch.demo_prise_cafe", "allumée", |e| {
        value(e) == "on"
    })
    .await;

    core.timers
        .upsert(
            "cafe",
            TimerInput {
                name: "Cafetière".into(),
                room: Some("Cuisine".into()),
                duration_s: 600,
                on_finish: Some(FinishAction {
                    entity_id: "switch.demo_prise_cafe".into(),
                    action: Action::TurnOff,
                }),
            },
        )
        .await
        .unwrap();

    let view = core
        .timers
        .control(
            "cafe",
            TimerOp::Start {
                duration_s: Some(1),
            },
        )
        .await
        .unwrap();
    assert!(view.remaining_s.is_some_and(|s| s <= 1));
    let running = wait_for(&core, "timer.cafe", "en cours", |e| value(e) == "running").await;
    assert!(
        running.state.unwrap().attributes["ends_at"].is_string(),
        "l'interface a besoin de l'heure de fin"
    );

    // À l'échéance (1 s) : timer terminé et prise coupée.
    tokio::time::sleep(Duration::from_millis(900)).await;
    wait_for(&core, "timer.cafe", "terminé", |e| value(e) == "finished").await;
    wait_for(
        &core,
        "switch.demo_prise_cafe",
        "coupée par le timer",
        |e| value(e) == "off",
    )
    .await;
}

#[tokio::test]
async fn timer_pause_resume_and_errors() {
    let dir = temp_dir("pause");
    let core = Core::start(&config(&dir, false)).unwrap();
    core.timers
        .upsert(
            "",
            TimerInput {
                name: "Lessive".into(),
                room: None,
                duration_s: 3600,
                on_finish: None,
            },
        )
        .await
        .unwrap();

    assert!(
        core.timers
            .control("lessive", TimerOp::Pause)
            .await
            .is_err(),
        "pas de pause sur un timer arrêté"
    );
    core.timers
        .control("lessive", TimerOp::Start { duration_s: None })
        .await
        .unwrap();
    let paused = core
        .timers
        .control("lessive", TimerOp::Pause)
        .await
        .unwrap();
    let left = paused.remaining_s.unwrap();
    assert!((3598..=3600).contains(&left), "restant : {left}");
    let e = wait_for(&core, "timer.lessive", "en pause", |e| value(e) == "paused").await;
    assert_eq!(e.state.unwrap().attributes["ends_at"], json!(null));
    core.timers
        .control("lessive", TimerOp::Resume)
        .await
        .unwrap();
    wait_for(&core, "timer.lessive", "repris", |e| value(e) == "running").await;
    assert!(
        core.timers
            .control(
                "lessive",
                TimerOp::Start {
                    duration_s: Some(0)
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn everything_survives_a_restart() {
    let dir = temp_dir("restart");
    let ends_at;
    {
        let core = Core::start(&config(&dir, true)).unwrap();
        ready(&core).await;
        core.registry
            .command(
                "light.demo_salon",
                Action::TurnOn {
                    brightness: Some(90),
                    color_temp: None,
                },
                "t",
            )
            .await
            .unwrap();
        wait_for(&core, "light.demo_salon", "allumée", |e| value(e) == "on").await;
        core.registry
            .update_meta(
                "light.demo_salon",
                serde_json::from_value(
                    json!({"name": "Grande lampe", "plan": {"x": 0.3, "y": 0.6}}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        core.timers
            .upsert(
                "four",
                TimerInput {
                    name: "Four".into(),
                    room: None,
                    duration_s: 1800,
                    on_finish: None,
                },
            )
            .await
            .unwrap();
        let v = core
            .timers
            .control("four", TimerOp::Start { duration_s: None })
            .await
            .unwrap();
        ends_at = match v.timer.status {
            heim::timers::TimerStatus::Running { ends_at } => ends_at,
            s => panic!("{s:?}"),
        };
        let store = core.store.clone();
        tokio::task::spawn_blocking(move || store.flush_blocking())
            .await
            .unwrap();
    }

    // Redémarrage sans intégration : on retrouve tout, mais l'ampoule est marquée injoignable
    // tant qu'elle n'a pas redonné signe de vie.
    let core = Core::start(&config(&dir, false)).unwrap();
    let lamp = wait_for(&core, "light.demo_salon", "restaurée", |e| {
        e.state.is_some()
    })
    .await;
    assert_eq!(lamp.meta.name, "Grande lampe");
    assert!(lamp.meta.user_edited);
    let state = lamp.state.unwrap();
    assert_eq!(state.brightness(), Some(90));
    assert!(!state.available);

    let timers = core.timers.list().await;
    assert_eq!(
        timers[0].timer.status,
        heim::timers::TimerStatus::Running { ends_at }
    );
    wait_for(&core, "timer.four", "toujours en cours", |e| {
        value(e) == "running"
    })
    .await;
}

#[tokio::test]
async fn turn_on_for_then_manual_off_cancels() {
    let dir = temp_dir("runfor");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;

    let v = core
        .timers
        .run_for("switch.demo_prise_cafe", 1800)
        .await
        .unwrap();
    assert!(v.timer.auto);
    wait_for(&core, "switch.demo_prise_cafe", "allumée", |e| {
        value(e) == "on"
    })
    .await;
    let auto = wait_for(
        &core,
        "timer.auto_switch_demo_prise_cafe",
        "minuterie en cours",
        |e| value(e) == "running",
    )
    .await;
    assert!(
        auto.meta.hidden,
        "la minuterie de carte reste invisible dans les listes"
    );

    // Éteinte à la main avant la fin : la minuterie s'annule toute seule.
    core.registry
        .command("switch.demo_prise_cafe", Action::TurnOff, "interrupteur")
        .await
        .unwrap();
    wait_for(
        &core,
        "timer.auto_switch_demo_prise_cafe",
        "annulée",
        |e| value(e) == "idle",
    )
    .await;

    // Et une vraie échéance éteint bien l'appareil.
    core.timers.run_for("light.demo_chambre", 1).await.unwrap();
    wait_for(&core, "light.demo_chambre", "allumée", |e| {
        value(e) == "on"
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    wait_for(
        &core,
        "light.demo_chambre",
        "éteinte par la minuterie",
        |e| value(e) == "off",
    )
    .await;
}

#[tokio::test]
async fn automation_reacts_to_a_state_change() {
    let dir = temp_dir("auto");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;
    wait_for(&core, "binary_sensor.demo_porte", "porte présente", |e| {
        e.state.is_some()
    })
    .await;

    let input: heim::automations::AutomationInput = serde_json::from_value(json!({
        "name": "Entrée quand la porte s'ouvre",
        "triggers": [{"type": "state", "entity_id": "binary_sensor.demo_porte", "to": "on"}],
        "actions": [{"type": "turn_on_for", "entity_id": "light.demo_salon", "minutes": 5}]
    }))
    .unwrap();
    let a = core.automations.upsert("", input).await.unwrap();
    assert_eq!(a.id, "entree_quand_la_porte_s_ouvre");

    // La porte s'ouvre.
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("on"),
            Default::default(),
            true,
        )
        .await;
    wait_for(
        &core,
        "light.demo_salon",
        "allumée par l'automatisme",
        |e| value(e) == "on",
    )
    .await;
    wait_for(
        &core,
        "timer.auto_light_demo_salon",
        "minuterie de 5 min",
        |e| value(e) == "running",
    )
    .await;
    for _ in 0..100 {
        if core.automations.list().await[0].last_run.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let ran = &core.automations.list().await[0];
    assert!(
        ran.last_run.is_some() && ran.last_error.is_none(),
        "{ran:?}"
    );

    // Une condition fausse bloque le déclenchement.
    let input: heim::automations::AutomationInput = serde_json::from_value(json!({
        "name": "Jamais",
        "triggers": [{"type": "state", "entity_id": "binary_sensor.demo_porte", "to": "off"}],
        "conditions": [{"type": "state", "entity_id": "light.demo_chambre", "is": "on"}],
        "actions": [{"type": "command", "entity_id": "light.demo_lampadaire", "action": "turn_on"}]
    }))
    .unwrap();
    core.automations.upsert("", input).await.unwrap();
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("off"),
            Default::default(),
            true,
        )
        .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        value(&core.registry.get("light.demo_lampadaire").await.unwrap()),
        "off"
    );
}

#[tokio::test]
async fn rooms_settings_and_backup() {
    let dir = temp_dir("settings");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;

    // Renommer une pièce touche toutes ses entités.
    let n = core
        .registry
        .rename_room("Salon", Some("Séjour".into()))
        .await;
    assert!(n >= 3, "{n}");
    assert_eq!(
        core.registry
            .get("light.demo_salon")
            .await
            .unwrap()
            .meta
            .room
            .as_deref(),
        Some("Séjour")
    );

    // Zigbee réglé depuis l'interface, appliqué à chaud (broker absent : « déconnecté », pas de plantage).
    let mut events = core.bus.subscribe();
    core.settings
        .set(
            heim::settings::ZIGBEE2MQTT,
            &heim::settings::Zigbee2MqttSettings {
                enabled: true,
                host: "127.0.0.1".into(),
                port: 1,
                username: None,
                password: None,
                base_topic: "zigbee2mqtt".into(),
            },
        )
        .unwrap();
    core.apply_zigbee_settings();
    let mut seen_zigbee = false;
    for _ in 0..50 {
        if let Ok(Ok(heim::bus::Event::IntegrationStatus { name, .. })) =
            tokio::time::timeout(Duration::from_millis(100), events.recv()).await
            && name == "zigbee2mqtt"
        {
            seen_zigbee = true;
            break;
        }
    }
    assert!(seen_zigbee);

    // Sauvegarde à chaud : un vrai fichier SQLite.
    let path = dir.join("copie.db");
    let store = core.store.clone();
    let p = path.clone();
    tokio::task::spawn_blocking(move || store.backup_blocking(&p))
        .await
        .unwrap()
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(heim::store::looks_like_sqlite(&bytes));
}

#[tokio::test]
async fn demo_devices_vanish_without_demo_mode() {
    let dir = temp_dir("purge");
    {
        let core = Core::start(&config(&dir, true)).unwrap();
        ready(&core).await;
        let store = core.store.clone();
        tokio::task::spawn_blocking(move || store.flush_blocking())
            .await
            .unwrap();
    }
    let core = Core::start(&config(&dir, false)).unwrap();
    for _ in 0..100 {
        if core.registry.get("light.demo_salon").await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let left: Vec<String> = core
        .registry
        .snapshot()
        .await
        .into_iter()
        .filter(|e| e.meta.integration == "virtual")
        .map(|e| e.meta.entity_id)
        .collect();
    assert!(left.is_empty(), "restes de démo : {left:?}");
}

#[tokio::test]
async fn mailbox_state_rearm_trace_and_journal() {
    use heim::automations::OpKind;
    let dir = temp_dir("mailbox");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;
    wait_for(&core, "binary_sensor.demo_porte", "porte présente", |e| {
        e.state.is_some()
    })
    .await;

    // « Courrier » : la trappe avant (porte) lève l'état, la trappe arrière (mouvement) réarme.
    let input: heim::automations::AutomationInput = serde_json::from_value(json!({
        "name": "Courrier",
        "triggers": [{"type": "state", "entity_id": "binary_sensor.demo_porte", "to": "on"}],
        "actions": [{"type": "command", "entity_id": "light.demo_salon", "action": "turn_on"}],
        "rearm": {"triggers": [{"type": "state", "entity_id": "binary_sensor.demo_mouvement", "to": "on"}]}
    }))
    .unwrap();
    let a = core.automations.upsert("", input).await.unwrap();
    assert!(!a.state && !a.fired());
    wait_for(&core, "automation.courrier", "entité au repos", |e| {
        value(e) == "off"
    })
    .await;

    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("on"),
            Default::default(),
            true,
        )
        .await;
    let e = wait_for(&core, "automation.courrier", "courrier arrivé", |e| {
        value(e) == "on"
    })
    .await;
    assert_eq!(e.state.unwrap().attributes["fired"], json!(true));

    // La trappe arrière s'ouvre : réarmé tout seul.
    core.registry
        .report(
            "binary_sensor.demo_mouvement",
            json!("on"),
            Default::default(),
            true,
        )
        .await;
    wait_for(&core, "automation.courrier", "réarmé", |e| {
        value(e) == "off"
    })
    .await;

    // Réarmement à la main (appui long) après un nouveau courrier.
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("off"),
            Default::default(),
            true,
        )
        .await;
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("on"),
            Default::default(),
            true,
        )
        .await;
    wait_for(&core, "automation.courrier", "re-courrier", |e| {
        value(e) == "on"
    })
    .await;
    let a = core
        .automations
        .op("courrier", OpKind::State(None), "user:Alex")
        .await
        .unwrap();
    assert!(!a.state);

    // Désactivé : la porte ne lève plus l'état.
    core.automations
        .op("courrier", OpKind::Enabled(false), "test")
        .await
        .unwrap();
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("off"),
            Default::default(),
            true,
        )
        .await;
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("on"),
            Default::default(),
            true,
        )
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        value(&core.registry.get("automation.courrier").await.unwrap()),
        "off"
    );

    // Une condition bloque : on sait pourquoi.
    let input: heim::automations::AutomationInput = serde_json::from_value(json!({
        "name": "Bloqué",
        "triggers": [{"type": "state", "entity_id": "binary_sensor.demo_porte", "to": "off"}],
        "conditions": [{"type": "state", "entity_id": "light.demo_chambre", "is": "on"}],
        "actions": [{"type": "command", "entity_id": "light.demo_lampadaire", "action": "turn_on"}]
    }))
    .unwrap();
    core.automations.upsert("", input).await.unwrap();
    core.registry
        .report(
            "binary_sensor.demo_porte",
            json!("off"),
            Default::default(),
            true,
        )
        .await;
    let mut reason = None;
    for _ in 0..100 {
        let list = core.automations.list().await;
        if let Some(c) = list
            .iter()
            .find(|a| a.id == "bloque")
            .and_then(|a| a.last_check.clone())
        {
            reason = c.reason;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let reason = reason.expect("trace de la condition");
    assert!(reason.contains("éteint"), "{reason}");

    // Un groupe allume ses deux lampes.
    core.settings
        .set(
            heim::groups::SETTINGS_KEY,
            &json!([{"id": "salon", "name": "Salon", "members": ["light.demo_salon", "light.demo_lampadaire"]}]),
        )
        .unwrap();
    wait_for(&core, "group.salon", "groupe déclaré", |e| {
        e.state.is_some()
    })
    .await;
    core.registry
        .command("group.salon", Action::TurnOff, "test")
        .await
        .unwrap();
    wait_for(
        &core,
        "light.demo_lampadaire",
        "éteinte par le groupe",
        |e| value(e) == "off",
    )
    .await;
    core.registry
        .command(
            "group.salon",
            Action::TurnOn {
                brightness: Some(100),
                color_temp: None,
            },
            "user:Alex",
        )
        .await
        .unwrap();
    wait_for(
        &core,
        "light.demo_lampadaire",
        "allumée par le groupe",
        |e| value(e) == "on",
    )
    .await;
    wait_for(&core, "group.salon", "groupe allumé", |e| value(e) == "on").await;

    // Le journal a tout vu.
    let log = core.history.query(&heim::history::LogQuery::default());
    assert!(
        log.iter().any(|l| l
            .message
            .as_deref()
            .is_some_and(|m| m.starts_with("pas lancé"))),
        "{log:#?}"
    );
    assert!(
        log.iter()
            .any(|l| l.entity_id.as_deref() == Some("automation.courrier"))
    );
}

#[tokio::test]
async fn battery_changed_ignores_the_stale_level() {
    use heim::registry::{STALE_SINCE, STALE_VALUE};
    let dir = temp_dir("battery");
    let core = Core::start(&config(&dir, true)).unwrap();
    ready(&core).await;
    let id = "sensor.demo_porte_batterie";
    wait_for(&core, id, "pile présente", |e| e.state.is_some()).await;
    core.registry
        .report(id, json!(3), Default::default(), true)
        .await;
    wait_for(&core, id, "3 %", |e| {
        e.state.as_ref().is_some_and(|s| s.value == json!(3))
    })
    .await;

    // « Pile changée » : niveau effacé, l'ancien est mis de côté.
    let mut attrs = serde_json::Map::new();
    attrs.insert(STALE_VALUE.into(), json!(3));
    attrs.insert(STALE_SINCE.into(), json!("2026-10-25T12:00:00Z"));
    core.registry
        .report(id, serde_json::Value::Null, attrs, true)
        .await;
    wait_for(&core, id, "effacé", |e| {
        e.state.as_ref().is_some_and(|s| s.value.is_null())
    })
    .await;

    // Zigbee2MQTT renvoie l'ancien niveau de mémoire : ignoré.
    core.registry
        .report(id, json!(3), Default::default(), true)
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let e = core.registry.get(id).await.unwrap();
    assert!(e.state.as_ref().unwrap().value.is_null(), "{e:?}");

    // Le vrai nouveau niveau arrive.
    core.registry
        .report(id, json!(100), Default::default(), true)
        .await;
    let e = wait_for(&core, id, "100 %", |e| {
        e.state.as_ref().is_some_and(|s| s.value == json!(100))
    })
    .await;
    assert!(!e.state.unwrap().attributes.contains_key(STALE_VALUE));
}
