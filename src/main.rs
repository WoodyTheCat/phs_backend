#![warn(
    clippy::correctness,
    clippy::perf,
    clippy::suspicious,
    clippy::complexity,
    clippy::nursery,
    clippy::pedantic
)]
#![allow(clippy::module_name_repetitions)]

use std::error::Error;

use deadpool_redis::{Config as RedisConfig, Pool as RedisPool, Runtime};

use sqlx::{postgres::PgPoolOptions, Postgres};
//use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

type DbPool = sqlx::Pool<Postgres>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initiate logging
    //tracing_subscriber::registry()
    //    .with(EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(
    //        #[cfg(debug_assertions)]
    //        |_| "trace,sqlx=info,fred=info".into(),
    //        #[cfg(not(debug_assertions))]
    //        |_| "info,sqlx=info,fred=info".into(),
    //    )))
    //    .with(
    //        #[cfg(debug_assertions)]
    //        tracing_subscriber::fmt::layer()
    //            .pretty()
    //            .with_file(true)
    //            .with_line_number(true)
    //            .with_thread_ids(true),
    //        #[cfg(not(debug_assertions))]
    //        tracing_subscriber::fmt::layer()
    //            .compact()
    //            .with_thread_ids(true),
    //    )
    //    .try_init()?;

    console_subscriber::Builder::default()
        .filter_env_var(
            std::env::var("RUST_LOG").unwrap_or_else(
                #[cfg(debug_assertions)]
                |_| "trace,sqlx=info,fred=info,tokio=trace,runtime=trace".into(),
                #[cfg(not(debug_assertions))]
                |_| "info,sqlx=info,fred=info".into(),
            ), //.with(
               //    #[cfg(debug_assertions)]
               //    tracing_subscriber::fmt::layer()
               //        .pretty()
               //        .with_file(true)
               //        .with_line_number(true)
               //        .with_thread_ids(true),
               //    #[cfg(not(debug_assertions))]
               //    tracing_subscriber::fmt::layer()
               //        .compact()
               //        .with_thread_ids(true),
        )
        .server_addr(([127, 0, 0, 1], 5555))
        .init();

    let db = init_db().await?;
    let redis_pool = init_redis()?;

    // Create a folder for the dynamic page data
    if !tokio::fs::try_exists("./_pages").await? {
        tokio::fs::create_dir("./_pages").await?;
    }

    phs_backend::serve(db, redis_pool).await?;

    Ok(())
}

fn init_redis() -> Result<RedisPool, Box<dyn Error>> {
    let redis_cfg =
        RedisConfig::from_url(dotenv::var("REDIS_URL").map_err(|_| "REDIS_URL not set")?);

    Ok(redis_cfg.create_pool(Some(Runtime::Tokio1))?)
}

async fn init_db() -> Result<DbPool, Box<dyn Error>> {
    let database_url = dotenv::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set")?;

    // Create a db connpool and run unapplied migrations
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .map_err(|_| "Failed to connect to DATABASE_URL")?;
    sqlx::migrate!().run(&db).await?;

    Ok(db)
}
