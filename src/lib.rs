#![warn(
    clippy::correctness,
    clippy::perf,
    clippy::suspicious,
    clippy::complexity,
    clippy::nursery,
    clippy::pedantic
)]
#![allow(clippy::module_name_repetitions)]
#![forbid(unsafe_code)]

use auth::AuthManagerLayer;
use axum::{Extension, Router};
use axum_server::tls_rustls::RustlsConfig;
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{error::Error, net::SocketAddr, path::PathBuf, sync::Arc};
use tera::Tera;
use time::Duration;
use tokio::sync::Mutex;
use tower_cookies::Key;
use tower_http::{cors::CorsLayer, services::ServeDir};

use sessions::{Expiry, SessionConfig, SessionManagerLayer, SessionStore};

#[macro_use]
extern crate slugify;

mod auth;
mod error;
mod resources;
mod serve;
mod sessions;

#[allow(clippy::missing_panics_doc)]
pub fn app(db: PgPool, redis_pool: RedisPool, tera: Arc<Mutex<Tera>>) -> Router {
    let session_store = SessionStore::new(redis_pool.clone());
    #[cfg(feature = "signed_cookies")]
    let session_manager_layer = SessionManagerLayer::new_signed(
        session_store,
        SessionConfig::default(),
        Key::try_generate().expect("OS RNG"),
    )
    .with_secure(true)
    .with_expiry(Expiry::OnInactivity(Duration::hours(2)));

    #[cfg(not(feature = "signed_cookies"))]
    let session_manager_layer = SessionManagerLayer::new(session_store, SessionConfig::default())
        .with_secure(true)
        .with_expiry(Expiry::OnInactivity(Duration::hours(2)));

    let auth_layer = AuthManagerLayer::new(session_manager_layer);

    Router::new()
        // Routers
        .merge(resources::router())
        .merge(auth::router())
        .merge(serve::router())
        .route_service("/*page", ServeDir::new("pages/dist/"))
        // Layers
        .layer(Extension(db))
        .layer(Extension(redis_pool))
        .layer(Extension(tera))
        .layer(auth_layer)
        .layer(CorsLayer::permissive()) // TODO WARN: Restrict for prod build
}

#[allow(clippy::missing_panics_doc)]
pub async fn serve(
    db: PgPool,
    redis_pool: RedisPool,
    tera: Arc<Mutex<Tera>>,
) -> Result<(), Box<dyn Error>> {
    let app = app(db, redis_pool, tera).into_make_service();

    #[cfg(feature = "ssl")]
    {
        let certificate_path = dotenv::var("SSL_CERT").expect("CERT_PATH must be set");
        let key_path = std::env::var("SSL_KEY").expect("KEY_PATH must be set");

        // TODO combine HTTP and HTTPS servers
        let https_port: u16 = dotenv::var("HTTPS_PORT")
            .expect("HTTPS_PORT must be set")
            .parse()
            .expect("HTTPS port should be a number");
        let http_port: u16 = dotenv::var("HTTP_PORT")
            .expect("HTTP_PORT must be set")
            .parse()
            .expect("HTTP port should be a number");

        let config =
            RustlsConfig::from_pem_file(PathBuf::from(certificate_path), PathBuf::from(key_path))
                .await
                .expect("Failed to load certificate");

        let addr = SocketAddr::from(([0, 0, 0, 0], https_port));

        axum_server::bind_rustls(addr, config).serve(app).await?;
    }

    #[cfg(not(feature = "ssl"))]
    {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:5000")
            .await
            .map_err(|_| "Listening on port 5000 failed. Is this port in use?")?;

        axum::serve(listener, app).await?
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PaginationOptions {
    pub page: i32,
    pub page_size: i32,
}

pub type TeraState = Arc<Mutex<Tera>>;
