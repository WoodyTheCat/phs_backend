use deadpool_redis::Pool as RedisPool;
use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

use std::{
    fmt::{Debug, Display},
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::{
    auth::AuthUser,
    sessions::{
        session::{Id, SessionData},
        Expiry,
    },
};

#[derive(thiserror::Error, Debug)]
pub enum SessionStoreError {
    #[error("Redis client error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Redis connection pool error: {0}")]
    DeadpoolError(#[from] deadpool_redis::PoolError),
    #[error("Error whilst serialising or deserialising data: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Error whilst getting an OS random number: {0}")]
    Rand(#[from] rand_core::Error),
    #[error("Miscellaneous error: {0}")]
    Misc(String),
    #[error("Session not found")]
    NotFound,
}

/// A Redis session store.
#[derive(Clone)]
pub struct SessionStore {
    client: RedisPool,
    csprng: Arc<Mutex<ChaCha20Rng>>,
}

enum ExistenceFlag {
    NX,
    XX,
}

impl Display for ExistenceFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::NX => "NX",
                Self::XX => "XX",
            }
        )
    }
}

impl SessionStore {
    pub fn new(client: RedisPool) -> Self {
        Self {
            client,
            // New PRNG seeded from Linux `getrandom` or equivalent
            csprng: Arc::new(Mutex::new(ChaCha20Rng::from_entropy())),
        }
    }

    async fn save_with_options(
        &self,
        id: &Id,
        data: &SessionData,
        exists: ExistenceFlag,
    ) -> Result<bool, SessionStoreError> {
        let session_data = SessionStoreData::from_session_data(data).unwrap();
        let key = "sessions:".to_string() + &id.hashed_id();

        let mut conn = self.client.get().await?;

        #[rustfmt::skip]
        let (set_result, expireat_result) = redis::pipe()

            .cmd("JSON.SET")
            .arg(&key)
            .arg("$")
            .arg(serde_json::to_string(&session_data).unwrap())
            .arg(exists.to_string())

            .cmd("EXPIREAT")
            .arg(&key)
            .arg(session_data.expiry.expiry_date().unix_timestamp())

            .query_async(&mut conn).await?;

        Ok(set_result && expireat_result)
    }

    pub async fn create(&self, data: &SessionData) -> Result<Id, SessionStoreError> {
        let mut id = self.new_id().await?;
        loop {
            if !self.save_with_options(&id, data, ExistenceFlag::NX).await? {
                id = self.new_id().await?;
                continue;
            }
            break;
        }

        Ok(id)
    }

    pub async fn save(&self, id: &Id, data: &SessionData) -> Result<(), SessionStoreError> {
        self.save_with_options(id, data, ExistenceFlag::XX).await?;
        Ok(())
    }

    pub async fn load(&self, session_id: &Id) -> Result<Option<SessionData>, SessionStoreError> {
        let key = "sessions:".to_string() + &session_id.hashed_id();
        let mut conn = self.client.get().await?;

        let query = redis::cmd("JSON.GET")
            .arg(key)
            .arg("$")
            .query_async::<Option<String>>(&mut conn)
            .await?
            .ok_or(SessionStoreError::NotFound)?;

        let returned_values = serde_json::from_str::<Vec<SessionStoreData>>(&query)?;

        if returned_values.is_empty() {
            return Err(SessionStoreError::NotFound);
        }

        if returned_values.len() > 1 {
            return Err(SessionStoreError::Misc(String::from(
                "Multiple values returned from JSON.GET",
            )));
        }

        let data = returned_values.first().unwrap();

        Ok(Some(SessionData::new(
            *session_id,
            Some(data.data.clone()),
            data.expiry,
        )))
    }

    pub async fn delete(&self, session_id: &Id) -> Result<(), SessionStoreError> {
        let key = "sessions:".to_string() + &session_id.hashed_id();
        let mut conn = self.client.get().await?;

        redis::cmd("JSON.DEL")
            .arg(key)
            .arg("$")
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    async fn new_id(&self) -> Result<Id, SessionStoreError> {
        let mut slice = [0_u8; 16];
        self.csprng.lock().await.try_fill_bytes(&mut slice)?;
        Ok(Id::new(i128::from_le_bytes(slice)))
    }
}

#[derive(Serialize, Deserialize)]
struct SessionStoreData {
    #[serde(flatten)]
    data: AuthUser,

    expiry: Expiry,
}

impl SessionStoreData {
    fn from_session_data(session_data: &SessionData) -> Option<Self> {
        Some(Self {
            data: session_data.data()?,
            expiry: session_data.expiry(),
        })
    }
}
