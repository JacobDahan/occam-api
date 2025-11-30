use sqlx::{postgres::PgPoolOptions, PgPool};

/// Creates a PostgreSQL connection pool
///
/// Establishes a pool of database connections for efficient reuse.
/// The pool automatically manages connection lifecycle and limits.
pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    Ok(pool)
}
