//! Les clés locales Tuya, récupérées en scannant un QR code avec l'appli Smart Life.
//!
//! C'est la méthode de l'intégration Tuya de Home Assistant (SDK « device sharing ») : pas de
//! compte développeur. L'utilisateur donne son « code utilisateur » (Smart Life › Moi ›
//! Réglages › Compte et sécurité › Code utilisateur), Heim affiche un QR code, l'appli le
//! scanne et autorise l'accès. Heim lit alors la liste des appareils de la maison, avec leur
//! clé locale et leurs points de données, puis ne parle plus au cloud : tout passe en local.
//!
//! Les jetons ne sont pas conservés : une fois les clés lues, ils sont oubliés.

use std::collections::BTreeMap;
#[cfg(feature = "webhook")]
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;

/// L'identifiant client du SDK, le même que celui de Home Assistant.
pub const CLIENT_ID: &str = "HA_3y9q4ak7g4ephrvke";
pub const SCHEMA: &str = "haauthorize";
const LOGIN_HOST: &str = "https://apigw.iotbing.com";
const NONCE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZabcdefhijkmnprstwxyz2345678";

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("clé HMAC");
    m.update(msg);
    hex(&m.finalize().into_bytes())
}

fn md5_hex(msg: &[u8]) -> String {
    use md5::Digest;
    hex(&md5::Md5::digest(msg))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("aléa");
    b
}

pub fn uuid4() -> String {
    let mut b = random_bytes::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn nonce() -> [u8; 12] {
    random_bytes::<12>().map(|r| NONCE_CHARS[r as usize % NONCE_CHARS.len()])
}

/// Clés d'une requête : (hash_key, secret).
pub fn request_keys(rid: &str, refresh_token: &str) -> (String, String) {
    let hash_key = md5_hex(format!("{rid}{refresh_token}").as_bytes());
    let secret = hmac_hex(rid.as_bytes(), hash_key.as_bytes())[..16].to_string();
    (hash_key, secret)
}

pub fn encrypt(plain: &str, secret: &str, nonce: [u8; 12]) -> String {
    let key: [u8; 16] = secret
        .as_bytes()
        .try_into()
        .expect("secret de 16 caractères");
    let c = aes_gcm::Aes128Gcm::new(&key.into());
    let ct = c
        .encrypt(&aes_gcm::Nonce::from(nonce), plain.as_bytes())
        .expect("chiffrement GCM");
    format!("{}{}", B64.encode(nonce), B64.encode(ct))
}

pub fn decrypt(data: &str, secret: &str) -> Result<String, String> {
    let raw = B64
        .decode(data.trim())
        .map_err(|_| "réponse Tuya illisible".to_string())?;
    if raw.len() < 12 + 16 {
        return Err("réponse Tuya trop courte".into());
    }
    let key: [u8; 16] = secret
        .as_bytes()
        .try_into()
        .map_err(|_| "secret".to_string())?;
    let nonce: [u8; 12] = raw[..12].try_into().expect("12 octets");
    let c = aes_gcm::Aes128Gcm::new(&key.into());
    let plain = c
        .decrypt(&aes_gcm::Nonce::from(nonce), &raw[12..])
        .map_err(|_| "réponse Tuya indéchiffrable".to_string())?;
    String::from_utf8(plain).map_err(|_| "réponse Tuya illisible".to_string())
}

/// La signature des en-têtes et des données chiffrées.
pub fn sign(hash_key: &str, headers: &[(&str, &str)], query_enc: &str, body_enc: &str) -> String {
    let mut s = headers
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("||");
    s.push_str(query_enc);
    s.push_str(body_enc);
    hmac_hex(hash_key.as_bytes(), s.as_bytes())
}

/// Un « code utilisateur » Smart Life : quelques lettres et chiffres.
pub fn check_user_code(code: &str) -> Result<String, String> {
    let c = code.trim();
    if c.is_empty() || c.len() > 64 || !c.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("code utilisateur Smart Life invalide".into());
    }
    Ok(c.to_string())
}

/// Le contenu du QR code à scanner avec l'appli.
pub fn qr_payload(qrcode: &str) -> String {
    format!("tuyaSmart--qrLogin?token={qrcode}")
}

pub fn qr_svg(payload: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build())
}

/// Le serveur de la région du compte (donné par Tuya) : seulement un domaine Tuya en https.
fn check_endpoint(e: &str) -> Result<String, String> {
    let e = e.trim().trim_end_matches('/');
    let host = e.strip_prefix("https://").ok_or("serveur Tuya inattendu")?;
    let ok = !host.contains(['/', '@', ':'])
        && (host.ends_with(".iotbing.com")
            || host
                .split('.')
                .rev()
                .nth(1)
                .is_some_and(|d| d.starts_with("tuya")));
    if !ok {
        return Err("serveur Tuya inattendu".into());
    }
    Ok(e.to_string())
}

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "webhook"), allow(dead_code))]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub endpoint: String,
}

/// Points de données du cloud (`dpStatusRelationDTOS`) → « mapping » à la tinytuya.
pub fn mapping_of(status: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for dp in status
        .get("dpStatusRelationDTOS")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = dp
            .get("dpId")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        else {
            continue;
        };
        let Some(code) = dp
            .get("statusCode")
            .or_else(|| dp.get("dpCode"))
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let values = match dp.get("valueDesc") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
            Some(v) => v.clone(),
            None => Value::Null,
        };
        let kind = dp.get("valueType").and_then(Value::as_str).map(|t| {
            let mut c = t.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        });
        out.insert(
            id.to_string(),
            json!({ "code": code, "type": kind, "values": values }),
        );
    }
    out
}

/// Un appareil du cloud au format `devices.json` de tinytuya.
pub fn tinytuya_entry(d: &Value, mapping: Map<String, Value>) -> Value {
    json!({
        "id": d.get("id"),
        "name": d.get("name"),
        "key": d.get("local_key").or_else(|| d.get("localKey")),
        "category": d.get("category"),
        "product_name": d.get("product_name").or_else(|| d.get("productName")),
        "sub": d.get("sub"),
        "mapping": mapping,
    })
}

#[cfg(feature = "webhook")]
pub struct Cloud {
    http: reqwest::Client,
}

#[cfg(feature = "webhook")]
impl Default for Cloud {
    fn default() -> Self {
        Cloud {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[cfg(feature = "webhook")]
fn answer(v: &Value) -> Result<Value, String> {
    if v.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
    let msg = v.get("msg").and_then(Value::as_str).unwrap_or("refusé");
    Err(format!("Tuya : {msg}"))
}

#[cfg(feature = "webhook")]
impl Cloud {
    /// Le jeton du QR code à afficher.
    pub async fn qr_start(&self, user_code: &str) -> Result<String, String> {
        let r: Value = self
            .http
            .post(format!(
                "{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens"
            ))
            .query(&[
                ("clientid", CLIENT_ID),
                ("usercode", user_code),
                ("schema", SCHEMA),
            ])
            .send()
            .await
            .map_err(|e| format!("Tuya injoignable : {e}"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        answer(&r)?
            .get("qrcode")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "Tuya n'a pas donné de QR code".into())
    }

    /// `None` tant que le QR code n'est pas scanné et validé dans l'appli.
    pub async fn qr_result(&self, qrcode: &str, user_code: &str) -> Result<Option<Token>, String> {
        if !qrcode
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("QR code invalide".into());
        }
        let r: Value = self
            .http
            .get(format!(
                "{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens/{qrcode}"
            ))
            .query(&[("clientid", CLIENT_ID), ("usercode", user_code)])
            .send()
            .await
            .map_err(|e| format!("Tuya injoignable : {e}"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if r.get("success").and_then(Value::as_bool) != Some(true) {
            return Ok(None);
        }
        let res = r.get("result").cloned().unwrap_or_default();
        let s = |k: &str| res.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        if s("access_token").is_empty() {
            return Ok(None);
        }
        Ok(Some(Token {
            access_token: s("access_token"),
            refresh_token: s("refresh_token"),
            endpoint: check_endpoint(&s("endpoint"))?,
        }))
    }

    async fn get(&self, t: &Token, path: &str, params: Option<Value>) -> Result<Value, String> {
        let rid = uuid4();
        let (hash_key, secret) = request_keys(&rid, &t.refresh_token);
        let query_enc = params
            .filter(|p| p.as_object().is_some_and(|o| !o.is_empty()))
            .map(|p| encrypt(&p.to_string(), &secret, nonce()))
            .unwrap_or_default();
        let time = chrono::Utc::now().timestamp_millis().to_string();
        let headers = [
            ("X-appKey", CLIENT_ID),
            ("X-requestId", rid.as_str()),
            ("X-sid", ""),
            ("X-time", time.as_str()),
            ("X-token", t.access_token.as_str()),
        ];
        let sig = sign(&hash_key, &headers, &query_enc, "");
        let mut req = self.http.get(format!("{}{path}", t.endpoint));
        for (k, v) in headers {
            req = req.header(k, v);
        }
        req = req.header("X-sign", sig);
        if !query_enc.is_empty() {
            req = req.query(&[("encdata", query_enc.as_str())]);
        }
        let r: Value = req
            .send()
            .await
            .map_err(|e| format!("Tuya injoignable : {e}"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        match answer(&r)? {
            Value::String(enc) => {
                let plain = decrypt(&enc, &secret)?;
                Ok(serde_json::from_str(&plain).unwrap_or(Value::String(plain)))
            }
            other => Ok(other),
        }
    }

    /// Tous les appareils des maisons du compte, au format tinytuya.
    pub async fn devices(&self, t: &Token) -> Result<Vec<Value>, String> {
        let homes = self.get(t, "/v1.0/m/life/users/homes", None).await?;
        let mut out = Vec::new();
        let mut seen = BTreeMap::new();
        for h in homes.as_array().into_iter().flatten() {
            let Some(home) = h
                .get("ownerId")
                .map(|v| v.as_str().map(str::to_string).unwrap_or(v.to_string()))
            else {
                continue;
            };
            let list = self
                .get(
                    t,
                    "/v1.0/m/life/ha/home/devices",
                    Some(json!({ "homeId": home })),
                )
                .await?;
            for d in list.as_array().into_iter().flatten() {
                let Some(id) = d.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if seen.insert(id.to_string(), ()).is_some() {
                    continue;
                }
                let status = self
                    .get(t, &format!("/v1.0/m/life/devices/{id}/status"), None)
                    .await
                    .unwrap_or_default();
                out.push(tinytuya_entry(d, mapping_of(&status)));
            }
        }
        Ok(out)
    }
}

/// Sans client HTTP (binaire compilé sans `webhook`) : la connexion Smart Life n'existe pas.
#[cfg(not(feature = "webhook"))]
#[derive(Default)]
pub struct Cloud;

#[cfg(not(feature = "webhook"))]
impl Cloud {
    pub async fn qr_start(&self, _: &str) -> Result<String, String> {
        Err("connexion Smart Life absente de ce binaire".into())
    }
    pub async fn qr_result(&self, _: &str, _: &str) -> Result<Option<Token>, String> {
        Err("connexion Smart Life absente de ce binaire".into())
    }
    pub async fn devices(&self, _: &Token) -> Result<Vec<Value>, String> {
        Err("connexion Smart Life absente de ce binaire".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_roundtrip_and_shapes() {
        let (hash_key, secret) = request_keys("rid", "refresh");
        assert_eq!(hash_key, md5_hex(b"ridrefresh"));
        assert_eq!(secret.len(), 16);
        let enc = encrypt(r#"{"homeId":"1"}"#, &secret, *b"ABCDEFGHJKMN");
        assert!(enc.starts_with(&B64.encode(b"ABCDEFGHJKMN")));
        assert_eq!(decrypt(&enc, &secret).unwrap(), r#"{"homeId":"1"}"#);
        let u = uuid4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(check_endpoint("https://apigw.tuyaeu.com").is_ok());
        assert!(check_endpoint("https://evil.example.com").is_err());
        assert!(check_endpoint("http://apigw.tuyaeu.com").is_err());
        assert!(qr_svg(&qr_payload("abc")).unwrap().contains("<svg"));
    }

    /// Mêmes valeurs que le SDK Python (hashlib, hmac, cryptography AESGCM).
    #[test]
    fn matches_python_sdk() {
        let (hash_key, secret) = request_keys("rid", "refresh");
        assert_eq!(secret, "b1101aa39ac4167f");
        let enc = encrypt(r#"{"homeId":"1"}"#, &secret, *b"ABCDEFGHJKMN");
        assert_eq!(
            enc,
            "QUJDREVGR0hKS01Ow+1wpRvz+UVJlA9+qbNQU9YRlxeXzCfA1Xmm0hEf"
        );
        let h = [
            ("X-appKey", "a"),
            ("X-requestId", "rid"),
            ("X-sid", ""),
            ("X-time", "1"),
            ("X-token", "t"),
        ];
        assert_eq!(
            sign(&hash_key, &h, &enc, ""),
            "5b0a96bda796c09179356b7026a34336d143182927ed63bbfe4688d4e6b3e18e"
        );
    }

    #[test]
    fn signature_skips_empty_headers() {
        let a = sign(
            "k",
            &[("X-appKey", "a"), ("X-sid", ""), ("X-time", "1")],
            "Q",
            "",
        );
        assert_eq!(a, hmac_hex(b"k", b"X-appKey=a||X-time=1Q"));
    }

    #[test]
    fn mapping_from_cloud() {
        let status = json!({ "dpStatusRelationDTOS": [
            { "dpId": 1, "statusCode": "switch_1", "valueType": "Boolean", "valueDesc": "{}", "supportLocal": true },
            { "dpId": 19, "statusCode": "cur_power", "valueType": "Integer",
              "valueDesc": "{\"min\":0,\"max\":99999,\"scale\":1,\"unit\":\"W\"}", "supportLocal": true }
        ]});
        let m = mapping_of(&status);
        assert_eq!(m["1"]["code"], "switch_1");
        assert_eq!(m["19"]["values"]["scale"], 1);
        let d = json!({ "id": "bf123", "name": "Prise", "local_key": "0123456789abcdef", "category": "cz" });
        let text = json!([tinytuya_entry(&d, m)]).to_string();
        let devs = super::super::tuya::import_tinytuya(&text).unwrap();
        assert_eq!(devs[0].key, "0123456789abcdef");
        assert_eq!(devs[0].mapping["19"].code, "cur_power");
    }
}
