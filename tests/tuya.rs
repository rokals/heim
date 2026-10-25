//! Tuya de bout en bout contre de faux appareils qui utilisent la bibliothèque tinytuya
//! (tests/fake_tuya.py). Sauté si Python ou tinytuya manquent.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use heim::model::Action;
use heim::{Config, Core};
use serde_json::json;

struct Fake(Child);
impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn fake(ip: &str, version: &str, id: &str, key: &str, announce: bool) -> Option<Fake> {
    let mut cmd = Command::new("python3");
    cmd.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_tuya.py"))
        .args([ip, version, id, key])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if announce {
        cmd.arg("annonce");
    }
    let mut child = cmd.spawn().ok()?;
    use std::io::{BufRead, BufReader};
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    line.contains("prêt").then_some(Fake(child))
}

fn have_tinytuya() -> bool {
    Command::new("python3")
        .args(["-c", "import tinytuya"])
        .status()
        .is_ok_and(|s| s.success())
}

async fn wait_value(core: &Core, id: &str, want: serde_json::Value) {
    for _ in 0..800 {
        if core
            .registry
            .get(id)
            .await
            .and_then(|e| e.state)
            .is_some_and(|s| s.available && s.value == want)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{id} n'a pas atteint {want} : {:?}",
        core.registry.get(id).await.and_then(|e| e.state)
    );
}

#[tokio::test]
async fn tuya_33_34_35_against_tinytuya() {
    if !have_tinytuya() {
        eprintln!("tinytuya absent : test Tuya sauté (pip install tinytuya)");
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heim=debug")
        .with_test_writer()
        .try_init();
    let key = "0123456789abcdef";
    let _a = fake("127.0.0.2", "3.3", "bf00000000000000000033", key, false).expect("faux 3.3");
    let _b = fake("127.0.0.3", "3.4", "bf00000000000000000034", key, false).expect("faux 3.4");
    // Celui-ci n'a pas d'adresse dans les réglages : Heim le trouve par son annonce réseau.
    let _c = fake("127.0.0.4", "3.5", "bf00000000000000000035", key, true).expect("faux 3.5");

    let dir = std::env::temp_dir().join(format!("heim-tuya-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let core = Core::start(&Config {
        data_dir: dir,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo: false,
        zigbee2mqtt: None,
    })
    .unwrap();
    let mapping = json!({"1": {"code": "switch_1", "type": "Boolean"}, "19": {"code": "cur_power", "values": {"scale": 1}}});
    let settings: heim::integrations::tuya::TuyaSettings = serde_json::from_value(json!({
        "enabled": true,
        "devices": [
            {"id": "bf00000000000000000033", "name": "Prise 33", "key": key, "ip": "127.0.0.2", "version": "3.3", "category": "cz", "mapping": mapping},
            {"id": "bf00000000000000000034", "name": "Prise 34", "key": key, "ip": "127.0.0.3", "version": "3.4", "category": "cz", "mapping": mapping},
            {"id": "bf00000000000000000035", "name": "Prise 35", "key": key, "category": "cz", "mapping": mapping}
        ]
    }))
    .unwrap();
    core.settings
        .set(heim::integrations::tuya::SETTINGS_KEY, &settings)
        .unwrap();
    core.apply_tuya_settings();

    for v in ["33", "34", "35"] {
        let sw = format!("switch.prise_{v}");
        wait_value(&core, &sw, json!("off")).await;
        core.registry
            .command(
                &sw,
                Action::TurnOn {
                    brightness: None,
                    color_temp: None,
                },
                "test",
            )
            .await
            .unwrap();
        // L'appareil confirme lui-même, et la puissance suit.
        wait_value(&core, &sw, json!("on")).await;
        wait_value(&core, &format!("sensor.prise_{v}_power"), json!(85.3)).await;
        core.registry
            .command(&sw, Action::TurnOff, "test")
            .await
            .unwrap();
        wait_value(&core, &sw, json!("off")).await;
    }

    // Réglages génériques : la même prise, par ses points de données.
    let id = "tuya:bf00000000000000000034";
    let controls = heim::devices::lock(&core.controls)
        .get(id)
        .cloned()
        .expect("contrôles");
    assert_eq!(controls.controls[0]["name"], json!("switch_1"));
    let mut values = serde_json::Map::new();
    values.insert("1".into(), json!(true));
    core.bus.publish(heim::bus::Event::DeviceSet {
        integration: "tuya".into(),
        device_id: id.into(),
        values,
    });
    wait_value(&core, "switch.prise_34", json!("on")).await;
    let seen = heim::devices::lock(&core.controls)
        .get(id)
        .unwrap()
        .values
        .get("19")
        .cloned();
    assert_eq!(seen, Some(json!(85.3)), "puissance mise à l'échelle");
}
