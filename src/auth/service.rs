use axum::{
    extract::Request,
    response::{IntoResponse, Response},
};
use std::{
    error::Error,
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_cookies::CookieManager;
use tower_layer::Layer;
use tower_service::Service;
use tower_sessions::{
    service::{CookieController, PlaintextCookie},
    Session, SessionManager, SessionManagerLayer, SessionStore,
};
use tracing::Instrument;

use crate::error::PhsError;

use super::AuthSession;

/// A middleware that provides [`AuthSession`] as a request extension.
#[derive(Debug, Clone)]
pub struct AuthManager<S> {
    inner: S,
    data_key: &'static str,
}

impl<S> AuthManager<S> {
    pub fn new(inner: S, data_key: &'static str) -> Self {
        Self { inner, data_key }
    }
}

impl<S> Service<Request> for AuthManager<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Error,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let span = tracing::info_span!("auth service");

        // let _backend = self.backend.clone();
        let data_key = self.data_key;

        // Because the inner service can panic until ready, we need to ensure we only
        // use the ready service.
        //
        // See: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(
            async move {
                let Some(session) = req.extensions().get::<Session>().cloned() else {
                    tracing::error!("`Session` not found in request extensions");
                    return Ok(PhsError::INTERNAL.into_response());
                };

                match AuthSession::from_session(session, data_key).await {
                    Ok(Some(auth_session)) => {
                        req.extensions_mut().insert(auth_session);
                    }
                    Err(e) => {
                        tracing::error!(?e, "Error when converting Session to AuthSession");
                        return Ok(e.into_response());
                    }
                    _ => {}
                }

                inner.call(req).await
            }
            .instrument(span),
        )
    }
}

#[derive(Debug, Clone)]
pub struct AuthManagerLayer<Sessions: SessionStore, C: CookieController = PlaintextCookie> {
    session_manager_layer: SessionManagerLayer<Sessions, C>,
    data_key: &'static str,
}

impl<Sessions: SessionStore, C: CookieController> AuthManagerLayer<Sessions, C> {
    pub(crate) fn new(
        session_manager_layer: SessionManagerLayer<Sessions, C>,
        data_key: &'static str,
    ) -> Self {
        Self {
            session_manager_layer,
            data_key,
        }
    }
}

impl<S, Sessions: SessionStore, C: CookieController> Layer<S> for AuthManagerLayer<Sessions, C> {
    type Service = CookieManager<SessionManager<AuthManager<S>, Sessions, C>>;

    fn layer(&self, inner: S) -> Self::Service {
        self.session_manager_layer
            .layer(AuthManager::new(inner, self.data_key))
    }
}
