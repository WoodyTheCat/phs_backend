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

#[cfg(feature = "ssl")]
use ::{
    axum_server::tls_rustls::RustlsConfig,
    std::{net::SocketAddr, path::PathBuf},
};

use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{error::Error, sync::Arc};
use tera::Tera;
use time::Duration;
use tokio::sync::Mutex;
use tower_cookies::Key;
use tower_http::{cors::CorsLayer, services::ServeDir};

use sessions::{Expiry, SessionConfig, SessionManagerLayer, SessionStore};

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
        .layer(CorsLayer::very_permissive().allow_credentials(true)) // TODO WARN: Restrict for prod build
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

#[derive(Deserialize, Debug, Serialize)]
pub struct CursorOptions {
    #[serde(default)]
    cursor: i32,
    #[serde(default = "_default_cursor_length")]
    #[serde(rename = "cursor[length]")]
    length: i32,
    #[serde(default)]
    #[serde(rename = "cursor[previous]")]
    previous: bool,
}

#[rustfmt::skip]
const fn _default_cursor_length() -> i32 { 20 }

#[derive(Deserialize, Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CursorResponse<T> {
    next_cursor: Option<i32>,
    previous_cursor: i32,
    has_next_page: bool,

    data: Vec<T>,
}

trait CursorPaginatable {
    fn id(&self) -> i32;
}

impl<T: CursorPaginatable> CursorResponse<T> {
    fn new(data: Vec<T>) -> Self {
        let (previous_cursor, next_cursor) = (
            data.first().map_or(0, |v| <T as CursorPaginatable>::id(v)),
            data.last().map(|v| <T as CursorPaginatable>::id(v)),
        );

        Self {
            next_cursor,
            previous_cursor,
            has_next_page: data.len() != 0,

            data,
        }
    }
}
