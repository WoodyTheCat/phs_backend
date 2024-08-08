#![warn(clippy::correctness, clippy::perf, clippy::suspicious)]
#![forbid(unsafe_code)]

use auth::AuthManagerLayer;
use axum::{Extension, Router};
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::error::Error;
use time::Duration;
use tower_http::cors::CorsLayer;

use sessions::{Expiry, SessionManagerLayer, SessionStore};

mod auth;
mod error;
mod resources;
mod sessions;

pub fn app(db: PgPool, redis_pool: RedisPool) -> Router {
    let session_store = SessionStore::new(redis_pool.clone());
    let session_manager_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)
        .with_expiry(Expiry::OnInactivity(Duration::hours(2)));

    let auth_layer = AuthManagerLayer::new(session_manager_layer);

    Router::new()
        .merge(resources::router())
        .merge(auth::router())
        .layer(Extension(db))
        .layer(Extension(redis_pool))
        .layer(auth_layer)
        .layer(CorsLayer::permissive()) // TODO WARN: Change with a domain
}

pub async fn serve(db: PgPool, redis: RedisPool) -> Result<(), Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000")
        .await
        .map_err(|_| "Listening on port 5000 failed. Is this port in use?")?;

    let app = app(db, redis).into_make_service();
    axum::serve(listener, app)
        .await
        .map_err(|_| "Server IO error".into())
}
