//! Notifications push (Web Push), sans service tiers.
//!
//! Le téléphone donne une adresse (`endpoint`, chez Apple ou Google) et deux clés. Heim chiffre
//! le message pour ce téléphone seul (RFC 8291, aes128gcm) et le signe avec sa propre clé
//! (VAPID, RFC 8292). Apple et Google ne voient qu'un message chiffré.
//!
//! Sur iPhone : iOS 16.4 ou plus, Heim ajouté à l'écran d'accueil, adresse en HTTPS.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chrono::{DateTime, Utc};
use p256::ecdsa::signature::Signer;
use p256::elliptic_curve::sec1::ToSec1Point;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SETTINGS_KEY: &str = "push.vapid";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

/// Les services push des navigateurs (iPhone, Chrome/Android, Firefox, Edge).
pub fn known_push_service(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    const EXACT: [&str; 4] = [
        "fcm.googleapis.com",
        "android.googleapis.com",
        "updates.push.services.mozilla.com",
        "push.services.mozilla.com",
    ];
    EXACT.contains(&host.as_str())
        || host.ends_with(".push.apple.com")
        || host == "web.push.apple.com"
        || host.ends_with(".notify.windows.com")
}

/// Ce que `PushManager.subscribe()` renvoie dans le navigateur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
    #[serde(default = "Utc::now")]
    pub created: DateTime<Utc>,
}

impl Subscription {
    pub fn validate(&self) -> Result<(), String> {
        if !self.endpoint.starts_with("https://") || self.endpoint.len() > 2048 {
            return Err("adresse d'abonnement invalide".into());
        }
        // Heim n'envoie qu'aux vrais services push (Apple, Google, Mozilla, Microsoft) : une
        // adresse inventée ne doit pas le faire écrire ailleurs (box, autre machine du réseau).
        if !known_push_service(&self.service()) {
            return Err(format!(
                "service de notifications inconnu : {}",
                self.service()
            ));
        }
        let p = B64
            .decode(self.keys.p256dh.trim_end_matches('='))
            .map_err(|_| "clé p256dh illisible")?;
        let a = B64
            .decode(self.keys.auth.trim_end_matches('='))
            .map_err(|_| "clé auth illisible")?;
        if p.len() != 65 || a.len() != 16 {
            return Err("clés d'abonnement invalides".into());
        }
        Ok(())
    }

    /// « web.push.apple.com », « fcm.googleapis.com »... pour l'affichage.
    pub fn service(&self) -> String {
        self.endpoint
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

/// La paire de clés VAPID de ce Heim, créée au premier démarrage et gardée dans les réglages.
#[derive(Clone, Serialize, Deserialize)]
pub struct Vapid {
    /// Clé privée (32 octets, base64url).
    private: String,
    /// Clé publique non compressée (65 octets, base64url) : donnée au navigateur.
    pub public: String,
    /// Contact demandé par Apple et Google (mailto: ou https:).
    #[serde(default = "default_subject")]
    pub subject: String,
}

fn default_subject() -> String {
    "mailto:heim@example.com".into()
}

impl Vapid {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        let key = loop {
            getrandom::getrandom(&mut bytes).expect("source d'aléa du système");
            if let Ok(k) = p256::SecretKey::from_slice(&bytes) {
                break k;
            }
        };
        Self::from_secret(&key)
    }

    fn from_secret(key: &p256::SecretKey) -> Self {
        let public = key.public_key().to_sec1_point(false);
        Vapid {
            private: B64.encode(key.to_bytes()),
            public: B64.encode(public.as_bytes()),
            subject: default_subject(),
        }
    }

    fn signing_key(&self) -> Result<p256::ecdsa::SigningKey, String> {
        let raw = B64.decode(&self.private).map_err(|e| e.to_string())?;
        p256::ecdsa::SigningKey::from_slice(&raw).map_err(|e| e.to_string())
    }

    /// En-tête `Authorization: vapid t=<JWT>, k=<clé publique>` pour un service push.
    pub fn authorization(&self, endpoint: &str) -> Result<String, String> {
        let origin = {
            let rest = endpoint
                .strip_prefix("https://")
                .ok_or("endpoint non HTTPS")?;
            format!("https://{}", rest.split('/').next().unwrap_or_default())
        };
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = B64.encode(
            json!({
                "aud": origin,
                "exp": Utc::now().timestamp() + 12 * 3600,
                "sub": self.subject,
            })
            .to_string(),
        );
        let input = format!("{header}.{claims}");
        let sig: p256::ecdsa::Signature = self.signing_key()?.sign(input.as_bytes());
        Ok(format!(
            "vapid t={input}.{}, k={}",
            B64.encode(sig.to_bytes()),
            self.public
        ))
    }
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), ikm)
        .expand(info, &mut out)
        .expect("longueur HKDF valide");
    out
}

/// Chiffre `plaintext` pour un abonnement (RFC 8291). `salt` et la clé éphémère sont tirés
/// au hasard ; les tests les fixent pour retrouver le vecteur de la RFC.
pub fn encrypt(sub: &Subscription, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    let mut eph = [0u8; 32];
    let secret = loop {
        getrandom::getrandom(&mut eph).map_err(|e| e.to_string())?;
        if let Ok(k) = p256::SecretKey::from_slice(&eph) {
            break k;
        }
    };
    encrypt_with(sub, plaintext, &salt, &secret)
}

fn encrypt_with(
    sub: &Subscription,
    plaintext: &[u8],
    salt: &[u8; 16],
    as_secret: &p256::SecretKey,
) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    if plaintext.len() > 3800 {
        return Err("message trop long pour une notification".into());
    }
    let ua_public_raw = B64
        .decode(sub.keys.p256dh.trim_end_matches('='))
        .map_err(|_| "clé p256dh illisible")?;
    let auth = B64
        .decode(sub.keys.auth.trim_end_matches('='))
        .map_err(|_| "clé auth illisible")?;
    let ua_public =
        p256::PublicKey::from_sec1_bytes(&ua_public_raw).map_err(|_| "clé p256dh invalide")?;
    let as_public = as_secret.public_key().to_sec1_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());

    // RFC 8291 §3.4
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(&ua_public_raw);
    key_info.extend_from_slice(as_public.as_bytes());
    let ikm = hkdf(&auth, shared.raw_secret_bytes(), &key_info, 32);
    // RFC 8188 §2.2 et 2.3
    let cek = hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
    let nonce = hkdf(salt, &ikm, b"Content-Encoding: nonce\0", 12);

    let mut record = plaintext.to_vec();
    record.push(0x02); // dernier (et seul) enregistrement
    let cipher = aes_gcm::Aes128Gcm::new_from_slice(&cek).map_err(|e| e.to_string())?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| "nonce invalide")?;
    let ciphertext = cipher
        .encrypt(&aes_gcm::Nonce::from(nonce), record.as_slice())
        .map_err(|_| "chiffrement impossible")?;

    let mut body = Vec::with_capacity(86 + ciphertext.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&4096u32.to_be_bytes());
    body.push(65);
    body.extend_from_slice(as_public.as_bytes());
    body.extend_from_slice(&ciphertext);
    Ok(body)
}

#[derive(Debug, PartialEq)]
pub enum PushError {
    /// L'abonnement n'existe plus (application désinstallée, autorisation retirée) : l'oublier.
    Gone,
    Other(String),
}

/// Contenu lu par le service worker (`static/sw.js`).
pub fn payload(title: &str, message: &str, level: &str, url: &str, tag: Option<&str>) -> Value {
    json!({ "title": title, "message": message, "level": level, "url": url, "tag": tag })
}

#[cfg(feature = "webhook")]
pub async fn send(
    http: &reqwest::Client,
    vapid: &Vapid,
    sub: &Subscription,
    payload: &Value,
    urgent: bool,
) -> Result<(), PushError> {
    let body = encrypt(sub, payload.to_string().as_bytes()).map_err(PushError::Other)?;
    let auth = vapid
        .authorization(&sub.endpoint)
        .map_err(PushError::Other)?;
    let resp = http
        .post(&sub.endpoint)
        .header("Authorization", auth)
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header("TTL", if urgent { "3600" } else { "86400" })
        .header("Urgency", if urgent { "high" } else { "normal" })
        .body(body)
        .send()
        .await
        .map_err(|e| PushError::Other(format!("service push injoignable : {e}")))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        404 | 410 => Err(PushError::Gone),
        code => {
            let text = resp.text().await.unwrap_or_default();
            Err(PushError::Other(format!(
                "service push : réponse {code} {}",
                text.chars().take(200).collect::<String>()
            )))
        }
    }
}

#[cfg(not(feature = "webhook"))]
pub async fn send(
    _http: &(),
    _vapid: &Vapid,
    _sub: &Subscription,
    _payload: &Value,
    _urgent: bool,
) -> Result<(), PushError> {
    Err(PushError::Other("Heim compilé sans accès HTTP".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vecteur de test de la RFC 8291, annexe A.
    #[test]
    fn rfc8291_test_vector() {
        let sub = Subscription {
            endpoint: "https://fcm.googleapis.com/fcm/send/JzLQ3raZJfFBR0aqvOMsLrt54w4rJUsV".into(),
            keys: SubscriptionKeys {
                p256dh: "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4".into(),
                auth: "BTBZMqHH6r4Tts7J_aSIgg".into(),
            },
            created: Utc::now(),
        };
        let as_secret = p256::SecretKey::from_slice(
            &B64.decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")
                .unwrap(),
        )
        .unwrap();
        let salt: [u8; 16] = B64
            .decode("DGv6ra1nlYgDCS1FRnbzlw")
            .unwrap()
            .try_into()
            .unwrap();
        let body = encrypt_with(
            &sub,
            b"When I grow up, I want to be a watermelon",
            &salt,
            &as_secret,
        )
        .unwrap();
        assert_eq!(
            B64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn vapid_jwt_is_verifiable() {
        use p256::ecdsa::signature::Verifier;
        let v = Vapid::generate();
        let auth = v
            .authorization("https://web.push.apple.com/QGuQyavXutnMH")
            .unwrap();
        let t = auth
            .strip_prefix("vapid t=")
            .unwrap()
            .split(", k=")
            .next()
            .unwrap();
        let (input, sig) = t.rsplit_once('.').unwrap();
        let claims: Value =
            serde_json::from_slice(&B64.decode(input.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims["aud"], json!("https://web.push.apple.com"));
        let public =
            p256::ecdsa::VerifyingKey::from_sec1_bytes(&B64.decode(&v.public).unwrap()).unwrap();
        let sig = p256::ecdsa::Signature::from_slice(&B64.decode(sig).unwrap()).unwrap();
        assert!(public.verify(input.as_bytes(), &sig).is_ok());
    }
}
