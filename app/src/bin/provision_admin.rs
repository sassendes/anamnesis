use anyhow::Context;
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::password_hash::{PasswordHash, PasswordHasher};
use argon2::Argon2;
use sqlx::postgres::PgPoolOptions;
use std::env;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let hospital = env::var("HOSPITAL_ID").context("HOSPITAL_ID is required")?;
    let username = env::var("USERNAME").context("USERNAME is required")?;
    let password = env::var("PASSWORD").context("PASSWORD is required")?;
    let name = env::var("NAME").unwrap_or_else(|_| username.clone());
    let role_title = env::var("ROLE_TITLE").unwrap_or_else(|_| "System Administrator".into());
    let roles =
        env::var("ROLES").unwrap_or_else(|_| "sysadmin,admin,doctor,nurse,pharmacist".into());

    database_url.parse::<sqlx::postgres::PgConnectOptions>()?;
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?
        .to_string();
    let _ = PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("invalid hash: {e}"))?;

    let hospital_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM hospitals WHERE code = $1 OR id::text = $1")
            .bind(&hospital)
            .fetch_optional(&pool)
            .await?
            .unwrap_or_else(|| {
                Uuid::parse_str(&hospital).unwrap_or_else(|_| {
                    panic!("hospital neither found by code/id nor a valid UUID: {hospital:?}")
                })
            });

    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO staff (hospital_id, username, name, role_title, roles, active, password_hash)
        VALUES ($1, $2, $3, $4, $5::text[], true, $6)
        ON CONFLICT (hospital_id, username) DO UPDATE
        SET password_hash = EXCLUDED.password_hash, name = EXCLUDED.name,
            role_title = EXCLUDED.role_title, roles = EXCLUDED.roles,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(hospital_id)
    .bind(&username)
    .bind(&name)
    .bind(&role_title)
    .bind(&roles)
    .bind(&hash)
    .fetch_one(&pool)
    .await?;

    println!("provisioned staff {username}:{id} for hospital {hospital_id}");
    Ok(())
}
