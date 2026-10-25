//! Protocole local Tuya (versions 3.3, 3.4 et 3.5), sans dépendance au cloud.
//!
//! Trames « 55AA » (3.3, 3.4) :
//!   préfixe 000055AA | n° | commande | longueur | [code retour] | charge | CRC32 ou HMAC | 0000AA55
//! Trames « 6699 » (3.5) :
//!   préfixe 00006699 | 0000 | n° | commande | longueur | IV (12) | charge chiffrée | tag (16) | 00009966
//!
//! - 3.3 : AES-128-ECB avec la clé locale, en-tête « 3.3 » + 12 zéros hors chiffrement, CRC32.
//! - 3.4 : clé de session négociée, en-tête « 3.4 » chiffré avec la charge, HMAC-SHA256.
//! - 3.5 : clé de session négociée, AES-128-GCM.
//!
//! Vérifié contre la bibliothèque tinytuya (tests/fake_tuya.py).

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes_gcm::aead::{Aead, Payload};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

pub const PREFIX_55AA: u32 = 0x0000_55AA;
pub const SUFFIX_55AA: u32 = 0x0000_AA55;
pub const PREFIX_6699: u32 = 0x0000_6699;
pub const SUFFIX_6699: u32 = 0x0000_9966;

pub mod cmd {
    pub const SESS_KEY_NEG_START: u32 = 3;
    pub const SESS_KEY_NEG_RESP: u32 = 4;
    pub const SESS_KEY_NEG_FINISH: u32 = 5;
    pub const CONTROL: u32 = 7;
    pub const STATUS: u32 = 8;
    pub const HEART_BEAT: u32 = 9;
    pub const DP_QUERY: u32 = 10;
    pub const CONTROL_NEW: u32 = 13;
    pub const DP_QUERY_NEW: u32 = 16;
    pub const UPDATEDPS: u32 = 18;
}

/// Commandes envoyées sans l'en-tête « 3.x » + 12 zéros.
fn no_header(c: u32) -> bool {
    matches!(
        c,
        cmd::DP_QUERY
            | cmd::DP_QUERY_NEW
            | cmd::UPDATEDPS
            | cmd::HEART_BEAT
            | cmd::SESS_KEY_NEG_START
            | cmd::SESS_KEY_NEG_RESP
            | cmd::SESS_KEY_NEG_FINISH
            | 0x40
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V33,
    V34,
    V35,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "3.3" | "3.2" => Some(Version::V33),
            "3.4" => Some(Version::V34),
            "3.5" => Some(Version::V35),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Version::V33 => "3.3",
            Version::V34 => "3.4",
            Version::V35 => "3.5",
        }
    }

    fn header(self) -> [u8; 15] {
        let mut h = [0u8; 15];
        h[..3].copy_from_slice(self.as_str().as_bytes());
        h
    }

    pub fn negotiates(self) -> bool {
        self != Version::V33
    }
}

// ---- Chiffrement ---------------------------------------------------------------------

fn pad(data: &[u8]) -> Vec<u8> {
    let n = 16 - data.len() % 16;
    let mut v = data.to_vec();
    v.extend(std::iter::repeat_n(n as u8, n));
    v
}

fn unpad(mut data: Vec<u8>) -> Vec<u8> {
    if let Some(&n) = data.last() {
        let n = n as usize;
        if (1..=16).contains(&n)
            && n <= data.len()
            && data[data.len() - n..].iter().all(|&b| b as usize == n)
        {
            data.truncate(data.len() - n);
        }
    }
    data
}

pub fn ecb_encrypt(key: &[u8; 16], data: &[u8], padding: bool) -> Vec<u8> {
    let cipher = Aes128::new(key.into());
    let mut buf = if padding { pad(data) } else { data.to_vec() };
    let (blocks, _) = buf.as_chunks_mut::<16>();
    for block in blocks {
        cipher.encrypt_block(block.into());
    }
    buf
}

pub fn ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(format!("longueur chiffrée invalide ({})", data.len()));
    }
    let cipher = Aes128::new(key.into());
    let mut buf = data.to_vec();
    let (blocks, _) = buf.as_chunks_mut::<16>();
    for block in blocks {
        cipher.decrypt_block(block.into());
    }
    Ok(unpad(buf))
}

fn hmac256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("clé HMAC");
    m.update(data);
    m.finalize().into_bytes().into()
}

fn gcm_encrypt(key: &[u8; 16], iv: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let cipher = aes_gcm::Aes128Gcm::new(key.into());
    cipher
        .encrypt(&aes_gcm::Nonce::from(*iv), Payload { msg: plain, aad })
        .expect("chiffrement GCM")
}

fn gcm_decrypt(
    key: &[u8; 16],
    iv: &[u8],
    aad: &[u8],
    ct_and_tag: &[u8],
) -> Result<Vec<u8>, String> {
    let cipher = aes_gcm::Aes128Gcm::new(key.into());
    let iv: [u8; 12] = iv
        .try_into()
        .map_err(|_| "vecteur d'initialisation invalide".to_string())?;
    cipher
        .decrypt(
            &aes_gcm::Nonce::from(iv),
            Payload {
                msg: ct_and_tag,
                aad,
            },
        )
        .map_err(|_| "message illisible (mauvaise clé locale ?)".to_string())
}

/// Clé de découverte des annonces UDP : md5("yGAdlopoPVldABfn").
pub fn udp_key() -> [u8; 16] {
    use md5::Digest;
    md5::Md5::digest(b"yGAdlopoPVldABfn").into()
}

// ---- Trames ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub seqno: u32,
    pub cmd: u32,
    pub retcode: Option<u32>,
    /// Charge déchiffrée, sans en-tête de version.
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn json(&self) -> Option<Value> {
        let text = std::str::from_utf8(&self.payload).ok()?;
        let text = text.trim_end_matches('\0').trim();
        if text.is_empty() {
            return None;
        }
        serde_json::from_str(text).ok()
    }

    /// Les points de données (« dps ») d'une réponse, quel que soit l'emballage.
    pub fn dps(&self) -> Option<serde_json::Map<String, Value>> {
        let v = self.json()?;
        let dps = v.get("dps").or_else(|| v.pointer("/data/dps"))?;
        dps.as_object().cloned()
    }
}

/// Retire l'en-tête « 3.x » + 12 octets s'il est présent.
fn strip_version(mut data: Vec<u8>) -> Vec<u8> {
    if data.len() >= 15 && data[0] == b'3' && data[1] == b'.' && data[2].is_ascii_digit() {
        data.drain(..15);
    }
    data
}

/// Code retour de l'appareil : 4 octets en tête, presque toujours 0 ou 1.
fn split_retcode(data: &[u8]) -> (Option<u32>, &[u8]) {
    if data.len() >= 4 && data[0] == 0 && data[1] == 0 && data[2] == 0 {
        (
            Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]])),
            &data[4..],
        )
    } else {
        (None, data)
    }
}

/// Une connexion avec un appareil : version, clés, numéro de message.
pub struct Session {
    pub version: Version,
    pub dev_id: String,
    local_key: [u8; 16],
    session_key: [u8; 16],
    seqno: u32,
    local_nonce: [u8; 16],
    /// Appareils « device22 » (3.3, identifiant court) : la lecture passe par CONTROL_NEW.
    pub device22: bool,
}

pub fn parse_key(key: &str) -> Result<[u8; 16], String> {
    let bytes = key.as_bytes();
    if bytes.len() != 16 {
        return Err(format!(
            "la clé locale doit faire 16 caractères (celle-ci en fait {})",
            key.chars().count()
        ));
    }
    let mut k = [0u8; 16];
    k.copy_from_slice(bytes);
    Ok(k)
}

impl Session {
    pub fn new(dev_id: &str, local_key: &str, version: Version) -> Result<Self, String> {
        let key = parse_key(local_key)?;
        Ok(Session {
            version,
            dev_id: dev_id.to_string(),
            local_key: key,
            session_key: key,
            seqno: 1,
            local_nonce: [0u8; 16],
            device22: false,
        })
    }

    /// Fabrique une trame prête à envoyer.
    pub fn encode(&mut self, command: u32, json_payload: &[u8]) -> Vec<u8> {
        let seq = self.seqno;
        self.seqno = self.seqno.wrapping_add(1);
        match self.version {
            Version::V33 => {
                let mut body = Vec::new();
                let enc = ecb_encrypt(&self.local_key, json_payload, true);
                if !no_header(command) {
                    body.extend_from_slice(&self.version.header());
                }
                body.extend_from_slice(&enc);
                frame_55aa(seq, command, &body, None)
            }
            Version::V34 => {
                let mut plain = Vec::new();
                if !no_header(command) {
                    plain.extend_from_slice(&self.version.header());
                }
                plain.extend_from_slice(json_payload);
                let enc = ecb_encrypt(&self.session_key, &plain, true);
                frame_55aa(seq, command, &enc, Some(&self.session_key))
            }
            Version::V35 => {
                let mut plain = Vec::new();
                if !no_header(command) {
                    plain.extend_from_slice(&self.version.header());
                }
                plain.extend_from_slice(json_payload);
                let mut iv = [0u8; 12];
                getrandom::getrandom(&mut iv).expect("aléa");
                frame_6699(seq, command, &plain, &self.session_key, &iv)
            }
        }
    }

    /// Lit une trame en tête de `buf`. `Ok(None)` : pas encore assez d'octets.
    /// Renvoie la trame et le nombre d'octets consommés.
    pub fn decode(&self, buf: &[u8]) -> Result<Option<(Frame, usize)>, String> {
        decode_with(buf, self.version, &self.session_key)
    }

    // -- Négociation de la clé de session (3.4, 3.5) --

    pub fn negotiation_start(&mut self) -> Vec<u8> {
        getrandom::getrandom(&mut self.local_nonce).expect("aléa");
        self.session_key = self.local_key;
        let nonce = self.local_nonce;
        self.encode(cmd::SESS_KEY_NEG_START, &nonce)
    }

    /// Vérifie la réponse de l'appareil et renvoie la dernière trame à envoyer.
    /// Une erreur ici veut presque toujours dire : mauvaise clé locale.
    pub fn negotiation_finish(&mut self, resp: &Frame) -> Result<Vec<u8>, String> {
        if resp.cmd != cmd::SESS_KEY_NEG_RESP || resp.payload.len() < 48 {
            return Err("l'appareil refuse la connexion (clé locale incorrecte ?)".into());
        }
        let remote: [u8; 16] = resp.payload[..16].try_into().expect("16 octets");
        if hmac256(&self.local_key, &self.local_nonce)[..] != resp.payload[16..48] {
            return Err("clé locale incorrecte".into());
        }
        let finish = self.encode(cmd::SESS_KEY_NEG_FINISH, &hmac256(&self.local_key, &remote));
        let mut xor = [0u8; 16];
        for i in 0..16 {
            xor[i] = self.local_nonce[i] ^ remote[i];
        }
        self.session_key = match self.version {
            Version::V34 => ecb_encrypt(&self.local_key, &xor, false)
                .try_into()
                .expect("16 octets"),
            _ => {
                let iv: [u8; 12] = self.local_nonce[..12].try_into().expect("12 octets");
                gcm_encrypt(&self.local_key, &iv, &[], &xor)[..16]
                    .try_into()
                    .expect("16 octets")
            }
        };
        Ok(finish)
    }

    fn timestamp() -> i64 {
        chrono::Utc::now().timestamp()
    }

    /// Demande de l'état complet.
    pub fn query(&mut self, known_dps: &[String]) -> Vec<u8> {
        let id = self.dev_id.clone();
        match self.version {
            Version::V33 if self.device22 => {
                let dps: serde_json::Map<String, Value> = if known_dps.is_empty() {
                    [("1".to_string(), Value::Null)].into_iter().collect()
                } else {
                    known_dps.iter().map(|k| (k.clone(), Value::Null)).collect()
                };
                let body = json!({ "devId": id, "uid": id, "t": Self::timestamp().to_string(), "dps": dps });
                self.encode(cmd::CONTROL_NEW, body.to_string().as_bytes())
            }
            Version::V33 => {
                let body = json!({ "gwId": id, "devId": id, "uid": id, "t": Self::timestamp().to_string() });
                self.encode(cmd::DP_QUERY, body.to_string().as_bytes())
            }
            _ => self.encode(cmd::DP_QUERY_NEW, b"{}"),
        }
    }

    pub fn heartbeat(&mut self) -> Vec<u8> {
        let id = self.dev_id.clone();
        let body = json!({ "gwId": id, "devId": id });
        self.encode(cmd::HEART_BEAT, body.to_string().as_bytes())
    }

    /// Change des points de données : `{"1": true, "22": 500}`.
    pub fn control(&mut self, dps: &serde_json::Map<String, Value>) -> Vec<u8> {
        let id = self.dev_id.clone();
        match self.version {
            Version::V33 => {
                let body = json!({ "devId": id, "uid": id, "t": Self::timestamp().to_string(), "dps": dps });
                self.encode(cmd::CONTROL, body.to_string().as_bytes())
            }
            _ => {
                let body = json!({ "protocol": 5, "t": Self::timestamp(), "data": { "dps": dps } });
                self.encode(cmd::CONTROL_NEW, body.to_string().as_bytes())
            }
        }
    }
}

fn frame_55aa(seq: u32, command: u32, body: &[u8], hmac_key: Option<&[u8; 16]>) -> Vec<u8> {
    let end_len = if hmac_key.is_some() { 36 } else { 8 };
    let mut out = Vec::with_capacity(16 + body.len() + end_len);
    out.extend_from_slice(&PREFIX_55AA.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&command.to_be_bytes());
    out.extend_from_slice(&((body.len() + end_len) as u32).to_be_bytes());
    out.extend_from_slice(body);
    match hmac_key {
        Some(k) => {
            let mac = hmac256(k, &out);
            out.extend_from_slice(&mac);
        }
        None => {
            let crc = crc32fast::hash(&out);
            out.extend_from_slice(&crc.to_be_bytes());
        }
    }
    out.extend_from_slice(&SUFFIX_55AA.to_be_bytes());
    out
}

fn frame_6699(seq: u32, command: u32, plain: &[u8], key: &[u8; 16], iv: &[u8; 12]) -> Vec<u8> {
    let mut out = Vec::with_capacity(18 + 12 + plain.len() + 16 + 4);
    out.extend_from_slice(&PREFIX_6699.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&command.to_be_bytes());
    out.extend_from_slice(&((12 + plain.len() + 16) as u32).to_be_bytes());
    let aad = out[4..18].to_vec();
    let ct = gcm_encrypt(key, iv, &aad, plain);
    out.extend_from_slice(iv);
    out.extend_from_slice(&ct);
    out.extend_from_slice(&SUFFIX_6699.to_be_bytes());
    out
}

/// Décode une trame avec une version et une clé données (sert aussi aux annonces UDP).
pub fn decode_with(
    buf: &[u8],
    version: Version,
    key: &[u8; 16],
) -> Result<Option<(Frame, usize)>, String> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let prefix = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    match prefix {
        PREFIX_55AA => {
            if buf.len() < 16 {
                return Ok(None);
            }
            let seqno = u32::from_be_bytes(buf[4..8].try_into().unwrap());
            let command = u32::from_be_bytes(buf[8..12].try_into().unwrap());
            let length = u32::from_be_bytes(buf[12..16].try_into().unwrap()) as usize;
            if length > 64 * 1024 {
                return Err("trame trop grande : flux désynchronisé".into());
            }
            let total = 16 + length;
            if buf.len() < total {
                return Ok(None);
            }
            let end_len = if version == Version::V33 { 8 } else { 36 };
            if length < end_len {
                return Err("trame trop courte".into());
            }
            let body = &buf[16..total - end_len];
            let (retcode, body) = split_retcode(body);
            let payload = match version {
                Version::V33 => {
                    let body = strip_version(body.to_vec());
                    if body.is_empty()
                        || body.first() == Some(&b'{')
                        || !body.len().is_multiple_of(16)
                    {
                        body
                    } else {
                        ecb_decrypt(key, &body)?
                    }
                }
                _ => {
                    if body.is_empty() {
                        Vec::new()
                    } else {
                        let mac = hmac256(key, &buf[..total - end_len]);
                        if mac[..] != buf[total - end_len..total - 4] {
                            return Err("signature invalide (clé de session ?)".into());
                        }
                        strip_version(ecb_decrypt(key, body)?)
                    }
                }
            };
            Ok(Some((
                Frame {
                    seqno,
                    cmd: command,
                    retcode,
                    payload,
                },
                total,
            )))
        }
        PREFIX_6699 => {
            if buf.len() < 18 {
                return Ok(None);
            }
            let seqno = u32::from_be_bytes(buf[6..10].try_into().unwrap());
            let command = u32::from_be_bytes(buf[10..14].try_into().unwrap());
            let length = u32::from_be_bytes(buf[14..18].try_into().unwrap()) as usize;
            if !(28..=64 * 1024).contains(&length) {
                return Err("trame 3.5 invalide".into());
            }
            let total = 18 + length + 4;
            if buf.len() < total {
                return Ok(None);
            }
            let iv = &buf[18..30];
            let ct = &buf[30..total - 4];
            let plain = gcm_decrypt(key, iv, &buf[4..18], ct)?;
            let (retcode, rest) = match split_retcode(&plain) {
                (Some(c), rest)
                    if rest.first() == Some(&b'{')
                        || rest.starts_with(b"3.")
                        || rest.is_empty()
                        || (command == cmd::SESS_KEY_NEG_RESP && rest.len() >= 48) =>
                {
                    (Some(c), rest)
                }
                _ => (None, &plain[..]),
            };
            Ok(Some((
                Frame {
                    seqno,
                    cmd: command,
                    retcode,
                    payload: strip_version(rest.to_vec()),
                },
                total,
            )))
        }
        _ => {
            // On se recale sur le prochain préfixe connu.
            let skip = buf[1..]
                .windows(4)
                .position(|w| w == PREFIX_55AA.to_be_bytes() || w == PREFIX_6699.to_be_bytes())
                .map(|p| p + 1)
                .unwrap_or(buf.len());
            Err(format!("octets inattendus ignorés ({skip})"))
        }
    }
}

/// Annonce UDP d'un appareil (ports 6666, 6667) : {"ip", "gwId", "version", "productKey"...}.
pub fn decode_broadcast(packet: &[u8]) -> Option<Value> {
    let key = udp_key();
    if packet.len() >= 4 && packet[..4] == PREFIX_6699.to_be_bytes() {
        let (f, _) = decode_with(packet, Version::V35, &key).ok()??;
        return f.json();
    }
    if packet.len() >= 4 && packet[..4] == PREFIX_55AA.to_be_bytes() {
        let length = u32::from_be_bytes(packet.get(12..16)?.try_into().ok()?) as usize;
        let total = 16 + length;
        if packet.len() < total || length < 8 {
            return None;
        }
        let body = &packet[16..total - 8];
        let (_, body) = split_retcode(body);
        if body.first() == Some(&b'{') {
            return serde_json::from_slice(body).ok();
        }
        let plain = ecb_decrypt(&key, body).ok()?;
        return serde_json::from_slice(
            std::str::from_utf8(&plain)
                .ok()?
                .trim_end_matches('\0')
                .as_bytes(),
        )
        .ok();
    }
    let plain = ecb_decrypt(&key, packet).ok()?;
    serde_json::from_slice(&plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecb_roundtrip_and_padding() {
        let key = *b"0123456789abcdef";
        let enc = ecb_encrypt(&key, b"{\"dps\":{\"1\":true}}", true);
        assert_eq!(enc.len() % 16, 0);
        assert_eq!(ecb_decrypt(&key, &enc).unwrap(), b"{\"dps\":{\"1\":true}}");
    }

    #[test]
    fn frames_roundtrip_for_each_version() {
        for v in [Version::V33, Version::V34, Version::V35] {
            let mut s = Session::new("bf1234567890abcdefgh", "0123456789abcdef", v).unwrap();
            let mut dps = serde_json::Map::new();
            dps.insert("1".into(), json!(true));
            let bytes = s.control(&dps);
            let (frame, used) = s.decode(&bytes).unwrap().unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(frame.dps().unwrap()["1"], json!(true), "{v:?}");
            // Trame coupée en deux : on attend la suite.
            assert!(s.decode(&bytes[..bytes.len() - 3]).unwrap().is_none());
        }
    }

    #[test]
    fn bad_key_length_is_explained() {
        assert!(Session::new("x", "court", Version::V33).is_err());
    }
}

#[cfg(test)]
mod broadcast_probe {
    #[test]
    fn decodes_35_broadcast() {
        let hex = "000066990000000000010000001300000067633c5e2bc4d8d58e8d9ccc437fec4bfd9655bf4924c2cc25988fe870e7d32d7ad63e1b3e4b24f0be3e4adc199eb4d99008a8ce52124c6d4a0b90775ce8983c1b45dcd327f765a2005aee11092d27d8128af60da54f6beaec0260205d7d8488d57b50774d31808800009966";
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let v = super::decode_broadcast(&bytes);
        assert_eq!(
            v.unwrap()["gwId"],
            serde_json::json!("bf00000000000000000035")
        );
    }
}
