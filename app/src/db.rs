use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Postgres, Transaction};

pub async fn build_pool(database_url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    // Honour whatever sslmode the URL asks for (disable locally, require in prod)
    // instead of forcing Require, which broke plaintext local/in-cluster connections.
    let options: PgConnectOptions = database_url.parse()?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Set the tenant GUC for the current transaction. Every RLS policy keys off
/// `app.hospital_id`, so this must run before any tenant-scoped statement.
pub async fn set_tenant<'c>(
    tx: &mut Transaction<'c, Postgres>,
    hospital_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT set_config('app.hospital_id', $1, true), set_config('app.staff_id', $2, true)",
    )
    .bind(hospital_id)
    .bind("")
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Begin a transaction that is already scoped to the caller's hospital. Reads
/// use this exactly like writes do, so RLS can never see across tenants.
pub async fn begin_as_tenant(
    pool: &PgPool,
    hospital_id: &str,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    set_tenant(&mut tx, hospital_id).await?;
    Ok(tx)
}

/// Like [`begin_as_tenant`] but also records the acting staff id, so the audit
/// trigger can attribute writes. Use for mutating requests.
pub async fn begin_as_tenant_staff(
    pool: &PgPool,
    hospital_id: &str,
    staff_id: &str,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT set_config('app.hospital_id', $1, true), set_config('app.staff_id', $2, true)",
    )
    .bind(hospital_id)
    .bind(staff_id)
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}
