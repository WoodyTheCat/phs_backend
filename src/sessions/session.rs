use std::{
    fmt::{self, Debug, Display},
    hash::Hash,
    str::{self, FromStr},
    sync::Arc,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, DecodeError, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;

use crate::auth::AuthUser;

use super::{Error, Result, SessionStore};

#[derive(Clone)]
pub struct Session {
    store: Arc<SessionStore>,
    session_data: Arc<Mutex<SessionData>>,
}

#[derive(Clone, Copy, Default)]
#[non_exhaustive]
pub enum IdType {
    #[default]
    None,
    Unloaded(Id),
    Id(Id),
}

impl Debug for IdType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::None => "None",
                Self::Unloaded(_) => "Unloaded([redacted])",
                Self::Id(_) => "Id([redacted])",
            }
        )
    }
}

#[derive(Clone)]
pub struct SessionData {
    id: IdType,
    data: Option<AuthUser>,
    expiry: Expiry,
    should_save: bool,
}

impl SessionData {
    pub fn data(&self) -> Option<AuthUser> {
        self.data.clone()
    }

    pub const fn expiry(&self) -> Expiry {
        self.expiry
    }
}

impl SessionData {
    pub const fn new(id: Id, data: Option<AuthUser>, expiry: Expiry) -> Self {
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
    /// WARN: THIS METHOD IS LAZY and does not invoke the overhead of talking to the
    /// backing store.
    pub fn new(session_id: Option<Id>, store: Arc<SessionStore>, expiry: Expiry) -> Self {
        Self {
            session_data: Arc::new(Mutex::new(SessionData {
                id: session_id.map_or(IdType::None, IdType::Unloaded), // ERROR: here?
                data: None,
                expiry,
                should_save: false,
            })),
            store,
        }
    }

    pub async fn get_hashed_id(&self) -> Option<String> {
        match self.id().await {
            IdType::None => None,
            IdType::Id(id) | IdType::Unloaded(id) => {
                Some(hex::encode(Sha256::digest(id.to_string())))
            }
        }
    }

    #[tracing::instrument(skip(self), err)]
    async fn maybe_load(&self) -> Result<()> {
        // If the lazy load has been completed, early return
        let IdType::Unloaded(id) = self.session_data.lock().await.id else {
            return Ok(());
        };

        let session_data = &mut *self.session_data.lock().await;

        tracing::trace!("Record not loaded from store, loading...");

        *session_data = if let Some(loaded_record) = self.store.load(&id).await? {
            loaded_record
        } else {
            // Reaching this point indicates that the browser sent an expired cookie,
            // it expired whilst in transit, or possible suspicious activity
            tracing::warn!(
                    "No session found. Was an expired cookie received, or is the store offline? Possible suspicious actvity"
                );
            let new_id = self.store.create(session_data).await?;

            SessionData {
                id: IdType::Id(new_id),
                data: None,
                expiry: session_data.expiry,
                should_save: false,
            }
        };

        Ok(())
    }
    /*
        /// Inserts a `impl Serialize` value into the session.
        pub async fn insert(&self, key: &str, value: impl Serialize) -> Result<()> {
            self.insert_value(key, serde_json::to_value(&value)?)
                .await?;
            Ok(())
        }
    */
    /// Inserts a `serde_json::Value` into the session.
    ///
    /// If the key was not present in the underlying map, `None` is returned and
    /// `modified` is set to `true`.
    ///
    /// If the underlying map did have the key and its value is the same as the
    /// provided value, `None` is returned and `modified` is not set.
    pub async fn set(&self, value: AuthUser) -> Result<()> {
        self.maybe_load().await?;

        let session_data = &mut *self.session_data.lock().await;

        session_data.should_save = true;
        session_data.data = Some(value);

        Ok(())
    }

    /// Gets an [`AuthUser`] from the store.
    pub async fn get(&self) -> Result<Option<AuthUser>> {
        self.maybe_load().await?;

        let session_data = &*self.session_data.lock().await;

        Ok(session_data.data.clone())
    }

    /*
    /// Removes a value from the store, retuning the value of the key if it was
    /// present in the underlying map.
    pub async fn remove<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self
            .remove_value(key)
            .await?
            .map(serde_json::from_value)
            .transpose()?)
    }
    */

    /// Removes a `serde_json::Value` from the session.
    pub async fn remove_value(&self) -> Result<()> {
        let session_data = &mut *self.session_data.lock().await;

        if session_data.data.take().is_some() {
            session_data.should_save = true;
        }

        Ok(())
    }

    /// Clears the session of all data but does not delete it from the store.
    pub async fn clear(&self) {
        let session_data = &mut *self.session_data.lock().await;

        session_data.data = None;
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

        !has_session_id && session_data.data.is_none()
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

        let (IdType::Unloaded(session_id) | IdType::Id(session_id)) = id else {
            tracing::error!({ ?id } , "Called `Session::delete` with an IdType other than Id");
            return Ok(());
        };

        tracing::trace!("Deleting session");

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
            data: None,
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

        // Self::save also obtains a mutex lock, so we have to drop it to avoid deadlocks
        {
            let session_data = &mut self.session_data.lock().await;

            let IdType::Id(old_id) = std::mem::replace(&mut session_data.id, IdType::None) else {
                return Ok(());
            };

            self.store.delete(&old_id).await.map_err(Error::Store)?;

            session_data.should_save = true;
        }

        self.save().await?;

        Ok(())
    }
}

/// ID type for sessions. Session stores should implement generating new session IDs securely
#[derive(Copy, Clone, Deserialize, Serialize, Eq, Hash, PartialEq)]
pub struct Id(i128);

impl Id {
    pub fn hashed_id(&self) -> String {
        hex::encode(Sha256::digest(self.to_string()))
    }

    pub const fn new(id: i128) -> Self {
        Self(id)
    }
}

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

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
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
    /// Expire on current session end, as defined by the browser.
    OnSessionEnd,

    /// Expire on inactivity.
    /// [`Session`] expiration is computed from the last time the session was
    /// _modified_.
    OnInactivity(Duration),

    /// Expire at a specific date and time.
    ///
    /// This value may be extended manually with
    /// [`set_expiry`](Session::set_expiry).
    AtDateTime(OffsetDateTime),
}

const DEFAULT_DURATION: Duration = Duration::weeks(2);

impl Expiry {
    /// Get session expiry as `OffsetDateTime`.
    pub fn expiry_date(&self) -> OffsetDateTime {
        match self {
            Self::OnInactivity(duration) => OffsetDateTime::now_utc().saturating_add(*duration),
            Self::AtDateTime(datetime) => *datetime,
            Self::OnSessionEnd => OffsetDateTime::now_utc().saturating_add(DEFAULT_DURATION),
        }
    }
}
