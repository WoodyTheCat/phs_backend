#![warn(clippy::correctness, clippy::perf, clippy::suspicious)]

use std::error::Error;

use fred::{
    prelude::{ClientLike, RedisPool},
    types::RedisConfig,
};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initiate logging
    tracing_subscriber::registry()
        .with(EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(
            |_| "trace,tower_sessions=trace,sqlx=warn,tower_http=debug,fred=info".into(),
        )))
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    let database_url = dotenv::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set")?;

    // Create a db connpool and run unapplied migrations
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .map_err(|_| "Failed to connect to DATABASE_URL")?;
    sqlx::migrate!().run(&db).await?;

    // let mut cfg = Config::from_url(dotenv::var("REDIS_URL").map_err(|_| "REDIS_URL not set")?);

    // Create a Redis connpool
    let redis_pool = RedisPool::new(RedisConfig::default(), None, None, None, 6)?;
    let redis_conn = redis_pool.connect();
    redis_pool.wait_for_connect().await?;

    phs_backend::serve(db, redis_pool).await?;

    redis_conn.await??;
    Ok(())
}
