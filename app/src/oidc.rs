use crate::errors::ApiError;
use base64::Engine;
use ring::signature::RsaPublicKeyComponents;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
struct Jwk {
    n: String,
    e: String,
}

pub struct OidcUser {
    pub username: String,
    pub display: Option<String>,
}

fn b64u(input: &str) -> Result<Vec<u8>, ApiError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|_| ApiError::unauthorized("oidc token is corrupted"))
}

fn decode_segment(input: &str) -> Result<Value, ApiError> {
    let raw = b64u(input)?;
    serde_json::from_slice(&raw).map_err(|_| ApiError::unauthorized("oidc payload invalid"))
}

async fn fetch_jwks(issuer: &str, kid: &str) -> Result<Jwk, ApiError> {
    let url = format!("{}/.well-known/jwks.json", issuer.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| ApiError::unauthorized(format!("jwks fetch failed: {e}")))?;
    let body: Value = resp
        .json()
        .await
        .map_err(|e| ApiError::unauthorized(format!("jwks parse failed: {e}")))?;
    let keys = body
        .get("keys")
        .and_then(|k| k.as_array())
        .ok_or_else(|| ApiError::unauthorized("jwks has no keys"))?;
    let wanted = keys
        .iter()
        .find(|k| k.get("kid").and_then(|v| v.as_str()) == Some(kid))
        .or_else(|| {
            keys.iter()
                .find(|k| k.get("use").is_none() && k.get("kty").map(|t| t == "RSA") == Some(true))
        })
        .ok_or_else(|| ApiError::unauthorized("no matching signing key"))?;
    let n = wanted
        .get("n")
        .and_then(|c| c.as_str())
        .ok_or_else(|| ApiError::unauthorized("missing modulus"))?
        .to_string();
    let e = wanted
        .get("e")
        .and_then(|c| c.as_str())
        .ok_or_else(|| ApiError::unauthorized("missing exponent"))?
        .to_string();
    Ok(Jwk { n, e })
}

pub async fn verify_id_token(
    issuer: &str,
    client_id: &str,
    id_token: &str,
) -> Result<OidcUser, ApiError> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err(ApiError::unauthorized("id_token is not a JWT"));
    }
    let header = decode_segment(parts[0])?;
    let payload = decode_segment(parts[1])?;
    let signature = b64u(parts[2])?;
    let kid = header
        .get("kid")
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .to_string();

    let alg = header
        .get("alg")
        .and_then(|a| a.as_str())
        .ok_or_else(|| ApiError::unauthorized("missing alg"))?;
    if !alg.eq_ignore_ascii_case("rs256") {
        return Err(ApiError::unauthorized(format!("unexpected alg {alg}")));
    }

    let jwk = fetch_jwks(issuer, &kid).await?;
    let n = b64u(&jwk.n)?;
    let e = b64u(&jwk.e)?;
    let signed = format!("{}.{}", parts[0], parts[1]);
    let key = RsaPublicKeyComponents {
        n: &n[..],
        e: &e[..],
    };
    key.verify(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        signed.as_bytes(),
        &signature,
    )
    .map_err(|_| ApiError::unauthorized("signature verification failed"))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let exp = payload
        .get("exp")
        .and_then(|e| e.as_i64())
        .ok_or_else(|| ApiError::unauthorized("missing exp"))?;
    if exp < now {
        return Err(ApiError::unauthorized("id_token expired"));
    }
    let iss = payload
        .get("iss")
        .and_then(|i| i.as_str())
        .ok_or_else(|| ApiError::unauthorized("missing iss"))?;
    if iss.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(ApiError::unauthorized("issuer mismatch"));
    }
    let aud_ok = match payload.get("aud") {
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(client_id)),
        Some(Value::String(s)) => s == client_id,
        _ => false,
    };
    if !aud_ok {
        return Err(ApiError::unauthorized("audience mismatch"));
    }

    let username = payload
        .get("email")
        .and_then(|m| m.as_str())
        .or_else(|| payload.get("sub").and_then(|s| s.as_str()))
        .ok_or_else(|| ApiError::unauthorized("no principal in token"))?
        .to_string();
    Ok(OidcUser {
        username,
        display: payload
            .get("name")
            .and_then(|n| n.as_str())
            .map(str::to_string),
    })
}
