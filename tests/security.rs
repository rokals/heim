//! Alarme de bout en bout avec les appareils virtuels.

use std::time::Duration;

use heim::model::EntityView;
use heim::security::{ArmError, Mode, Phase, SecurityConfig};
use heim::{Config, Core};
use serde_json::json;

fn config(dir: &std::path::Path) -> Config {
    Config {
        data_dir: dir.to_path_buf(),
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo: true,
        zigbee2mqtt: None,
        broker: false,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("heim-sec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

const PORTE: &str = "binary_sensor.demo_porte";
const MOUVEMENT: &str = "binary_sensor.demo_mouvement";
const FUITE: &str = "binary_sensor.ts0601_x7k3_occupancy";
const SIRENE: &str = "siren.demo_sirene";

fn value(e: &EntityView) -> String {
    e.state
        .as_ref()
        .and_then(|s| s.value.as_str())
        .unwrap_or_default()
        .to_string()
}

async fn wait_value(core: &Core, id: &str, want: &str) {
    for _ in 0..300 {
        if let Some(e) = core.registry.get(id).await
            && value(&e) == want
            && e.state.as_ref().is_some_and(|s| s.available)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{id} n'est pas passé à {want}");
}

async fn wait_phase(core: &Core, want: Phase) -> heim::security::Status {
    for _ in 0..400 {
        let s = core.security.status().await;
        if s.phase == want {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "phase {want:?} non atteinte : {:?}",
        core.security.status().await
    );
}

async fn set(core: &Core, id: &str, v: &str) {
    core.registry
        .report(id, json!(v), Default::default(), true)
        .await;
    wait_value(core, id, v).await;
}

async fn setup(core: &Core, exit_delay_s: u32, entry_delay_s: u32) {
    for id in [PORTE, MOUVEMENT, FUITE] {
        wait_value(core, id, "off").await;
    }
    wait_value(core, SIRENE, "off").await;
    let cfg: SecurityConfig = serde_json::from_value(json!({
        "sensors": [
            {"entity_id": PORTE, "role": "perimeter", "entry": true},
            {"entity_id": MOUVEMENT, "role": "interior"},
            {"entity_id": FUITE, "role": "h24"}
        ],
        "sirens": [SIRENE],
        "lights": ["light.demo_salon"],
        "exit_delay_s": exit_delay_s,
        "entry_delay_s": entry_delay_s,
        "siren_duration_s": 60
    }))
    .unwrap();
    core.settings.set(heim::security::CONFIG_KEY, &cfg).unwrap();
    // Le moteur relit ses réglages via le bus.
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn away_mode_full_cycle_with_code() {
    let dir = temp_dir("away");
    let core = Core::start(&config(&dir)).unwrap();
    setup(&core, 1, 0).await;

    // Porte ouverte : on prévient avant d'activer.
    set(&core, PORTE, "on").await;
    match core.security.arm(Mode::Away, false, None).await {
        Err(ArmError::NotReady { open, unavailable }) => {
            assert_eq!(open[0].entity_id, PORTE);
            assert!(unavailable.is_empty());
        }
        other => panic!("{other:?}"),
    }
    // Forcer : la porte est ignorée pour cette fois.
    let s = core.security.arm(Mode::Away, true, None).await.unwrap();
    assert_eq!(s.phase, Phase::Arming);
    assert_eq!(s.bypassed, vec![PORTE.to_string()]);
    core.security.disarm(None).await.unwrap();
    set(&core, PORTE, "off").await;

    // Code de désactivation.
    let mut cfg = core.security.config();
    cfg.code = Some(heim::security::new_code("2468").unwrap());
    core.settings.set(heim::security::CONFIG_KEY, &cfg).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let s = core.security.arm(Mode::Away, false, None).await.unwrap();
    assert_eq!(s.phase, Phase::Arming);
    // Pendant le délai de sortie on peut bouger.
    set(&core, MOUVEMENT, "on").await;
    set(&core, MOUVEMENT, "off").await;
    assert_eq!(core.security.status().await.phase, Phase::Arming);
    wait_phase(&core, Phase::Armed).await;

    // Mouvement intérieur, pas de délai d'entrée : ça sonne tout de suite.
    set(&core, MOUVEMENT, "on").await;
    let s = wait_phase(&core, Phase::Triggered).await;
    assert_eq!(s.cause.unwrap().entity_id, MOUVEMENT);
    wait_value(&core, SIRENE, "on").await;
    wait_value(&core, "light.demo_salon", "on").await;

    assert!(core.security.disarm(None).await.is_err());
    assert!(core.security.disarm(Some("1111".into())).await.is_err());
    let s = core.security.disarm(Some("2468".into())).await.unwrap();
    assert_eq!(s.phase, Phase::Disarmed);
    wait_value(&core, SIRENE, "off").await;

    let log = core.security.log();
    let kinds: Vec<&str> = log.iter().map(|l| l.kind.as_str()).collect();
    assert!(kinds.contains(&"triggered"), "{kinds:?}");
    assert!(kinds.contains(&"wrong_code"), "{kinds:?}");
    assert_eq!(kinds[0], "disarmed");
}

#[tokio::test]
async fn night_mode_ignores_interior_and_waits_at_the_door() {
    let dir = temp_dir("night");
    let core = Core::start(&config(&dir)).unwrap();
    setup(&core, 60, 1).await;

    // En Nuit, pas de délai de sortie par défaut.
    let s = core.security.arm(Mode::Night, false, None).await.unwrap();
    assert_eq!(s.phase, Phase::Armed);
    set(&core, MOUVEMENT, "on").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(core.security.status().await.phase, Phase::Armed);

    // La porte d'entrée laisse le délai d'entrée.
    set(&core, PORTE, "on").await;
    let s = wait_phase(&core, Phase::Pending).await;
    assert!(s.until.is_some());
    assert_eq!(
        core.registry
            .get(SIRENE)
            .await
            .map(|e| value(&e))
            .as_deref(),
        Some("off")
    );
    // Personne ne désactive : ça sonne.
    wait_phase(&core, Phase::Triggered).await;
    wait_value(&core, SIRENE, "on").await;
    core.security.disarm(None).await.unwrap();
    wait_value(&core, SIRENE, "off").await;
}

#[tokio::test]
async fn h24_sensor_rings_even_when_disarmed() {
    let dir = temp_dir("h24");
    let core = Core::start(&config(&dir)).unwrap();
    setup(&core, 0, 0).await;
    set(&core, FUITE, "on").await;
    wait_value(&core, SIRENE, "on").await;
    let s = core.security.status().await;
    assert_eq!(s.phase, Phase::Disarmed);
    assert!(s.sirens_until.is_some());
    assert!(s.cause.unwrap().h24);
    // « Arrêter » coupe les sirènes même si l'alarme n'était pas activée.
    core.security.disarm(None).await.unwrap();
    wait_value(&core, SIRENE, "off").await;
}

#[tokio::test]
async fn armed_state_survives_a_restart() {
    let dir = temp_dir("restart");
    {
        let core = Core::start(&config(&dir)).unwrap();
        setup(&core, 0, 0).await;
        core.security.arm(Mode::Away, false, None).await.unwrap();
        let store = core.store.clone();
        tokio::task::spawn_blocking(move || store.flush_blocking())
            .await
            .unwrap();
    }
    let core = Core::start(&config(&dir)).unwrap();
    let s = core.security.status().await;
    assert_eq!(s.phase, Phase::Armed);
    assert_eq!(s.mode, Some(Mode::Away));
}
