//! Le broker MQTT intégré, avec de vrais clients MQTT (rumqttc) et Heim branché dessus.

use std::time::Duration;

use heim::broker::{Broker, BrokerSettings};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn broker(port: u16) -> Broker {
    let b = Broker::new(None);
    b.apply(BrokerSettings {
        enabled: true,
        port,
        ..Default::default()
    })
    .await
    .unwrap_or_else(|e| panic!("broker sur {port} : {e}"));
    b
}

fn client(id: &str, port: u16, will: Option<LastWill>) -> (AsyncClient, EventLoop) {
    let mut o = MqttOptions::new(id, "127.0.0.1", port);
    o.set_keep_alive(Duration::from_secs(5));
    o.set_max_packet_size(4 * 1024 * 1024, 4 * 1024 * 1024);
    if let Some(w) = will {
        o.set_last_will(w);
    }
    AsyncClient::new(o, 64)
}

/// Lit les événements jusqu'à un PUBLISH (ou l'échéance).
async fn next_publish(ev: &mut EventLoop, secs: u64) -> Option<rumqttc::Publish> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        match tokio::time::timeout_at(deadline, ev.poll()).await {
            Ok(Ok(Event::Incoming(Packet::Publish(p)))) => return Some(p),
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => return None,
        }
    }
}

async fn until_suback(ev: &mut EventLoop) {
    loop {
        if let Ok(Event::Incoming(Packet::SubAck(_))) = ev.poll().await {
            return;
        }
    }
}

#[tokio::test]
async fn publish_subscribe_retain_and_will() {
    let port = free_port();
    let b = broker(port).await;

    let (a, mut ea) = client("a", port, None);
    a.subscribe("maison/#", QoS::AtLeastOnce).await.unwrap();
    until_suback(&mut ea).await;

    // Un message retenu, un message ordinaire.
    let (pubr, mut ep) = client("p", port, None);
    pubr.publish("maison/salon/temp", QoS::AtLeastOnce, true, "21.5")
        .await
        .unwrap();
    pubr.publish("maison/salon/lampe", QoS::AtMostOnce, false, "ON")
        .await
        .unwrap();
    pubr.publish("ailleurs/x", QoS::AtMostOnce, false, "non")
        .await
        .unwrap();
    tokio::spawn(async move {
        loop {
            if ep.poll().await.is_err() {
                break;
            }
        }
    });

    let p1 = next_publish(&mut ea, 5).await.expect("premier message");
    assert_eq!(p1.topic, "maison/salon/temp");
    assert_eq!(&p1.payload[..], b"21.5");
    assert!(!p1.retain, "en direct, pas de drapeau « retenu »");
    let p2 = next_publish(&mut ea, 5).await.expect("second message");
    assert_eq!(p2.topic, "maison/salon/lampe");

    // Un abonné arrivé après reçoit le retenu, avec le drapeau.
    let (late, mut el) = client("late", port, None);
    late.subscribe("maison/+/temp", QoS::AtMostOnce)
        .await
        .unwrap();
    let r = next_publish(&mut el, 5).await.expect("message retenu");
    assert_eq!(r.topic, "maison/salon/temp");
    assert!(r.retain);
    assert_eq!(
        b.retained("maison/salon/temp").as_deref(),
        Some(&b"21.5"[..])
    );

    // Dernières volontés : le client disparaît sans DISCONNECT.
    let (w, mut ew) = client(
        "zigbee2mqtt",
        port,
        Some(LastWill::new(
            "maison/bridge/state",
            "offline",
            QoS::AtLeastOnce,
            true,
        )),
    );
    w.publish("maison/bridge/state", QoS::AtLeastOnce, true, "online")
        .await
        .unwrap();
    let mut seen_online = false;
    for _ in 0..20 {
        if let Ok(Event::Outgoing(rumqttc::Outgoing::Publish(_))) = ew.poll().await {
            seen_online = true;
            break;
        }
    }
    assert!(seen_online);
    let online = next_publish(&mut ea, 5).await.unwrap();
    assert_eq!(&online.payload[..], b"online");
    drop(w);
    drop(ew); // la connexion TCP se ferme brutalement
    let offline = next_publish(&mut ea, 5).await.expect("dernières volontés");
    assert_eq!(offline.topic, "maison/bridge/state");
    assert_eq!(&offline.payload[..], b"offline");
    assert_eq!(
        b.retained("maison/bridge/state").as_deref(),
        Some(&b"offline"[..])
    );

    // Effacer un retenu : message vide.
    pubr.publish("maison/salon/temp", QoS::AtMostOnce, true, "")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(b.retained("maison/salon/temp").is_none());

    let status = b.status();
    assert!(status["running"].as_bool().unwrap());
    assert!(status["clients"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn same_client_id_takes_over() {
    let port = free_port();
    let b = broker(port).await;
    let (_c1, mut e1) = client("same", port, None);
    // Connexion 1 établie.
    loop {
        if let Ok(Event::Incoming(Packet::ConnAck(_))) = e1.poll().await {
            break;
        }
    }
    let (_c2, mut e2) = client("same", port, None);
    loop {
        if let Ok(Event::Incoming(Packet::ConnAck(_))) = e2.poll().await {
            break;
        }
    }
    // La première est coupée.
    let mut cut = false;
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(2), e1.poll()).await {
            Ok(Err(_)) => {
                cut = true;
                break;
            }
            Ok(Ok(_)) => {}
            Err(_) => break,
        }
    }
    assert!(cut, "l'ancienne connexion doit être fermée");
    assert_eq!(b.client_ids(), vec!["same".to_string()]);
}

#[tokio::test]
async fn opening_to_the_network_rebinds_at_once() {
    let port = free_port();
    let b = broker(port).await;
    b.apply(BrokerSettings {
        enabled: true,
        port,
        open: true,
        ..Default::default()
    })
    .await
    .expect("même port, écoute élargie : pas de « déjà pris »");
    assert!(b.running());
    let (_c, mut e) = client("after", port, None);
    loop {
        if let Ok(Event::Incoming(Packet::ConnAck(_))) = e.poll().await {
            break;
        }
    }
}

#[tokio::test]
async fn disabled_broker_does_not_listen() {
    let port = free_port();
    let b = broker(port).await;
    b.apply(BrokerSettings {
        enabled: false,
        port,
        ..Default::default()
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!b.running());
    assert_eq!(b.status()["running"], serde_json::json!(false));
}

#[tokio::test]
async fn port_taken_is_reported() {
    let holder = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = holder.local_addr().unwrap().port();
    let b = Broker::new(None);
    let err = b
        .apply(BrokerSettings {
            enabled: true,
            port,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.contains("déjà pris"), "{err}");
    assert_eq!(b.status()["error"].as_str(), Some(err.as_str()));
}

/// Heim + son broker + un faux Zigbee2MQTT (Python, paho) : l'appareil apparaît et obéit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn heim_reads_zigbee2mqtt_through_its_own_broker() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_z2m.py");
    let python_ok = std::process::Command::new("python3")
        .args(["-c", "import paho.mqtt.client"])
        .status()
        .is_ok_and(|s| s.success());
    if !python_ok {
        eprintln!("python3 + paho-mqtt absents : test sauté");
        return;
    }
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("heim-test-broker-{}", std::process::id()));
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
    core.settings
        .set(
            heim::settings::ZIGBEE2MQTT,
            &heim::settings::Zigbee2MqttSettings {
                enabled: true,
                host: "127.0.0.1".into(),
                port,
                username: None,
                password: None,
                base_topic: "zigbee2mqtt".into(),
            },
        )
        .unwrap();
    core.apply_zigbee_settings();

    // Pièces reprises de Home Assistant : posées dès que les entités arrivent.
    heim::migrate::remember_rooms(
        &core,
        &["Salon".to_string()],
        [("0x01".to_string(), "salon".to_string())].into(),
    )
    .unwrap();

    let mut fake = tokio::process::Command::new("python3")
        .arg(&script)
        .arg(port.to_string())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    // La lampe arrive par bridge/devices.
    let mut found = None;
    for _ in 0..100 {
        if let Some(e) =
            core.registry.snapshot().await.into_iter().find(|e| {
                e.meta.integration == "zigbee2mqtt" && e.meta.entity_id.starts_with("light.")
            })
        {
            found = Some(e);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let lamp = found.expect("la lampe de Zigbee2MQTT doit apparaître");
    let mut room = None;
    for _ in 0..30 {
        room = core
            .registry
            .snapshot()
            .await
            .into_iter()
            .find(|e| e.meta.device_id.as_deref() == Some("0x01"))
            .and_then(|e| e.meta.room);
        if room.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        room.as_deref(),
        Some("Salon"),
        "pièce reprise de Home Assistant"
    );

    // Une commande part vers Zigbee2MQTT, qui répond par son nouvel état.
    core.registry
        .command(
            &lamp.meta.entity_id,
            heim::model::Action::TurnOn {
                brightness: None,
                color_temp: None,
            },
            "test",
        )
        .await
        .ok();
    let mut on = false;
    for _ in 0..50 {
        if let Some(e) = core.registry.get(&lamp.meta.entity_id).await
            && e.state
                .as_ref()
                .is_some_and(|s| s.value == serde_json::json!("on"))
        {
            on = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(on, "la lampe doit passer à « on »");

    // Appairage : Zigbee2MQTT est en ligne, il confirme l'ouverture du réseau.
    assert_eq!(core.pairing.lock().unwrap().online, Some(true));
    core.bus
        .publish(heim::bus::Event::ZigbeePermitJoin { seconds: 60 });
    let mut confirmed = false;
    for _ in 0..50 {
        let p = core.pairing.lock().unwrap().clone();
        if p.last_response
            .as_ref()
            .is_some_and(|r| r.0 == "permit_join" && r.1)
            && p.open
        {
            confirmed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = fake.kill().await;
    assert!(confirmed, "Zigbee2MQTT doit confirmer l'appairage");
    let _ = std::fs::remove_dir_all(dir);
}
