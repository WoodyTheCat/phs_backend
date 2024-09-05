use crate::sessions::{CookieController, Session, SessionManager, SessionManagerLayer};
use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::{
    error::Error,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_cookies::CookieManager;
use tower_layer::Layer;
use tower_service::Service;

use crate::error::PhsError;

use super::AuthSession;

/// A middleware that provides [`AuthSession`] as a request extension.
#[derive(Clone)]
pub struct AuthManager<S> {
    inner: S,
}

impl<S> AuthManager<S> {
    pub const fn new(inner: S) -> Self {
        Self { inner }
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
        //let span = tracing::info_span!("auth service");

        // let _backend = self.backend.clone();

        // Because the inner service can panic until ready, we need to ensure we only
        // use the ready service.
        //
        // See: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(
            async move {
                let Some(session) = req.extensions().get::<Session>().cloned() else {
                    return Ok(PhsError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        None,
                        "Session not found in request extensions",
                    )
                    .into_response());
                };

                match AuthSession::from_session(session).await {
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
            }, // TODO Span here without wrapping all errors
               //.instrument(span),
        )
    }
}

#[derive(Clone)]
pub struct AuthManagerLayer<C: CookieController> {
    session_manager_layer: SessionManagerLayer<C>,
}

impl<C: CookieController> AuthManagerLayer<C> {
    pub(crate) const fn new(session_manager_layer: SessionManagerLayer<C>) -> Self {
        Self {
            session_manager_layer,
        }
    }
}

impl<S, C: CookieController> Layer<S> for AuthManagerLayer<C> {
    type Service = CookieManager<SessionManager<AuthManager<S>, C>>;

    fn layer(&self, inner: S) -> Self::Service {
        self.session_manager_layer
            .layer(AuthManager::<_>::new(inner))
    }
}
