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

use axum::{
    extract::{Host, Request},
    handler::HandlerWithoutStateExt,
    http::{StatusCode, Uri},
    response::Redirect,
    BoxError, Extension, Router, ServiceExt,
};

use ::{axum_server::tls_rustls::RustlsConfig, std::net::SocketAddr};

use tokio::sync::{Mutex, RwLock};
use tower_cookies::Key;
use tower_http::{cors::CorsLayer, normalize_path::NormalizePathLayer, services::ServeDir};
use tower_layer::Layer;

use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;

use std::{error::Error, sync::Arc};

use tera::Tera;
use time::Duration;
extern crate slugify;

mod auth;
mod config;
mod error;
mod resources;
mod serve;
mod sessions;
mod settings;

pub use {config::ServerConfig, settings::ServerSettings};

use auth::AuthManagerLayer;
use sessions::{Expiry, SessionConfig, SessionManagerLayer, SessionStore};

#[allow(clippy::missing_panics_doc)]
pub fn app(
    db: PgPool,
    redis_pool: RedisPool,
    tera: Arc<Mutex<Tera>>,
    config: &ServerConfig,
    settings: Arc<RwLock<ServerSettings>>,
) -> Router {
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
        .layer(auth_layer)
        // TODO WARN: Restrict for prod build
        .layer(CorsLayer::very_permissive().allow_credentials(true))
        .layer(Extension(db))
        .layer(Extension(redis_pool))
        .layer(Extension(tera))
        .layer(Extension(config.clone()))
        // This settings state needs to be saved to TOML on write, or with a timed batch operation
        .layer(Extension(Arc::new(RwLock::new(settings))))
}

#[allow(clippy::missing_panics_doc)]
pub async fn serve_http(
    db: PgPool,
    redis_pool: RedisPool,
    tera: Arc<Mutex<Tera>>,
    config: &ServerConfig,
    settings: Arc<RwLock<ServerSettings>>,
) -> Result<(), Box<dyn Error>> {
    let app = ServiceExt::<Request>::into_make_service(
        NormalizePathLayer::trim_trailing_slash()
            .layer(app(db, redis_pool, tera, config, settings)),
    );

    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], config.http_port)))
            .await
            .map_err(|_| {
                format!(
                    "Listening on port {} failed. Is this port in use?",
                    config.http_port
                )
            })?;

    axum::serve(listener, app).await.map_err(Into::into)
}

pub async fn serve(
    db: PgPool,
    redis_pool: RedisPool,
    tera: Arc<Mutex<Tera>>,
    config: &ServerConfig,
    settings: Arc<RwLock<ServerSettings>>,
) -> Result<(), Box<dyn Error>> {
    let app = ServiceExt::<Request>::into_make_service(
        NormalizePathLayer::trim_trailing_slash()
            .layer(app(db, redis_pool, tera, config, settings)),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], config.https_port));

    tracing::info!("Listening on port {}", addr);

    tokio::spawn(redirect_http_to_https(config.clone()));

    assert!(config.tls_enabled, "Serve called with TLS disabled");

    let Some(tls_options) = config.tls_options.as_ref() else {
        panic!("TLS is enabled but no options have been provided. Check that there is a [tls] section in config.toml")
    };

    let rustls_config =
        RustlsConfig::from_pem_file(&tls_options.cert_path, &tls_options.key_path).await?;

    axum_server::bind_rustls(addr, rustls_config)
        .serve(app)
        .await?;

    Ok(())
}

async fn redirect_http_to_https(server_config: ServerConfig) {
    fn make_https(
        host: String,
        uri: Uri,
        http_port: u16,
        https_port: u16,
    ) -> Result<Uri, BoxError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&http_port.to_string(), &https_port.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let ServerConfig {
        https_port,
        http_port,
        ..
    } = server_config;

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri, http_port, https_port) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(error) => {
                tracing::warn!(%error, "Failed to convert URI to HTTPS");
                Err(StatusCode::BAD_REQUEST)
            }
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], http_port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("Listening on port {}", listener.local_addr().unwrap());
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}
