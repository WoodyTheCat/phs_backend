#![warn(
    clippy::correctness,
    clippy::perf,
    clippy::suspicious,
    clippy::complexity,
    clippy::nursery,
    clippy::pedantic
)]
#![allow(clippy::module_name_repetitions)]

use std::{error::Error, sync::Arc};

use deadpool_redis::{Config as RedisConfig, Pool as RedisPool, Runtime};
use phs_backend::{ServerConfig, ServerSettings};
use sqlx::{postgres::PgPoolOptions, Postgres};
use tera::Tera;
use tokio::sync::Mutex;
use tokio::{fs, sync::RwLock};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

type DbPool = sqlx::Pool<Postgres>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (server_settings_value, server_config) = get_configs().await;

    let server_settings = Arc::new(RwLock::new(server_settings_value));
    init_logging(&server_config, server_settings.clone()).await?;

    init_file_layout().await?;

    let db_pool = init_db().await?;
    let redis_pool = init_redis()?;

    let tera = Arc::new(Mutex::new(Tera::new("pages/templates/**/*")?));

    if server_config.tls_enabled {
        phs_backend::serve(db_pool, redis_pool, tera, &server_config, server_settings).await?;
    } else {
        phs_backend::serve_http(db_pool, redis_pool, tera, &server_config, server_settings).await?;
    }

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

async fn init_file_layout() -> Result<(), Box<dyn Error>> {
    // Create folders for the dynamic page data
    if !fs::try_exists("./pages/fragments").await? {
        fs::create_dir_all("./pages/fragments").await?;
    }
    if !fs::try_exists("./pages/dist").await? {
        fs::create_dir_all("./pages/dist").await?;
    }
    if !fs::try_exists("./pages/specs").await? {
        fs::create_dir_all("./pages/specs").await?;
    }

    Ok(())
}

async fn init_logging(
    config: &ServerConfig,
    settings: Arc<RwLock<ServerSettings>>,
) -> Result<(), Box<dyn Error>> {
    #[cfg(debug_assertions)]
    let use_console = config.use_tokio_console;
    #[cfg(not(debug_assertions))]
    let use_console = false;

    if use_console {
        console_subscriber::Builder::default()
            .filter_env_var("trace,sqlx=info,fred=info,tokio=trace,runtime=trace")
            .server_addr(([127, 0, 0, 1], 5555))
            .init();
        tracing::info!("Using Tokio debug console");
    } else {
        tracing_subscriber::registry()
            .with(EnvFilter::new("trace,sqlx=info,fred=info"))
            .with(
                #[cfg(debug_assertions)]
                tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_file(true)
                    .with_line_number(true)
                    .with_thread_ids(true),
                #[cfg(not(debug_assertions))]
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_thread_ids(true),
            )
            .try_init()?;
        tracing::info!("Logging to stdout");
    }

    Ok(())
}

async fn get_configs() -> (ServerSettings, ServerConfig) {
    (
        ServerSettings {},
        ServerConfig {
            http_port: 5000,
            https_port: 5001,
            tls_enabled: false,
            tls_options: None,
            #[cfg(debug_assertions)]
            use_tokio_console: false,
        },
    )
}
