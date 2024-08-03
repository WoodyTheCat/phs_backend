use std::{
    collections::HashMap,
    fmt::{self, Display},
    hash::Hash,
    result,
    str::{self, FromStr},
    sync::Arc,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, DecodeError, Engine as _};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;

use crate::{session_store, SessionStore};

const DEFAULT_DURATION: Duration = Duration::weeks(2);

type Result<T> = result::Result<T, Error>;

/// Session errors.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Maps `serde_json` errors.
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    /// Maps `session_store::Error` errors.
    #[error(transparent)]
    Store(#[from] session_store::Error),
}

/// A session which allows HTTP applications to associate key-value pairs with
/// visitors.
#[derive(Debug, Clone)]
pub struct Session {
    store: Arc<dyn SessionStore>,

    // This will be `None` when:
    //
    // 1. We have not been provided a session cookie or have failed to parse it,
    // 2. The store has not found the session.
    //
    // Sync lock, see: https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html#which-kind-of-mutex-should-you-use
    session_data: Arc<Mutex<SessionData>>,
}

#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub enum IdType {
    #[default]
    None,
    Unloaded(Id),
    Id(Id),
}

impl Display for IdType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
pub struct SessionData {
    id: IdType,
    data: HashMap<String, Value>,
    expiry: Expiry,
    should_save: bool,
}

impl SessionData {
    pub fn id(&self) -> &IdType {
        &self.id
    }

    pub fn expiry_date(&self) -> OffsetDateTime {
        self.expiry.expiry_date()
    }

    pub fn data(&self) -> HashMap<String, Value> {
        self.data.clone()
    }

    pub fn expiry(&self) -> Expiry {
        self.expiry
    }
}

impl SessionData {
    pub fn new(id: Id, data: HashMap<String, Value>, expiry: Expiry) -> Self {
        Self {
            id: IdType::Unloaded(id),
            data,
            expiry,
            should_save: false,
        }
    }
}

impl Session {
    /// Creates a new session with the session ID, store, and expiry.
    ///
    /// This method is lazy and does not invoke the overhead of talking to the
    /// backing store.
    pub fn new(session_id: Option<Id>, store: Arc<impl SessionStore>, expiry: Expiry) -> Self {
        // let Some(id) = session_id else {
        //     return Ok(Self {
        //         session_data: Arc::new(Mutex::new(SessionData::new(expiry))),
        //         store,
        //     });
        // };

        // let Some(session_data) = store.load(&id).await.map_err(Error::Store)? else {
        //     return Ok(Self {
        //         session_data: Arc::new(Mutex::new(SessionData::new(expiry))),
        //         store,
        //     });
        // };

        Self {
            session_data: Arc::new(Mutex::new(SessionData {
                id: match session_id {
                    Some(id) => IdType::Unloaded(id),
                    None => IdType::None,
                },
                data: HashMap::default(),
                expiry,
                should_save: false,
            })),
            store,
        }
    }

    /// WARN: Remove
    pub async fn get_session_data(&self) -> SessionData {
        self.session_data.lock().await.clone()
    }

    #[tracing::instrument(skip(self), err)]
    async fn maybe_load(&self) -> Result<()> {
        tracing::trace!("In maybe_load");
        // If the lazy load has been completed, early return
        let IdType::Unloaded(id) = self.session_data.lock().await.id else {
            return Ok(());
        };

        let session_data = &mut *self.session_data.lock().await;

        tracing::trace!("Record not loaded from store, loading...");

        *session_data = match self.store.load(&id).await? {
            Some(loaded_record) => loaded_record,
            None => {
                // Reaching this point indicates that the browser sent an expired cookie,
                // it expired whilst in transit, or possible suspicious activity
                tracing::warn!("Expired cookie received, possible suspicious actvity");
                let new_id = self.store.create(session_data).await?;

                SessionData {
                    id: IdType::Id(new_id),
                    data: HashMap::default(),
                    expiry: session_data.expiry,
                    should_save: false,
                }
            }
        };

        Ok(())
    }

    /// Inserts a `impl Serialize` value into the session.
    pub async fn insert(&self, key: &str, value: impl Serialize) -> Result<()> {
        self.insert_value(key, serde_json::to_value(&value)?)
            .await?;
        Ok(())
    }

    /// Inserts a `serde_json::Value` into the session.
    ///
    /// If the key was not present in the underlying map, `None` is returned and
    /// `modified` is set to `true`.
    ///
    /// If the underlying map did have the key and its value is the same as the
    /// provided value, `None` is returned and `modified` is not set.
    pub async fn insert_value(&self, key: &str, value: Value) -> Result<Option<Value>> {
        self.maybe_load().await?;

        let session_data = &mut *self.session_data.lock().await;

        Ok(if Some(&value) != session_data.data.get(key) {
            session_data.should_save = true;
            session_data.data.insert(key.to_string(), value)
        } else {
            None
        })
    }

    /// Gets a value from the store.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self
            .get_value(key)
            .await?
            .map(serde_json::from_value)
            .transpose()?)
    }

    /// Gets a `serde_json::Value` from the store.
    pub async fn get_value(&self, key: &str) -> Result<Option<Value>> {
        self.maybe_load().await?;

        let session_data = &*self.session_data.lock().await;

        Ok(session_data.data.get(key).cloned())
    }

    /// Removes a value from the store, retuning the value of the key if it was
    /// present in the underlying map.
    pub async fn remove<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self
            .remove_value(key)
            .await?
            .map(serde_json::from_value)
            .transpose()?)
    }

    /// Removes a `serde_json::Value` from the session.
    pub async fn remove_value(&self, key: &str) -> Result<Option<Value>> {
        let session_data = &mut *self.session_data.lock().await;

        Ok(match session_data.data.remove(key) {
            some if some.is_some() => {
                session_data.should_save = true;
                some
            }
            _ => None,
        })
    }

    /// Clears the session of all data but does not delete it from the store.
    pub async fn clear(&self) {
        let session_data = &mut *self.session_data.lock().await;

        session_data.data.clear();
        session_data.should_save = true;
    }

    /// Returns `true` if there is no session ID and the session is empty.
    pub async fn is_empty(&self) -> bool {
        let session_data = &*self.session_data.lock().await;

        // Session IDs are `None` if:
        // 1. The cookie was not provided or otherwise could not be parsed,
        // 2. Or the session could not be loaded from the store.
        // or `Cycle` if:
        // 3. It is in the process of being cycled
        let has_session_id = matches!(session_data.id, IdType::Id(..));

        !has_session_id && session_data.data.is_empty()
    }

    /// Get the session ID.
    pub async fn id(&self) -> IdType {
        self.session_data.lock().await.id
    }

    /// Get the session expiry.
    pub async fn expiry(&self) -> Expiry {
        self.session_data.lock().await.expiry
    }

    /// Set `expiry` to the given value.
    pub async fn set_expiry(&self, expiry: Expiry) {
        let session_data = &mut *self.session_data.lock().await;

        session_data.expiry = expiry;
        session_data.should_save = true;
    }

    /// Get session expiry as `Duration`.
    pub async fn expiry_age(&self) -> Option<Duration> {
        Some(std::cmp::max(
            self.session_data.lock().await.expiry.expiry_date() - OffsetDateTime::now_utc(),
            Duration::ZERO,
        ))
    }

    /// Returns `true` if the session has been modified during the request.
    pub async fn should_save(&self) -> bool {
        self.session_data.lock().await.should_save
    }

    /// Saves the session record to the store.
    ///
    /// Note that this method is generally not needed and is reserved for
    /// situations where the session store must be updated during the
    /// request.
    #[tracing::instrument(skip(self), err)]
    pub async fn save(&self) -> Result<()> {
        let session_data = &mut *self.session_data.lock().await;

        if let IdType::Id(id) = session_data.id {
            self.store.save(&id, session_data).await?;
        } else {
            let id = self.store.create(session_data).await?;
            session_data.id = IdType::Id(id);
            session_data.should_save = false;
        }

        Ok(())
    }

    /// Loads the session record from the store.
    #[tracing::instrument(skip(self), err)]
    async fn load(&self) -> Result<()> {
        let IdType::Id(session_id) = self.id().await else {
            tracing::warn!("Called load with an IdType other than Id");
            return Ok(());
        };

        match self.store.load(&session_id).await.map_err(Error::Store)? {
            Some(s) => {
                *self.session_data.lock().await = s;
            }
            None => self.flush().await?,
        }

        Ok(())
    }

    /// Deletes the session from the store.
    #[tracing::instrument(skip(self), err)]
    pub async fn delete(&self) -> Result<()> {
        let id = self.id().await;

        if !matches!(id, IdType::Id(_) | IdType::Unloaded(_)) {
            tracing::warn!({ %id } , "Called `Session::delete` with an IdType other than Id");
            return Ok(());
        };

        tracing::trace!("Deleting session");

        let session_id = match id {
            IdType::Unloaded(id) => id,
            IdType::Id(id) => id,
            _ => unreachable!(),
        };

        self.store.delete(&session_id).await.map_err(Error::Store)?;

        Ok(())
    }

    /// Flushes the session by removing all data contained in the session and
    /// then deleting it from the store.
    pub async fn flush(&self) -> Result<()> {
        let expiry = { self.session_data.lock().await.expiry };

        self.clear().await;
        self.delete().await?;

        *self.session_data.lock().await = SessionData {
            id: IdType::None,
            data: HashMap::default(),
            expiry,
            should_save: false,
        };

        Ok(())
    }

    /// Cycles the session ID while retaining any data that was associated with
    /// it.
    ///
    /// Using this method helps prevent session fixation attacks by ensuring a
    /// new ID is assigned to the session.
    pub async fn cycle_id(&self) -> Result<()> {
        // let mut record_guard = self.get_record().await?;

        // let old_session_id = record_guard.id;
        // record_guard.id = Id::default();
        // *self.inner.session_id.lock() = None; // Setting `None` ensures `save` invokes the store's
        //                                       // `create` method.

        let session_data = &mut *self.session_data.lock().await;

        let IdType::Id(old_id) = std::mem::replace(&mut session_data.id, IdType::None) else {
            return Ok(());
        };

        self.store.delete(&old_id).await.map_err(Error::Store)?;

        session_data.should_save = true;

        Ok(())
    }
}

/// ID type for sessions. Session stores should implement generating new session IDs securely
#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, Hash, PartialEq)]
pub struct Id(pub i128); // TODO: By this being public, it may be possible to override the
                         // session ID, which is undesirable.

impl Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut encoded = [0; 22];
        URL_SAFE_NO_PAD
            .encode_slice(self.0.to_le_bytes(), &mut encoded)
            .expect("Encoded ID must be exactly 22 bytes");
        let encoded = str::from_utf8(&encoded).expect("Encoded ID must be valid UTF-8");

        f.write_str(encoded)
    }
}

impl FromStr for Id {
    type Err = base64::DecodeSliceError;

    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        let mut decoded = [0; 16];
        let bytes_decoded = URL_SAFE_NO_PAD.decode_slice(s.as_bytes(), &mut decoded)?;
        if bytes_decoded != 16 {
            let err = DecodeError::InvalidLength(bytes_decoded);
            return Err(base64::DecodeSliceError::DecodeError(err));
        }

        Ok(Self(i128::from_le_bytes(decoded)))
    }
}

/// Session expiry configuration.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Expiry {
    /// Expire on [current session end][current-session-end], as defined by the
    /// browser.
    ///
    /// [current-session-end]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Cookies#define_the_lifetime_of_a_cookie
    OnSessionEnd,

    /// Expire on inactivity.
    ///
    /// Reading a session is not considered activity for expiration purposes.
    /// [`Session`] expiration is computed from the last time the session was
    /// _modified_.
    OnInactivity(Duration),

    /// Expire at a specific date and time.
    ///
    /// This value may be extended manually with
    /// [`set_expiry`](Session::set_expiry).
    AtDateTime(OffsetDateTime),
}

impl Expiry {
    /// Get session expiry as `OffsetDateTime`.
    fn expiry_date(&self) -> OffsetDateTime {
        match self {
            Expiry::OnInactivity(duration) => OffsetDateTime::now_utc().saturating_add(*duration),
            Expiry::AtDateTime(datetime) => *datetime,
            Expiry::OnSessionEnd => {
                // TODO: The default should probably be configurable.
                OffsetDateTime::now_utc().saturating_add(DEFAULT_DURATION)
            }
        }
    }
}
