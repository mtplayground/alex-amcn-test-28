use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Builds the shared PostgreSQL connection pool.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}
