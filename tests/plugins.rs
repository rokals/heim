//! Plugins : découverte MQTT (un appareil Tasmota) et un plugin externe téléchargé, lancé,
//! qui annonce son capteur sur le broker de Heim.

use std::time::Duration;

use heim::broker::BrokerSettings;
use heim::plugins::{Installed, PluginsSettings};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn core_with_broker(name: &str) -> (heim::Core, u16, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("heim-test-plugins-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let core = heim::Core::start(&heim::Config {
        data_dir: dir.clone(),
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo: false,
        zigbee2mqtt: None,
        broker: false,
    })
    .unwrap();
    let port = free_port();
    core.settings
        .set(
            heim::broker::SETTINGS_KEY,
            &BrokerSettings {
                enabled: true,
                port,
                ..Default::default()
            },
        )
        .unwrap();
    core.apply_broker_settings().await.unwrap();
    (core, port, dir)
}

async fn wait_entity(core: &heim::Core, id: &str) -> Option<heim::model::EntityView> {
    for _ in 0..100 {
        if let Some(e) = core.registry.get(id).await
            && e.state.is_some()
        {
            return Some(e);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tasmota_device_through_discovery() {
    let (core, port, dir) = core_with_broker("tasmota").await;
    let mut s = core.plugin_settings();
    s.installed.insert(
        "mqtt_discovery".into(),
        Installed {
            id: "mqtt_discovery".into(),
            builtin: true,
            official: true,
            enabled: true,
            ..Default::default()
        },
    );
    core.settings.set(heim::plugins::SETTINGS_KEY, &s).unwrap();
    core.apply_discovery();

    // Une prise Tasmota s'annonce (messages retenus, comme le vrai).
    let mut o = MqttOptions::new("tasmota_ABC", "127.0.0.1", port);
    o.set_keep_alive(Duration::from_secs(5));
    let (c, mut ev) = AsyncClient::new(o, 32);
    c.subscribe("tasmota_ABC/cmnd/#", QoS::AtMostOnce)
        .await
        .unwrap();
    c.publish(
        "homeassistant/switch/ABC_RL_1/config",
        QoS::AtLeastOnce,
        true,
        r#"{"name":"Prise salon","~":"tasmota_ABC/","stat_t":"~stat/POWER","cmd_t":"~cmnd/POWER","avty_t":"~tele/LWT","pl_avail":"Online","pl_not_avail":"Offline","uniq_id":"ABC_RL_1","dev":{"ids":["ABC"],"name":"Prise salon","mdl":"Sonoff S26"}}"#,
    )
    .await
    .unwrap();
    c.publish("tasmota_ABC/tele/LWT", QoS::AtLeastOnce, true, "Online")
        .await
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c2 = c.clone();
    tokio::spawn(async move {
        let mut announced = false;
        loop {
            match ev.poll().await {
                Ok(Event::Incoming(Packet::SubAck(_))) if !announced => {
                    announced = true;
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    let _ = tx.send((
                        p.topic.clone(),
                        String::from_utf8_lossy(&p.payload).to_string(),
                    ));
                    // Elle obéit et renvoie son état.
                    if p.topic == "tasmota_ABC/cmnd/POWER" {
                        let _ = c2.try_publish(
                            "tasmota_ABC/stat/POWER",
                            QoS::AtMostOnce,
                            false,
                            p.payload.to_vec(),
                        );
                    }
                }
                Ok(_) => {}
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    });
    // L'entité apparaît, puis reçoit son état.
    let mut found = false;
    for _ in 0..100 {
        if core.registry.get("switch.mqtt_abc_rl_1").await.is_some() {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(found, "la prise Tasmota doit apparaître");
    c.publish("tasmota_ABC/stat/POWER", QoS::AtMostOnce, false, "OFF")
        .await
        .unwrap();
    let e = wait_entity(&core, "switch.mqtt_abc_rl_1").await.unwrap();
    assert_eq!(e.meta.name, "Prise salon");
    assert_eq!(
        e.meta.capabilities["model"],
        serde_json::json!("Sonoff S26")
    );

    // Une commande de Heim part sur cmnd/POWER.
    core.registry
        .command(
            "switch.mqtt_abc_rl_1",
            heim::model::Action::TurnOn {
                brightness: None,
                color_temp: None,
            },
            "test",
        )
        .await
        .ok();
    let mut got = None;
    for _ in 0..50 {
        if let Ok(Some((t, p))) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
            && t == "tasmota_ABC/cmnd/POWER"
        {
            got = Some(p);
            break;
        }
    }
    assert_eq!(got.as_deref(), Some("ON"));
    let mut on = false;
    for _ in 0..50 {
        if core
            .registry
            .get("switch.mqtt_abc_rl_1")
            .await
            .and_then(|e| e.state)
            .is_some_and(|s| s.value == serde_json::json!("on"))
        {
            on = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(on);
    let _ = std::fs::remove_dir_all(dir);
}

/// Un plugin externe : manifeste et programme servis en local, empreinte vérifiée, lancé par
/// Heim avec ses réglages, il annonce un capteur par la découverte MQTT.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_plugin_is_downloaded_checked_and_run() {
    use sha2::Digest;
    let python_ok = std::process::Command::new("python3")
        .args(["-c", "import paho.mqtt.client"])
        .status()
        .is_ok_and(|s| s.success());
    let Some(arch) = heim::plugins::arch_key() else {
        return;
    };
    if !python_ok || !cfg!(unix) {
        eprintln!("python3 + paho-mqtt absents : test sauté");
        return;
    }
    let (core, _port, dir) = core_with_broker("external").await;

    // Le « dépôt » : un serveur HTTP local.
    let www = dir.join("www");
    std::fs::create_dir_all(&www).unwrap();
    let script = r#"#!/usr/bin/env python3
import json, os, time
import paho.mqtt.client as mqtt
cfg = json.loads(os.environ["HEIM_PLUGIN_SETTINGS"])
c = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="plugin-demo")
c.connect(os.environ["HEIM_MQTT_HOST"], int(os.environ["HEIM_MQTT_PORT"]))
c.loop_start()
print("démarré pour", cfg["room"], flush=True)
c.publish("homeassistant/sensor/demo/temp/config", json.dumps({
  "name": "Température", "uniq_id": "demo_temp", "stat_t": "demo/temp", "dev_cla": "temperature",
  "unit_of_meas": "°C", "dev": {"ids": ["demo"], "name": "Capteur " + cfg["room"]}}), retain=True)
while True:
    c.publish("demo/temp", "21.5")
    time.sleep(0.5)
"#;
    std::fs::write(www.join("plugin"), script).unwrap();
    let sum: String = sha2::Sha256::digest(script.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let http_port = free_port();
    let manifest = serde_json::json!({
        "id": "demo", "name": "Démo", "version": "1.0.0", "category": "connections",
        "description": {"fr": "Un capteur pour essayer", "en": "A sensor to try"},
        "memory_mb": 15,
        "binaries": { arch: { "url": format!("http://127.0.0.1:{http_port}/plugin"), "sha256": sum } },
        "settings": [{"key": "room", "type": "string", "label": "Pièce", "required": true}]
    });
    std::fs::write(www.join("heim-plugin.json"), manifest.to_string()).unwrap();
    let mut server = tokio::process::Command::new("python3")
        .args([
            "-m",
            "http.server",
            &http_port.to_string(),
            "--bind",
            "127.0.0.1",
        ])
        .current_dir(&www)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;

    let url = format!("http://127.0.0.1:{http_port}/heim-plugin.json");
    let (m, src) = core.plugins.fetch_manifest(&url).await.expect("manifeste");
    assert_eq!(m.id, "demo");

    // Une empreinte fausse : rien ne s'installe.
    let mut bad = m.clone();
    for b in bad.binaries.values_mut() {
        b.sha256 = "0".repeat(64);
    }
    assert!(core.plugins.download(&bad).await.is_err());

    core.plugins.download(&m).await.expect("téléchargement");
    let settings = heim::plugins::clean_settings(
        &m.settings,
        serde_json::json!({"room": "Salon"}).as_object().unwrap(),
    )
    .unwrap();
    let entry = Installed {
        id: "demo".into(),
        source: Some(src),
        enabled: true,
        manifest: Some(m),
        settings,
        ..Default::default()
    };
    let mut s: PluginsSettings = core.plugin_settings();
    s.installed.insert(
        "mqtt_discovery".into(),
        Installed {
            id: "mqtt_discovery".into(),
            builtin: true,
            enabled: true,
            ..Default::default()
        },
    );
    s.installed.insert("demo".into(), entry.clone());
    core.settings.set(heim::plugins::SETTINGS_KEY, &s).unwrap();
    core.apply_discovery();
    core.apply_external(&entry);

    let e = wait_entity(&core, "sensor.mqtt_demo_temp")
        .await
        .expect("le capteur du plugin");
    assert_eq!(e.state.unwrap().value, serde_json::json!(21.5));
    assert_eq!(e.meta.name, "Capteur Salon Température");
    let st = core.plugins.state("demo").unwrap();
    assert!(st.running);
    assert!(core.plugins.log("demo").iter().any(|l| l.contains("Salon")));

    // Son processus est reconnu dans les performances.
    let snap = core.perf.sample();
    assert!(
        snap.processes.iter().any(|p| p.name == "plugin:demo"),
        "{:?}",
        snap.processes
    );

    core.plugins.stop("demo");
    let _ = server.kill().await;
    let _ = std::fs::remove_dir_all(dir);
}
