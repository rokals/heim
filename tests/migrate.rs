//! Reprise d'une installation Home Assistant : par l'API WebSocket de HA (faux HA en Python)
//! et directement par MQTT (faux Zigbee2MQTT sur le broker de Heim).

use std::time::Duration;

use heim::broker::{Broker, BrokerSettings};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn has_python(module: &str) -> bool {
    std::process::Command::new("python3")
        .args(["-c", &format!("import {module}")])
        .status()
        .is_ok_and(|s| s.success())
}

#[tokio::test]
async fn home_assistant_websocket() {
    if !has_python("websockets") {
        eprintln!("python3 + websockets absents : test sauté");
        return;
    }
    let port = free_port();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_ha.py");
    let mut fake = tokio::process::Command::new("python3")
        .arg(&script)
        .arg(port.to_string())
        .arg("bon-jeton")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}/");
    let mut ha = None;
    for _ in 0..50 {
        match heim::migrate::Ha::connect(&url, "bon-jeton").await {
            Ok(h) => {
                ha = Some(h);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut ha = ha.expect("connexion au faux HA");
    assert_eq!(ha.version.as_deref(), Some("2026.10.1"));
    let err = heim::migrate::Ha::connect(&url, "mauvais")
        .await
        .err()
        .unwrap();
    assert!(err.contains("jeton refusé"), "{err}");

    let zip = ha.z2m_backup("zigbee2mqtt").await.expect("sauvegarde");
    let files = heim::migrate::unzip(&zip).unwrap();
    assert!(files.iter().any(|(n, _)| n == "database.db"));

    let (rooms, by_ieee) = ha.rooms().await.unwrap();
    assert_eq!(rooms, vec!["Cuisine".to_string(), "Salon".to_string()]);
    assert_eq!(by_ieee.get("0x0a").map(String::as_str), Some("Salon"));
    assert_eq!(by_ieee.len(), 1);

    let addon = ha.z2m_addon().await.expect("module");
    assert_eq!(addon.0, "45df7312_zigbee2mqtt");
    ha.stop_addon(&addon.0).await.unwrap();
    assert!(ha.stop_addon("../x").await.is_err());
    let _ = fake.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backup_over_mqtt_is_imported() {
    if !has_python("paho.mqtt.client") {
        eprintln!("python3 + paho-mqtt absents : test sauté");
        return;
    }
    let port = free_port();
    let b = Broker::new(None);
    b.apply(BrokerSettings {
        enabled: true,
        port,
        ..Default::default()
    })
    .await
    .unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_z2m.py");
    let mut fake = tokio::process::Command::new("python3")
        .arg(&script)
        .arg(port.to_string())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let env = heim::plugins::MqttEnv {
        host: "127.0.0.1".into(),
        port,
        username: None,
        password: None,
    };
    let zip = heim::migrate::z2m_backup_mqtt(&env, "zigbee2mqtt")
        .await
        .expect("sauvegarde par MQTT");
    let _ = fake.kill().await;
    let files = heim::migrate::unzip(&zip).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(!names.iter().any(|n| n.contains("log")), "{names:?}");

    let dir = std::env::temp_dir().join(format!("heim-test-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let z = heim::zigbee_local::ZigbeeLocal::with(
        dir.clone(),
        heim::zigbee_local::Runner::Child("true".into()),
        None,
    );
    let get = |n: &str| files.iter().find(|(f, _)| f == n).map(|(_, d)| d.clone());
    let conf = String::from_utf8(get("configuration.yaml").unwrap()).unwrap();
    let summary = z
        .import(
            &conf,
            get("database.db").as_deref(),
            get("coordinator_backup.json")
                .map(|b| String::from_utf8(b).unwrap())
                .as_deref(),
        )
        .unwrap();
    assert!(summary["devices"].as_u64().unwrap() >= 4, "{summary}");
    assert_eq!(z.import_extra(&files).unwrap(), 1);
    assert!(dir.join("secret.yaml").exists());
    // La configuration réécrite pour la box : broker de Heim, clé d'ici.
    let s = heim::zigbee_local::LocalSettings {
        port: Some("/dev/ttyUSB9".into()),
        ..Default::default()
    };
    z.write_config(&s, port).unwrap();
    let text = std::fs::read_to_string(dir.join("configuration.yaml")).unwrap();
    assert!(text.contains(&format!("127.0.0.1:{port}")), "{text}");
    assert!(text.contains("/dev/ttyUSB9"), "{text}");
    assert!(text.contains("network_key"), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}
