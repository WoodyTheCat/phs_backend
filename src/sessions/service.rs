//! A middleware that provides [`Session`] as a request extension.
use std::{
    borrow::Cow,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::http::{Request, Response, StatusCode};
use time::OffsetDateTime;

use tower_cookies::{cookie::SameSite, Cookie, CookieManager, Cookies, Key};
use tower_layer::Layer;
use tower_service::Service;

use super::{session, Expiry, IdType, Session, SessionStore};

#[doc(hidden)]
pub trait CookieController: Clone + Send + 'static {
    fn get(&self, cookies: &Cookies, name: &str) -> Option<Cookie<'static>>;
    fn add(&self, cookies: &Cookies, cookie: Cookie<'static>);
    fn remove(&self, cookies: &Cookies, cookie: Cookie<'static>);
}

#[doc(hidden)]
#[derive(Debug, Clone)]
#[cfg(not(feature = "signed"))]
pub struct PlaintextCookie;

impl CookieController for PlaintextCookie {
    fn get(&self, cookies: &Cookies, name: &str) -> Option<Cookie<'static>> {
        cookies.get(name).map(Cookie::into_owned)
    }

    fn add(&self, cookies: &Cookies, cookie: Cookie<'static>) {
        cookies.add(cookie);
    }

    fn remove(&self, cookies: &Cookies, cookie: Cookie<'static>) {
        cookies.remove(cookie);
    }
}

#[doc(hidden)]
#[derive(Clone)]
#[cfg(feature = "signed_cookies")]
pub struct SignedCookie {
    key: Key,
}

#[cfg(feature = "signed_cookies")]
impl CookieController for SignedCookie {
    fn get(&self, cookies: &Cookies, name: &str) -> Option<Cookie<'static>> {
        cookies.signed(&self.key).get(name).map(Cookie::into_owned)
    }

    fn add(&self, cookies: &Cookies, cookie: Cookie<'static>) {
        cookies.signed(&self.key).add(cookie);
    }

    fn remove(&self, cookies: &Cookies, cookie: Cookie<'static>) {
        cookies.signed(&self.key).remove(cookie);
    }
}
//
//#[doc(hidden)]
//#[derive(Debug, Clone)]
//pub struct PrivateCookie {
//    key: Key,
//}
//
//impl CookieController for PrivateCookie {
//    fn get(&self, cookies: &Cookies, name: &str) -> Option<Cookie<'static>> {
//        cookies.private(&self.key).get(name).map(Cookie::into_owned)
//    }
//
//    fn add(&self, cookies: &Cookies, cookie: Cookie<'static>) {
//        cookies.private(&self.key).add(cookie)
//    }
//
//    fn remove(&self, cookies: &Cookies, cookie: Cookie<'static>) {
//        cookies.private(&self.key).remove(cookie)
//    }
//}

#[derive(Debug, Clone)]
pub struct SessionConfig<'a> {
    name: Cow<'a, str>,
    http_only: bool,
    same_site: SameSite,
    expiry: Expiry,
    secure: bool,
    path: Cow<'a, str>,
    domain: Option<Cow<'a, str>>,
}

impl<'a> SessionConfig<'a> {
    fn build_cookie(self, session_id: session::Id, expiry: Expiry) -> Cookie<'a> {
        let mut cookie_builder = Cookie::build((self.name, session_id.to_string()))
            .http_only(self.http_only)
            .same_site(self.same_site)
            .secure(self.secure)
            .path(self.path);

        cookie_builder = match expiry {
            Expiry::OnInactivity(duration) => cookie_builder.max_age(duration),
            Expiry::AtDateTime(datetime) => {
                cookie_builder.max_age(datetime - OffsetDateTime::now_utc())
            }
            Expiry::OnSessionEnd => cookie_builder,
        };

        if let Some(domain) = self.domain {
            cookie_builder = cookie_builder.domain(domain);
        }

        cookie_builder.build()
    }
}

impl<'a> Default for SessionConfig<'a> {
    fn default() -> Self {
        Self {
            /* See: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-name-fingerprinting */
            name: "id".into(),
            http_only: true,
            same_site: SameSite::Strict,
            expiry: Expiry::OnSessionEnd,
            secure: true,
            path: "/".into(),
            domain: None,
        }
    }
}

/// A middleware that provides [`Session`] as a request extension.
#[derive(Clone)]
pub struct SessionManager<S, C: CookieController> {
    inner: S,
    session_store: Arc<SessionStore>,
    session_config: SessionConfig<'static>,
    cookie_controller: C,
}

// impl<S> SessionManager<S> {
//     /// Create a new [`SessionManager`].
//     pub fn new(inner: S, session_store: SessionStore) -> Self {
//         Self {
//             inner,
//             session_store: Arc::new(session_store),
//             session_config: Default::default(),
//             cookie_controller: PlaintextCookie,
//         }
//     }
// }

impl<ReqBody, ResBody, S, C: CookieController> Service<Request<ReqBody>> for SessionManager<S, C>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Send + 'static,
    ResBody: Default + Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        //let span = tracing::trace_span!("Session service");

        let session_store = self.session_store.clone();
        let session_config = self.session_config.clone();
        let cookie_controller = self.cookie_controller.clone();

        // Because the inner service can panic until ready, we need to ensure we only
        // use the ready service.
        //
        // See: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(
            async move {
                let Some(cookies) = req.extensions().get::<Cookies>().cloned() else {
                    // In practice this should never happen because we wrap `CookieManager` directly
                    tracing::error!("Missing Cookies request extension");
                    return Ok(Response::default());
                };

                let session_cookie = cookie_controller.get(&cookies, &session_config.name);
                let session_id = session_cookie.as_ref().and_then(|cookie| {
                    cookie
                        .value()
                        .parse::<session::Id>()
                        .map_err(|err| {
                            tracing::warn!(
                                err = %err,
                                "Malformed session id, possible suspicious activity"
                            );
                        })
                        .ok()
                });

                let session = Session::new(session_id, session_store, session_config.expiry);

                req.extensions_mut().insert(session.clone());

                let res = inner.call(req).await?;

                let should_save = session.should_save().await;
                let empty = session.is_empty().await;

                match session_cookie {
                    Some(mut cookie) if empty || res.status() == StatusCode::UNAUTHORIZED => {
                        // Path and domain must be manually set to ensure a proper removal cookie is
                        // constructed.
                        //
                        // See: https://docs.rs/cookie/latest/cookie/struct.CookieJar.html#method.remove
                        cookie.set_path(session_config.path);
                        if let Some(domain) = session_config.domain {
                            cookie.set_domain(domain);
                        }

                        tracing::trace!("Removing empty or invalid cookie from client");

                        cookie_controller.remove(&cookies, cookie);
                    }

                    _ if should_save && !res.status().is_server_error() => {
                        if let Err(err) = if empty {
                            session.delete().await
                        } else {
                            session.save().await
                        } {
                            tracing::error!(err = %err, "Failed to save session");

                            let mut response = Response::default();
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            return Ok(response);
                        };

                        let IdType::Id(session_id) = session.id().await else {
                            tracing::error!("Missing session id");

                            let mut response = Response::default();
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            return Ok(response);
                        };

                        let expiry = session.expiry().await;
                        let session_cookie = session_config.build_cookie(session_id, expiry);
                        cookie_controller.add(&cookies, session_cookie);
                    }

                    _ => (),
                };

                Ok(res)
            }, // TODO Span here without wrapping all errors
               //.instrument(span),
        )
    }
}

/// A layer for providing [`Session`] as a request extension.
#[derive(Clone)]
pub struct SessionManagerLayer<C: CookieController> {
    session_store: Arc<SessionStore>,
    session_config: SessionConfig<'static>,
    cookie_controller: C,
}

impl<C: CookieController> SessionManagerLayer<C> {
    pub fn with_name<N: Into<Cow<'static, str>>>(mut self, name: N) -> Self {
        self.session_config.name = name.into();
        self
    }

    pub const fn with_http_only(mut self, http_only: bool) -> Self {
        self.session_config.http_only = http_only;
        self
    }

    pub const fn with_same_site(mut self, same_site: SameSite) -> Self {
        self.session_config.same_site = same_site;
        self
    }

    pub const fn with_expiry(mut self, expiry: Expiry) -> Self {
        self.session_config.expiry = expiry;
        self
    }

    pub const fn with_secure(mut self, secure: bool) -> Self {
        self.session_config.secure = secure;
        self
    }

    pub fn with_path<P: Into<Cow<'static, str>>>(mut self, path: P) -> Self {
        self.session_config.path = path.into();
        self
    }

    pub fn with_domain<D: Into<Cow<'static, str>>>(mut self, domain: D) -> Self {
        self.session_config.domain = Some(domain.into());
        self
    }
}

#[cfg(feature = "signed_cookies")]
impl SessionManagerLayer<SignedCookie> {
    /// Manages the session cookie via a signed interface.
    pub fn new_signed(
        session_store: SessionStore,
        session_config: SessionConfig<'static>,
        key: Key,
    ) -> Self {
        Self {
            session_store: Arc::new(session_store),
            session_config,
            cookie_controller: SignedCookie { key },
        }
    }
}

#[cfg(feature = "private")]
impl SessionManagerLayer<C = PrivateCookie> {
    pub fn new_private(
        session_store: SessionStore,
        session_config: SessionConfig<'static>,
        key: Key,
    ) -> SessionManagerLayer<PrivateCookie> {
        SessionManagerLayer::<PrivateCookie> {
            session_store: Arc::new(session_store),
            session_config,
            cookie_controller: PrivateCookie { key },
        }
    }
}

#[cfg(not(feature = "signed_cookies"))]
impl SessionManagerLayer<PlaintextCookie> {
    pub fn new(session_store: SessionStore, session_config: SessionConfig<'static>) -> Self {
        Self {
            session_store: Arc::new(session_store),
            session_config,
            cookie_controller: PlaintextCookie,
        }
    }
}

impl<S, C: CookieController> Layer<S> for SessionManagerLayer<C> {
    type Service = CookieManager<SessionManager<S, C>>;

    fn layer(&self, inner: S) -> Self::Service {
        let session_manager = SessionManager {
            inner,
            session_store: self.session_store.clone(),
            session_config: self.session_config.clone(),
            cookie_controller: self.cookie_controller.clone(),
        };

        CookieManager::new(session_manager)
    }
}
