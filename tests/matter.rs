//! Matter de bout en bout contre un faux serveur Matter (tests/fake_matter.py).
//! Sauté si Python ou la bibliothèque websockets manquent.

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

async fn wait(core: &Core, id: &str, check: impl Fn(&heim::model::State) -> bool) {
    for _ in 0..500 {
        if core
            .registry
            .get(id)
            .await
            .and_then(|e| e.state)
            .is_some_and(|s| check(&s))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{id} : condition non atteinte ({:?})",
        core.registry.get(id).await.and_then(|e| e.state)
    );
}

#[tokio::test]
async fn matter_server_bridge() {
    let ok = Command::new("python3")
        .args(["-c", "import websockets"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("websockets absent : test Matter sauté");
        return;
    }
    let port = 25580 + (std::process::id() % 1000) as u16;
    let mut child = Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_matter.py"))
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::{BufRead, BufReader};
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _fake = Fake(child);

    let dir = std::env::temp_dir().join(format!("heim-matter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let core = Core::start(&Config {
        data_dir: dir,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo: false,
        zigbee2mqtt: None,
        broker: false,
    })
    .unwrap();
    let url = format!("127.0.0.1:{port}");
    core.settings
        .set(
            heim::integrations::matter::SETTINGS_KEY,
            &json!({"enabled": true, "url": url}),
        )
        .unwrap();
    core.apply_matter_settings();

    let id = "light.matter_4_1";
    wait(&core, id, |s| s.value == json!("off")).await;
    let e = core.registry.get(id).await.unwrap();
    assert_eq!(e.meta.name, "KAJPLATS E27");
    core.registry
        .command(
            id,
            Action::TurnOn {
                brightness: Some(200),
                color_temp: None,
            },
            "test",
        )
        .await
        .unwrap();
    wait(&core, id, |s| {
        s.value == json!("on") && s.attributes.get("brightness") == Some(&json!(200))
    })
    .await;
    core.registry
        .command(id, Action::TurnOff, "test")
        .await
        .unwrap();
    wait(&core, id, |s| s.value == json!("off")).await;

    // Appairage par code : bon code, mauvais code.
    let full = heim::integrations::matter::normalize_url(&url);
    let node = heim::integrations::matter::request(
        &full,
        "commission_with_code",
        json!({"code": "MT:Y.K90AFN00KA0648G00", "network_only": true}),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(node["node_id"], json!(4));
    let err = heim::integrations::matter::request(
        &full,
        "commission_with_code",
        json!({"code": "123"}),
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert!(err.contains("Commissioning failed"), "{err}");
}
