//! Les noms en `.local` (homeassistant.local, broker.local…).
//!
//! Heim est compilé avec musl : son résolveur ne lit que les serveurs DNS, il ne connaît pas
//! mDNS (avahi, Bonjour). Un nom en `.local` y donne « Name does not resolve ». On pose donc
//! nous-mêmes la question sur le réseau local (224.0.0.251, port 5353), en « unicast hérité »
//! (RFC 6762 §6.7) : le premier appareil qui porte ce nom répond directement à Heim.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

const GROUP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);

/// Le nom finit par `.local` ?
pub fn is_local_name(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    h.ends_with(".local") && h.len() > ".local".len()
}

/// L'adresse à utiliser pour se connecter : l'IP trouvée en mDNS pour un nom en `.local`,
/// sinon le nom tel quel (adresses IP et noms DNS ordinaires ne sont pas touchés).
pub async fn resolve(host: &str) -> String {
    let host = host.trim();
    if !is_local_name(host) {
        return host.to_string();
    }
    match lookup(host, Duration::from_millis(1500)).await {
        Some(ip) => {
            tracing::debug!("mdns : {host} = {ip}");
            ip.to_string()
        }
        None => host.to_string(),
    }
}

/// `ws://heim-matter.local:5580/ws` → la même adresse avec l'IP à la place du nom `.local`.
pub async fn resolve_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority = rest.split('/').next().unwrap_or("");
    let name = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    if !is_local_name(name) {
        return url.to_string();
    }
    let ip = resolve(name).await;
    format!("{scheme}://{}", rest.replacen(name, &ip, 1))
}

/// Cherche un nom `.local` sur le réseau. Deux essais, puis rien.
pub async fn lookup(host: &str, wait: Duration) -> Option<Ipv4Addr> {
    let name = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let query = build_query(&name)?;
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let _ = sock.set_multicast_ttl_v4(255);
    let mut buf = vec![0u8; 9000];
    for _ in 0..2 {
        if sock.send_to(&query, GROUP).await.is_err() {
            return None;
        }
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Ok((n, _))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await
        {
            if let Some(ip) = parse_answer(&buf[..n], &name) {
                return Some(ip);
            }
        }
    }
    None
}

/// Une question mDNS : type A, classe IN avec le bit « réponse directe » (QU).
fn build_query(name: &str) -> Option<Vec<u8>> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0x80, 1]);
    Some(q)
}

/// Lit un nom (avec les renvois de compression). Renvoie le nom et la position après lui.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xC0 == 0xC0 {
            let ptr = ((len & 0x3F) << 8) | *msg.get(pos + 1)? as usize;
            if end.is_none() {
                end = Some(pos + 2);
            }
            jumps += 1;
            if jumps > 16 {
                return None;
            }
            pos = ptr;
            continue;
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        pos += 1 + len;
    }
    Some((labels.join("."), end.unwrap_or(pos)))
}

/// La première adresse IPv4 donnée pour `name` dans une réponse.
fn parse_answer(msg: &[u8], name: &str) -> Option<Ipv4Addr> {
    if msg.len() < 12 || msg[2] & 0x80 == 0 {
        return None; // Pas une réponse.
    }
    let count = |i: usize| u16::from_be_bytes([msg[i], msg[i + 1]]) as usize;
    let (qd, records) = (count(4), count(6) + count(8) + count(10));
    let mut pos = 12;
    for _ in 0..qd {
        pos = read_name(msg, pos)?.1 + 4;
    }
    for _ in 0..records {
        let (owner, p) = read_name(msg, pos)?;
        let head = msg.get(p..p + 10)?;
        let kind = u16::from_be_bytes([head[0], head[1]]);
        let len = u16::from_be_bytes([head[8], head[9]]) as usize;
        let data = msg.get(p + 10..p + 10 + len)?;
        if kind == 1 && len == 4 && owner == name {
            return Some(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        pos = p + 10 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_names() {
        assert!(is_local_name("homeassistant.local"));
        assert!(is_local_name("HomeAssistant.Local."));
        assert!(!is_local_name("192.168.1.10"));
        assert!(!is_local_name("example.com"));
        assert!(!is_local_name(".local"));
    }

    #[tokio::test]
    async fn urls_without_local_names_are_untouched() {
        assert_eq!(
            resolve_url("ws://192.168.1.5:5580/ws").await,
            "ws://192.168.1.5:5580/ws"
        );
        assert_eq!(resolve_url("pas une adresse").await, "pas une adresse");
    }

    #[test]
    fn reads_an_answer() {
        // Réponse : 0 question, 1 réponse A pour homeassistant.local = 192.168.1.20,
        // suivie d'un enregistrement additionnel pointant vers le même nom (compression).
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 1];
        let owner = m.len();
        m.extend_from_slice(b"\x0dhomeassistant\x05local\x00");
        m.extend_from_slice(&[0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 192, 168, 1, 20]);
        m.extend_from_slice(&[0xC0, owner as u8, 0, 28, 0x80, 1, 0, 0, 0, 120, 0, 16]);
        m.extend_from_slice(&[0u8; 16]);
        assert_eq!(
            parse_answer(&m, "homeassistant.local"),
            Some(Ipv4Addr::new(192, 168, 1, 20))
        );
        assert_eq!(parse_answer(&m, "autre.local"), None);
        // Une question n'est pas une réponse.
        let q = build_query("homeassistant.local").unwrap();
        assert_eq!(parse_answer(&q, "homeassistant.local"), None);
    }
}
