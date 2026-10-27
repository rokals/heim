//! Broker MQTT intégré.
//!
//! Zigbee2MQTT (et d'autres : Tasmota, Shelly, ESPHome…) parlent MQTT. Jusqu'ici il fallait un
//! broker à part (Mosquitto, celui de Home Assistant). Heim en embarque un petit, pour que la box
//! se suffise à elle-même : Zigbee2MQTT tourne sur le Pi et se branche sur Heim.
//!
//! MQTT 3.1.1, sessions propres (clean session), QoS 0 et 1 en sortie, QoS 0, 1 et 2 en entrée,
//! messages retenus (gardés sur la carte SD, écrits au plus toutes les 30 s), dernières volontés,
//! jokers `+` et `#`. Les connexions de la box elle-même (127.0.0.1) entrent sans mot de passe ;
//! celles du réseau, seulement si on l'a ouvert, et avec un compte.
//!
//! Le décodage des paquets est celui de `rumqttc` (déjà là pour le client) : rien de nouveau à
//! compiler.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use rumqttc::{
    ConnAck, ConnectReturnCode, LastWill, Packet, PubAck, PubComp, PubRec, Publish, QoS, SubAck,
    SubscribeReasonCode, UnsubAck,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

pub const SETTINGS_KEY: &str = "mqtt_broker";
pub const DEFAULT_PORT: u16 = 1883;
/// Un `bridge/devices` de Zigbee2MQTT avec beaucoup d'appareils dépasse vite le mégaoctet.
const MAX_PACKET: usize = 8 * 1024 * 1024;
/// File d'attente par client : au-delà, un client trop lent perd ses messages (QoS 0) ou la
/// connexion, plutôt que de faire grossir la mémoire du Pi.
const QUEUE: usize = 2048;
const MAX_CLIENTS: usize = 64;
const MAX_RETAINED: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Ouvert au réseau de la maison (sinon : la box seulement).
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub users: Vec<BrokerUser>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerUser {
    pub username: String,
    pub password: String,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for BrokerSettings {
    fn default() -> Self {
        BrokerSettings {
            enabled: false,
            port: DEFAULT_PORT,
            open: false,
            users: Vec::new(),
        }
    }
}

impl BrokerSettings {
    pub fn validate(&mut self) -> Result<(), String> {
        if self.port == 0 {
            return Err("port invalide".into());
        }
        let mut seen = HashSet::new();
        for u in &mut self.users {
            u.username = u.username.trim().to_string();
            if u.username.is_empty() || u.username.len() > 64 {
                return Err("chaque compte MQTT doit avoir un nom (64 caractères au plus)".into());
            }
            if u.password.len() < 8 {
                return Err("mot de passe MQTT trop court (8 caractères au moins)".into());
            }
            if !seen.insert(u.username.clone()) {
                return Err(format!(
                    "le compte MQTT « {} » existe deux fois",
                    u.username
                ));
            }
        }
        if self.users.len() > 20 {
            return Err("20 comptes MQTT au plus".into());
        }
        Ok(())
    }

    /// Ce qu'on montre à l'interface : les mots de passe ne repartent pas.
    pub fn public(&self) -> Value {
        json!({
            "enabled": self.enabled, "port": self.port, "open": self.open,
            "users": self.users.iter().map(|u| json!({ "username": u.username })).collect::<Vec<_>>(),
        })
    }
}

/// Un mot de passe lisible, pour un compte MQTT créé depuis l'interface.
pub fn generate_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 18];
    let _ = getrandom::getrandom(&mut raw);
    raw.iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

/// `+` : un niveau ; `#` : tout le reste (et le niveau parent). Les sujets `$…` ne sont jamais
/// pris par un joker de tête.
pub fn matches(filter: &str, topic: &str) -> bool {
    if topic.starts_with('$') && (filter.starts_with('+') || filter.starts_with('#')) {
        return false;
    }
    let mut f = filter.split('/');
    let mut t = topic.split('/');
    loop {
        match (f.next(), t.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(_)) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

pub fn valid_filter(filter: &str) -> bool {
    if filter.is_empty() || filter.len() > 1024 {
        return false;
    }
    let levels: Vec<&str> = filter.split('/').collect();
    for (k, l) in levels.iter().enumerate() {
        if l.contains('#') && (*l != "#" || k != levels.len() - 1) {
            return false;
        }
        if l.contains('+') && *l != "+" {
            return false;
        }
    }
    true
}

pub fn valid_topic(topic: &str) -> bool {
    !topic.is_empty() && topic.len() <= 1024 && !topic.contains(['+', '#', '\0'])
}

enum Out {
    Publish(Publish),
    Packet(Packet),
    Close,
}

struct Client {
    conn: u64,
    tx: mpsc::Sender<Out>,
    subs: Vec<(String, QoS)>,
    addr: SocketAddr,
    username: Option<String>,
    since: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
struct State {
    clients: HashMap<String, Client>,
    retained: BTreeMap<String, (Bytes, QoS)>,
    dirty: bool,
}

struct Inner {
    state: Mutex<State>,
    settings: Mutex<BrokerSettings>,
    next_conn: AtomicU64,
    received: AtomicU64,
    sent: AtomicU64,
    file: Option<PathBuf>,
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
    error: Mutex<Option<String>>,
    /// Un seul changement de réglages à la fois.
    applying: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct Broker {
    inner: Arc<Inner>,
}

impl Broker {
    /// `file` : où garder les messages retenus (aucun fichier : en mémoire seulement).
    pub fn new(file: Option<PathBuf>) -> Self {
        let retained = file.as_deref().map(load_retained).unwrap_or_default();
        let broker = Broker {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    retained,
                    ..Default::default()
                }),
                settings: Mutex::new(BrokerSettings::default()),
                next_conn: AtomicU64::new(1),
                received: AtomicU64::new(0),
                sent: AtomicU64::new(0),
                file,
                listener: Mutex::new(None),
                error: Mutex::new(None),
                applying: tokio::sync::Mutex::new(()),
            }),
        };
        broker.spawn_saver();
        broker
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn settings(&self) -> BrokerSettings {
        self.inner
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Démarre, arrête ou relance l'écoute selon les réglages. Les clients déjà connectés
    /// sont déconnectés quand l'adresse change (ils se reconnectent d'eux-mêmes).
    pub async fn apply(&self, wanted: BrokerSettings) -> Result<(), String> {
        let _one = self.inner.applying.lock().await;
        let previous = {
            let mut s = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            std::mem::replace(&mut *s, wanted.clone())
        };
        let listening = self
            .inner
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|h| !h.is_finished());
        let rebind = !listening
            || previous.port != wanted.port
            || previous.open != wanted.open
            || previous.enabled != wanted.enabled;
        if !rebind {
            // Seuls les comptes ont changé : on coupe ceux du réseau, ils se reconnecteront
            // avec les nouveaux droits.
            if previous.users != wanted.users {
                self.kick(|c| !c.addr.ip().is_loopback());
            }
            return Ok(());
        }
        let old = self
            .inner
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = old {
            // On attend que l'ancienne écoute soit vraiment fermée : sinon le port est encore pris.
            h.abort();
            let _ = h.await;
        }
        self.kick(|_| true);
        *self.inner.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if !wanted.enabled {
            return Ok(());
        }
        let ip: IpAddr = if wanted.open {
            [0, 0, 0, 0].into()
        } else {
            [127, 0, 0, 1].into()
        };
        let addr = SocketAddr::new(ip, wanted.port);
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "le port {} est déjà pris (un autre broker, Mosquitto ?)",
                        wanted.port
                    )
                } else {
                    format!("impossible d'écouter sur {addr} : {e}")
                };
                tracing::warn!("broker MQTT : {msg}");
                *self.inner.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                return Err(msg);
            }
        };
        tracing::info!("broker MQTT intégré à l'écoute sur {addr}");
        let me = self.clone();
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let me = me.clone();
                        tokio::spawn(async move {
                            if let Err(e) = me.serve(stream, peer).await {
                                tracing::debug!("broker MQTT : {peer} : {e}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("broker MQTT : {e}");
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        });
        *self
            .inner
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle);
        Ok(())
    }

    fn kick(&self, which: impl Fn(&Client) -> bool) {
        let st = self.state();
        for c in st.clients.values().filter(|c| which(c)) {
            let _ = c.tx.try_send(Out::Close);
        }
    }

    /// Pour l'interface : qui est connecté, combien de messages.
    pub fn status(&self) -> Value {
        let settings = self.settings();
        let running = self
            .inner
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|h| !h.is_finished());
        let st = self.state();
        let mut clients: Vec<Value> = st
            .clients
            .iter()
            .map(|(id, c)| {
                json!({
                    "client_id": id,
                    "address": c.addr.ip().to_string(),
                    "local": c.addr.ip().is_loopback(),
                    "username": c.username,
                    "since": c.since,
                    "subscriptions": c.subs.len(),
                })
            })
            .collect();
        clients.sort_by(|a, b| a["client_id"].as_str().cmp(&b["client_id"].as_str()));
        json!({
            "settings": settings.public(),
            "running": running,
            "error": self.inner.error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            "clients": clients,
            "retained": st.retained.len(),
            "received": self.inner.received.load(Ordering::Relaxed),
            "sent": self.inner.sent.load(Ordering::Relaxed),
        })
    }

    pub fn running(&self) -> bool {
        self.inner
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    pub fn client_ids(&self) -> Vec<String> {
        self.state().clients.keys().cloned().collect()
    }

    fn authorize(&self, peer: &SocketAddr, login: Option<&rumqttc::Login>) -> ConnectReturnCode {
        let settings = self.settings();
        if peer.ip().is_loopback() {
            // La box elle-même : Zigbee2MQTT, Heim, un script local.
            return ConnectReturnCode::Success;
        }
        if !settings.open {
            return ConnectReturnCode::NotAuthorized;
        }
        let Some(login) = login else {
            return ConnectReturnCode::BadUserNamePassword;
        };
        let ok = settings.users.iter().any(|u| {
            u.username == login.username && same(u.password.as_bytes(), login.password.as_bytes())
        });
        if ok {
            ConnectReturnCode::Success
        } else {
            ConnectReturnCode::BadUserNamePassword
        }
    }

    async fn serve(&self, stream: TcpStream, peer: SocketAddr) -> anyhow::Result<()> {
        let _ = stream.set_nodelay(true);
        let (mut rd, mut wr) = stream.into_split();
        let mut buf = BytesMut::with_capacity(4096);

        // 1. Le CONNECT, dans les 10 secondes.
        let connect = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match Packet::read(&mut buf, MAX_PACKET) {
                    Ok(Packet::Connect(c)) => return Ok(c),
                    Ok(_) => anyhow::bail!("premier paquet autre que CONNECT"),
                    Err(rumqttc::Error::InsufficientBytes(_)) => {}
                    Err(rumqttc::Error::InvalidProtocolLevel(_)) => {
                        anyhow::bail!("version MQTT non prise en charge")
                    }
                    Err(e) => anyhow::bail!("paquet illisible : {e}"),
                }
                if rd.read_buf(&mut buf).await? == 0 {
                    anyhow::bail!("fermé avant CONNECT");
                }
            }
        })
        .await;
        let connect = match connect {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                if e.to_string().contains("version") {
                    let _ = write_packet(
                        &mut wr,
                        &Packet::ConnAck(ConnAck::new(
                            ConnectReturnCode::RefusedProtocolVersion,
                            false,
                        )),
                    )
                    .await;
                }
                return Err(e);
            }
            Err(_) => anyhow::bail!("pas de CONNECT"),
        };
        if connect.protocol != rumqttc::Protocol::V4 {
            let _ = write_packet(
                &mut wr,
                &Packet::ConnAck(ConnAck::new(
                    ConnectReturnCode::RefusedProtocolVersion,
                    false,
                )),
            )
            .await;
            anyhow::bail!("MQTT 5 non pris en charge (3.1.1 seulement)");
        }
        let code = self.authorize(&peer, connect.login.as_ref());
        if code != ConnectReturnCode::Success {
            let _ = write_packet(&mut wr, &Packet::ConnAck(ConnAck::new(code, false))).await;
            anyhow::bail!("refusé : {code:?}");
        }
        let client_id = if connect.client_id.is_empty() {
            if !connect.clean_session {
                let _ = write_packet(
                    &mut wr,
                    &Packet::ConnAck(ConnAck::new(ConnectReturnCode::BadClientId, false)),
                )
                .await;
                anyhow::bail!("identifiant vide sans session propre");
            }
            let mut raw = [0u8; 6];
            let _ = getrandom::getrandom(&mut raw);
            format!(
                "auto-{}",
                raw.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )
        } else {
            connect.client_id.clone()
        };

        let conn = self.inner.next_conn.fetch_add(1, Ordering::Relaxed);
        let (tx, mut rx) = mpsc::channel::<Out>(QUEUE);
        let full = {
            let mut st = self.state();
            if !st.clients.contains_key(&client_id) && st.clients.len() >= MAX_CLIENTS {
                true
            } else {
                // Même identifiant : l'ancienne connexion laisse la place.
                if let Some(old) = st.clients.insert(
                    client_id.clone(),
                    Client {
                        conn,
                        tx: tx.clone(),
                        subs: Vec::new(),
                        addr: peer,
                        username: connect.login.as_ref().map(|l| l.username.clone()),
                        since: chrono::Utc::now(),
                    },
                ) {
                    let _ = old.tx.try_send(Out::Close);
                }
                false
            }
        };
        if full {
            let _ = write_packet(
                &mut wr,
                &Packet::ConnAck(ConnAck::new(ConnectReturnCode::ServiceUnavailable, false)),
            )
            .await;
            anyhow::bail!("trop de clients");
        }
        tracing::debug!("broker MQTT : {client_id} connecté depuis {peer}");

        // 2. L'écriture : une tâche à part, nourrie par la file.
        let sent = self.inner.clone();
        let writer = tokio::spawn(async move {
            let mut pkid: u16 = 0;
            let mut out = BytesMut::with_capacity(4096);
            if write_packet(
                &mut wr,
                &Packet::ConnAck(ConnAck::new(ConnectReturnCode::Success, false)),
            )
            .await
            .is_err()
            {
                return;
            }
            while let Some(msg) = rx.recv().await {
                out.clear();
                let packet = match msg {
                    Out::Close => break,
                    Out::Packet(p) => p,
                    Out::Publish(mut p) => {
                        if p.qos != QoS::AtMostOnce {
                            pkid = pkid.wrapping_add(1).max(1);
                            p.pkid = pkid;
                        } else {
                            p.pkid = 0;
                        }
                        sent.sent.fetch_add(1, Ordering::Relaxed);
                        Packet::Publish(p)
                    }
                };
                if packet.write(&mut out, MAX_PACKET).is_err() {
                    continue;
                }
                // Tout ce qui attend part d'un coup.
                while let Ok(more) = rx.try_recv() {
                    match more {
                        Out::Close => {
                            let _ = wr.write_all(&out).await;
                            let _ = wr.shutdown().await;
                            return;
                        }
                        Out::Packet(p) => {
                            let _ = p.write(&mut out, MAX_PACKET);
                        }
                        Out::Publish(mut p) => {
                            if p.qos != QoS::AtMostOnce {
                                pkid = pkid.wrapping_add(1).max(1);
                                p.pkid = pkid;
                            } else {
                                p.pkid = 0;
                            }
                            sent.sent.fetch_add(1, Ordering::Relaxed);
                            let _ = Packet::Publish(p).write(&mut out, MAX_PACKET);
                        }
                    }
                    if out.len() > 256 * 1024 {
                        break;
                    }
                }
                if wr.write_all(&out).await.is_err() {
                    break;
                }
            }
            let _ = wr.shutdown().await;
        });

        // 3. La lecture.
        let keep_alive = match connect.keep_alive {
            0 => None,
            k => Some(Duration::from_millis(u64::from(k) * 1500)),
        };
        let mut will = connect.last_will.clone();
        let mut qos2: HashSet<u16> = HashSet::new();
        let result: anyhow::Result<()> = async {
            loop {
                loop {
                    match Packet::read(&mut buf, MAX_PACKET) {
                        Ok(p) => {
                            if self.handle(&client_id, conn, &tx, p, &mut will, &mut qos2)? {
                                return Ok(());
                            }
                        }
                        Err(rumqttc::Error::InsufficientBytes(_)) => break,
                        Err(e) => anyhow::bail!("paquet illisible : {e}"),
                    }
                }
                let read = rd.read_buf(&mut buf);
                let n = match keep_alive {
                    Some(k) => tokio::time::timeout(k, read)
                        .await
                        .map_err(|_| anyhow::anyhow!("silence au-delà du keep-alive"))??,
                    None => read.await?,
                };
                if n == 0 {
                    anyhow::bail!("connexion fermée");
                }
                if tx.is_closed() {
                    // Remplacé par une autre connexion du même client.
                    return Ok(());
                }
            }
        }
        .await;

        // 4. Le départ : ses volontés si ce n'était pas un DISCONNECT, puis on le retire.
        let replaced = {
            let mut st = self.state();
            match st.clients.get(&client_id) {
                Some(c) if c.conn == conn => {
                    st.clients.remove(&client_id);
                    false
                }
                _ => true,
            }
        };
        let _ = tx.try_send(Out::Close);
        drop(tx);
        let _ = writer.await;
        if result.is_err()
            && !replaced
            && let Some(w) = will.take()
        {
            self.route(&client_id, will_publish(w));
        }
        tracing::debug!("broker MQTT : {client_id} parti ({result:?})");
        result
    }

    /// Un paquet du client. `Ok(true)` : il s'en va proprement.
    fn handle(
        &self,
        client_id: &str,
        conn: u64,
        tx: &mpsc::Sender<Out>,
        packet: Packet,
        will: &mut Option<LastWill>,
        qos2: &mut HashSet<u16>,
    ) -> anyhow::Result<bool> {
        match packet {
            Packet::Publish(p) => {
                if !valid_topic(&p.topic) {
                    anyhow::bail!("sujet invalide : {:?}", p.topic);
                }
                self.inner.received.fetch_add(1, Ordering::Relaxed);
                match p.qos {
                    QoS::AtMostOnce => self.route(client_id, p),
                    QoS::AtLeastOnce => {
                        let pkid = p.pkid;
                        self.route(client_id, p);
                        let _ = tx.try_send(Out::Packet(Packet::PubAck(PubAck::new(pkid))));
                    }
                    QoS::ExactlyOnce => {
                        let pkid = p.pkid;
                        // Un doublon (DUP) d'un message déjà reçu n'est pas redistribué.
                        if qos2.insert(pkid) {
                            self.route(client_id, p);
                        }
                        let _ = tx.try_send(Out::Packet(Packet::PubRec(PubRec::new(pkid))));
                    }
                }
            }
            Packet::PubRel(r) => {
                qos2.remove(&r.pkid);
                let _ = tx.try_send(Out::Packet(Packet::PubComp(PubComp::new(r.pkid))));
            }
            // Accusés de nos envois : rien à garder (sessions propres).
            Packet::PubAck(_) | Packet::PubRec(_) | Packet::PubComp(_) => {
                if let Packet::PubRec(r) = packet {
                    let _ = tx.try_send(Out::Packet(Packet::PubRel(rumqttc::PubRel::new(r.pkid))));
                }
            }
            Packet::Subscribe(s) => {
                let mut codes = Vec::with_capacity(s.filters.len());
                let mut fresh = Vec::new();
                {
                    let mut st = self.state();
                    let Some(c) = st.clients.get_mut(client_id).filter(|c| c.conn == conn) else {
                        return Ok(true);
                    };
                    for f in &s.filters {
                        if !valid_filter(&f.path) {
                            codes.push(SubscribeReasonCode::Failure);
                            continue;
                        }
                        let qos = if f.qos == QoS::AtMostOnce {
                            QoS::AtMostOnce
                        } else {
                            QoS::AtLeastOnce
                        };
                        c.subs.retain(|(p, _)| p != &f.path);
                        c.subs.push((f.path.clone(), qos));
                        codes.push(SubscribeReasonCode::Success(qos));
                        fresh.push((f.path.clone(), qos));
                    }
                }
                let _ = tx.try_send(Out::Packet(Packet::SubAck(SubAck::new(s.pkid, codes))));
                // Les messages retenus qui correspondent, juste après le SUBACK.
                let st = self.state();
                for (topic, (payload, rqos)) in &st.retained {
                    let best = fresh
                        .iter()
                        .filter(|(f, _)| matches(f, topic))
                        .map(|(_, q)| *q)
                        .max_by_key(|q| *q as u8);
                    if let Some(q) = best {
                        let mut p =
                            Publish::from_bytes(topic.clone(), min_qos(q, *rqos), payload.clone());
                        p.retain = true;
                        if tx.try_send(Out::Publish(p)).is_err() {
                            break;
                        }
                    }
                }
            }
            Packet::Unsubscribe(u) => {
                if let Some(c) = self
                    .state()
                    .clients
                    .get_mut(client_id)
                    .filter(|c| c.conn == conn)
                {
                    c.subs.retain(|(p, _)| !u.topics.contains(p));
                }
                let _ = tx.try_send(Out::Packet(Packet::UnsubAck(UnsubAck::new(u.pkid))));
            }
            Packet::PingReq => {
                let _ = tx.try_send(Out::Packet(Packet::PingResp));
            }
            Packet::Disconnect => {
                *will = None;
                return Ok(true);
            }
            Packet::Connect(_) => anyhow::bail!("deuxième CONNECT"),
            other => anyhow::bail!("paquet inattendu : {other:?}"),
        }
        Ok(false)
    }

    /// Distribue un message à tous les abonnés (et le retient si demandé).
    fn route(&self, _from: &str, p: Publish) {
        let mut st = self.state();
        if p.retain {
            if p.payload.is_empty() {
                if st.retained.remove(&p.topic).is_some() {
                    st.dirty = true;
                }
            } else if st.retained.len() < MAX_RETAINED || st.retained.contains_key(&p.topic) {
                let changed = st
                    .retained
                    .get(&p.topic)
                    .is_none_or(|(old, q)| old != &p.payload || *q != p.qos);
                if changed {
                    st.retained
                        .insert(p.topic.clone(), (p.payload.clone(), p.qos));
                    st.dirty = true;
                }
            }
        }
        let mut slow = Vec::new();
        for (id, c) in &st.clients {
            let best = c
                .subs
                .iter()
                .filter(|(f, _)| matches(f, &p.topic))
                .map(|(_, q)| *q)
                .max_by_key(|q| *q as u8);
            let Some(q) = best else { continue };
            // Les messages en direct partent sans le drapeau « retenu » (MQTT 3.1.1, 3.3.1.3).
            let mut out =
                Publish::from_bytes(p.topic.clone(), min_qos(q, p.qos), p.payload.clone());
            out.retain = false;
            if let Err(mpsc::error::TrySendError::Full(_)) = c.tx.try_send(Out::Publish(out)) {
                slow.push(id.clone());
            }
        }
        for id in slow {
            tracing::warn!("broker MQTT : {id} ne suit pas, messages perdus");
        }
    }

    /// Publie depuis Heim lui-même (sans passer par le réseau).
    pub fn publish(&self, topic: &str, payload: impl Into<Bytes>, retain: bool) {
        let mut p = Publish::from_bytes(topic, QoS::AtMostOnce, payload.into());
        p.retain = retain;
        self.route("heim", p);
    }

    pub fn retained(&self, topic: &str) -> Option<Bytes> {
        self.state().retained.get(topic).map(|(p, _)| p.clone())
    }

    fn spawn_saver(&self) {
        let Some(file) = self.inner.file.clone() else {
            return;
        };
        let me = Arc::downgrade(&self.inner);
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(inner) = me.upgrade() else { return };
                let snapshot = {
                    let mut st = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                    if !st.dirty {
                        continue;
                    }
                    st.dirty = false;
                    st.retained.clone()
                };
                let file = file.clone();
                let _ = tokio::task::spawn_blocking(move || save_retained(&file, &snapshot)).await;
            }
        });
    }

    /// Écrit tout de suite les messages retenus (à l'arrêt de Heim).
    pub fn flush(&self) {
        let Some(file) = &self.inner.file else { return };
        let snapshot = {
            let mut st = self.state();
            if !st.dirty {
                return;
            }
            st.dirty = false;
            st.retained.clone()
        };
        save_retained(file, &snapshot);
    }
}

fn min_qos(a: QoS, b: QoS) -> QoS {
    if (a as u8) <= (b as u8) { a } else { b }
}

fn will_publish(w: LastWill) -> Publish {
    let mut p = Publish::from_bytes(w.topic, w.qos, w.message);
    p.retain = w.retain;
    p
}

/// Comparaison en temps constant (pas d'indice sur la longueur du bon préfixe).
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn write_packet(wr: &mut tokio::net::tcp::OwnedWriteHalf, p: &Packet) -> anyhow::Result<()> {
    let mut out = BytesMut::new();
    p.write(&mut out, MAX_PACKET)?;
    wr.write_all(&out).await?;
    Ok(())
}

fn load_retained(file: &std::path::Path) -> BTreeMap<String, (Bytes, QoS)> {
    use base64::Engine;
    let Ok(raw) = std::fs::read(file) else {
        return BTreeMap::new();
    };
    let Ok(map) = serde_json::from_slice::<BTreeMap<String, (String, u8)>>(&raw) else {
        tracing::warn!("broker MQTT : {} illisible, ignoré", file.display());
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(t, (p, q))| {
            let payload = base64::engine::general_purpose::STANDARD.decode(p).ok()?;
            let qos = if q == 0 {
                QoS::AtMostOnce
            } else {
                QoS::AtLeastOnce
            };
            Some((t, (Bytes::from(payload), qos)))
        })
        .collect()
}

fn save_retained(file: &std::path::Path, map: &BTreeMap<String, (Bytes, QoS)>) {
    use base64::Engine;
    let out: BTreeMap<&String, (String, u8)> = map
        .iter()
        .map(|(t, (p, q))| {
            (
                t,
                (
                    base64::engine::general_purpose::STANDARD.encode(p),
                    *q as u8,
                ),
            )
        })
        .collect();
    let Ok(raw) = serde_json::to_vec(&out) else {
        return;
    };
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(matches("zigbee2mqtt/#", "zigbee2mqtt/bridge/devices"));
        assert!(matches("zigbee2mqtt/#", "zigbee2mqtt"));
        assert!(matches("zigbee2mqtt/+/set", "zigbee2mqtt/lampe/set"));
        assert!(!matches("zigbee2mqtt/+/set", "zigbee2mqtt/lampe/x/set"));
        assert!(matches("+/+", "a/b"));
        assert!(!matches("+", "a/b"));
        assert!(matches("#", "a/b/c"));
        assert!(!matches("#", "$SYS/x"));
        assert!(matches("$SYS/#", "$SYS/x"));
        assert!(matches("a//b", "a//b"));
        assert!(!matches("a/b", "a/b/c"));
    }

    #[test]
    fn filters() {
        assert!(valid_filter("a/+/b/#"));
        assert!(!valid_filter("a/#/b"));
        assert!(!valid_filter("a/b#"));
        assert!(!valid_filter("a+/b"));
        assert!(!valid_filter(""));
        assert!(valid_topic("zigbee2mqtt/lampe"));
        assert!(!valid_topic("a/+"));
    }

    #[test]
    fn retained_survive_a_restart() {
        let file = std::env::temp_dir().join(format!("heim-retained-{}.json", std::process::id()));
        let mut m = BTreeMap::new();
        m.insert(
            "zigbee2mqtt/bridge/devices".to_string(),
            (Bytes::from_static(b"[1,2]"), QoS::AtMostOnce),
        );
        m.insert(
            "bin".to_string(),
            (Bytes::from_static(&[0, 255, 7]), QoS::AtLeastOnce),
        );
        save_retained(&file, &m);
        assert_eq!(load_retained(&file), m);
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn network_clients_need_an_account() {
        let b = Broker::new(None);
        let lan: SocketAddr = "192.168.1.20:5000".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let login = rumqttc::Login::new("shelly", "bon-mot-de-passe");
        assert_eq!(b.authorize(&local, None), ConnectReturnCode::Success);
        assert_eq!(
            b.authorize(&lan, Some(&login)),
            ConnectReturnCode::NotAuthorized
        );
        *b.inner.settings.lock().unwrap() = BrokerSettings {
            enabled: true,
            open: true,
            users: vec![BrokerUser {
                username: "shelly".into(),
                password: "bon-mot-de-passe".into(),
            }],
            ..Default::default()
        };
        assert_eq!(b.authorize(&lan, Some(&login)), ConnectReturnCode::Success);
        assert_eq!(
            b.authorize(&lan, None),
            ConnectReturnCode::BadUserNamePassword
        );
        let wrong = rumqttc::Login::new("shelly", "mauvais");
        assert_eq!(
            b.authorize(&lan, Some(&wrong)),
            ConnectReturnCode::BadUserNamePassword
        );
    }

    #[test]
    fn settings_check() {
        let mut s = BrokerSettings {
            users: vec![BrokerUser {
                username: " shelly ".into(),
                password: "court".into(),
            }],
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.users[0].password = generate_password();
        assert!(s.validate().is_ok());
        assert_eq!(s.users[0].username, "shelly");
        assert!(s.public()["users"][0].get("password").is_none());
    }
}
