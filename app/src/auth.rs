use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub hospital: String,
    pub name: String,
    pub roles: Vec<String>,
    pub exp: i64,
}

pub struct Authenticated {
    pub claims: Claims,
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed =
        PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("invalid password hash: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn encode_token(
    secret: &str,
    sub: &str,
    hospital: &str,
    name: &str,
    roles: Vec<String>,
    ttl_seconds: i64,
) -> anyhow::Result<String> {
    let exp = (Utc::now() + Duration::seconds(ttl_seconds)).timestamp();
    let claims = Claims {
        sub: sub.to_string(),
        hospital: hospital.to_string(),
        name: name.to_string(),
        roles,
        exp,
    };
    Ok(encode(&Header::default(), &claims, &encoding_key(secret)?)?)
}

pub fn decode_token(secret: &str, token: &str) -> anyhow::Result<Claims> {
    let decoding = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(decoding.claims)
}

fn encoding_key(secret: &str) -> anyhow::Result<EncodingKey> {
    Ok(EncodingKey::from_secret(secret.as_bytes()))
}
