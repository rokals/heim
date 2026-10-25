//! Protections côté navigateur, pour toutes les réponses de Heim.
//!
//! - **Origine** : une requête qui modifie quelque chose (POST, PUT, PATCH, DELETE) ou ouvre le
//!   WebSocket, venue d'une autre page que Heim lui-même, est refusée. Le cookie de session est
//!   déjà `SameSite=Lax`, mais un autre service sur la même machine (Home Assistant sur :8123, par
//!   exemple) est « même site » pour le navigateur : sans ce contrôle, une faille chez lui
//!   pourrait commander Heim. Les scripts (curl, sans en-tête Origin) passent normalement.
//! - **En-têtes** : pas d'affichage dans un cadre (clickjacking), pas de devinette de type,
//!   aucune adresse transmise aux autres sites, et une politique de contenu stricte : le seul
//!   script autorisé est celui de l'interface, reconnu par son empreinte SHA-256.

use std::sync::LazyLock;

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use sha2::{Digest, Sha256};

/// Empreintes des scripts intégrés à la page (`<script>…</script>`), pour la CSP.
pub fn script_hashes(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let body_start = open_end + 1;
        let Some(close) = after[body_start..].find("</script>") else {
            break;
        };
        let body = &after[body_start..body_start + close];
        if !body.is_empty() {
            out.push(format!(
                "'sha256-{}'",
                B64.encode(Sha256::digest(body.as_bytes()))
            ));
        }
        rest = &after[body_start + close..];
    }
    out
}

pub fn csp(html: &str) -> String {
    let scripts = script_hashes(html).join(" ");
    [
        "default-src 'self'".to_string(),
        format!("script-src 'self' {scripts}"),
        // Svelte pose des styles en ligne (couleurs des boutons, positions) : sans risque de script.
        "style-src 'self' 'unsafe-inline'".into(),
        "img-src 'self' data: blob:".into(),
        "font-src 'self' data:".into(),
        "connect-src 'self' ws: wss:".into(),
        "manifest-src 'self'".into(),
        "worker-src 'self'".into(),
        "object-src 'none'".into(),
        "base-uri 'none'".into(),
        "form-action 'self'".into(),
        "frame-ancestors 'none'".into(),
    ]
    .join("; ")
}

static CSP: LazyLock<HeaderValue> = LazyLock::new(|| {
    HeaderValue::from_str(&csp(crate::api::INDEX_HTML))
        .unwrap_or_else(|_| HeaderValue::from_static("frame-ancestors 'none'"))
});

/// L'hôte (avec port) d'une adresse d'origine : « http://heim.local:8090 » → « heim.local:8090 ».
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    Some(rest.split('/').next().unwrap_or(rest))
}

/// Ports implicites : « heim.local » et « heim.local:80 » sont le même endroit.
fn normalize(host: &str, scheme_https: bool) -> String {
    let host = host.trim().to_ascii_lowercase();
    let default = if scheme_https { ":443" } else { ":80" };
    host.strip_suffix(default)
        .map(str::to_string)
        .unwrap_or(host)
}

/// La requête vient-elle d'une page de Heim lui-même (ou d'un script sans navigateur) ?
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) else {
        // Pas d'Origin : curl, scripts, vieux navigateurs sur une requête simple. Le cookie
        // SameSite=Lax protège déjà ces cas-là côté navigateur.
        return true;
    };
    if origin == "null" {
        return false;
    }
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let https = origin.starts_with("https://");
    let Some(o) = origin_host(origin) else {
        return false;
    };
    // Derrière Tailscale (`tailscale serve`), l'en-tête Host peut rester celui d'origine.
    let forwarded = headers
        .get("x-forwarded-host")
        .and_then(|h| h.to_str().ok())
        .map(|h| normalize(h, https));
    let o = normalize(o, https);
    o == normalize(host, https) || forwarded.as_deref() == Some(o.as_str())
}

fn changes_something(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

pub async fn layer(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let guarded =
        path.starts_with("/api/") && (changes_something(req.method()) || path == "/api/ws");
    if guarded && !same_origin(req.headers()) {
        tracing::warn!(
            "requête refusée : origine étrangère {:?} pour {} {}",
            req.headers().get(header::ORIGIN),
            req.method(),
            path
        );
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({ "error": "requête venue d'un autre site : refusée" })),
        )
            .into_response();
    }
    let is_api = path.starts_with("/api/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    if is_api {
        // Les réponses de l'API (états, réglages) ne restent pas dans les caches.
        h.entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static("no-store"));
    } else {
        h.insert(header::CONTENT_SECURITY_POLICY, CSP.clone());
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(origin: Option<&str>, host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, host.parse().unwrap());
        if let Some(o) = origin {
            h.insert(header::ORIGIN, o.parse().unwrap());
        }
        h
    }

    #[test]
    fn origin_rules() {
        assert!(
            same_origin(&headers(None, "heim.local")),
            "script sans navigateur"
        );
        assert!(same_origin(&headers(
            Some("http://heim.local"),
            "heim.local"
        )));
        assert!(same_origin(&headers(
            Some("http://heim.local"),
            "heim.local:80"
        )));
        assert!(same_origin(&headers(
            Some("http://192.168.1.50:8090"),
            "192.168.1.50:8090"
        )));
        assert!(
            !same_origin(&headers(
                Some("http://192.168.1.50:8123"),
                "192.168.1.50:8090"
            )),
            "Home Assistant sur la même machine"
        );
        assert!(!same_origin(&headers(
            Some("https://evil.example"),
            "heim.local"
        )));
        assert!(!same_origin(&headers(Some("null"), "heim.local")));
        let mut ts = headers(Some("https://heim.tail1234.ts.net"), "127.0.0.1:8090");
        ts.insert("x-forwarded-host", "heim.tail1234.ts.net".parse().unwrap());
        assert!(same_origin(&ts), "Tailscale serve");
    }

    #[test]
    fn csp_hashes_the_inline_script() {
        let html = "<html><script type=\"module\">alert(1)</script></html>";
        let hashes = script_hashes(html);
        assert_eq!(hashes.len(), 1);
        // echo -n 'alert(1)' | openssl dgst -sha256 -binary | base64
        assert_eq!(
            hashes[0],
            "'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='"
        );
        let real = csp(crate::api::INDEX_HTML);
        assert!(real.contains("script-src 'self' 'sha256-"), "{real}");
        assert!(!real.contains("'unsafe-inline' ;") && !real.contains("script-src 'self' 'unsafe"));
    }
}
