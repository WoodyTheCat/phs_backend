use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use tower_sessions::{session::SessionData, Expiry};

use axum::async_trait;
use fred::{
    prelude::KeysInterface,
    types::{Expiration, SetOptions},
};
use time::OffsetDateTime;
use tower_sessions_core::{session::Id, session_store, SessionStore};

#[derive(Debug)]
pub enum RedisStoreError {
    Redis(fred::error::RedisError),
    Decode(rmp_serde::decode::Error),
    Encode(rmp_serde::encode::Error),
}

impl From<RedisStoreError> for session_store::Error {
    fn from(err: RedisStoreError) -> Self {
        match err {
            RedisStoreError::Redis(inner) => session_store::Error::Backend(inner.to_string()),
            RedisStoreError::Decode(inner) => session_store::Error::Decode(inner.to_string()),
            RedisStoreError::Encode(inner) => session_store::Error::Encode(inner.to_string()),
        }
    }
}

/// A Redis session store.
#[derive(Debug, Clone)]
pub struct RedisStore<C: KeysInterface + Send + Sync> {
    client: C,
    csprng: Arc<Mutex<ChaCha20Rng>>,
}

impl<C: KeysInterface + Send + Sync> RedisStore<C> {
    pub fn new(client: C) -> Self {
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
        options: Option<SetOptions>,
    ) -> session_store::Result<bool> {
        let expire = Some(Expiration::EXAT(OffsetDateTime::unix_timestamp(
            data.expiry_date(),
        )));

        let t = id.to_string();

        tracing::debug!("Id before hashing: {}", t);

        let hashed_id = hex::encode(Sha256::digest(t));

        tracing::debug!("Id after hashing: {}", hashed_id);

        Ok(self
            .client
            .set(
                hashed_id,
                rmp_serde::to_vec(&SessionStoreData::from(data))
                    .map_err(RedisStoreError::Encode)?
                    .as_slice(),
                expire,
                options,
                false,
            )
            .await
            .map_err(RedisStoreError::Redis)?)
    }
}

#[derive(Serialize, Deserialize)]
struct SessionStoreData {
    data: HashMap<String, Value>,
    expiry: Expiry,
}

impl From<&SessionData> for SessionStoreData {
    fn from(session_data: &SessionData) -> Self {
        Self {
            data: session_data.data(),
            expiry: session_data.expiry(),
        }
    }
}

#[async_trait]
impl<C> SessionStore for RedisStore<C>
where
    C: KeysInterface + Send + Sync + Debug + 'static,
{
    async fn create(&self, data: &SessionData) -> session_store::Result<Id> {
        let mut id = self.new_id().await?;
        loop {
            if !self
                .save_with_options(&id, data, Some(SetOptions::NX))
                .await?
            {
                id = self.new_id().await?;
                continue;
            }
            break;
        }

        Ok(id)
    }

    async fn save(&self, id: &Id, data: &SessionData) -> session_store::Result<()> {
        self.save_with_options(id, data, Some(SetOptions::XX))
            .await?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<SessionData>> {
        let hashed_id = hex::encode(Sha256::digest(session_id.to_string()));

        let data = self
            .client
            .get::<Option<Vec<u8>>, _>(hashed_id)
            .await
            .map_err(RedisStoreError::Redis)?;

        if let Some(data) = data {
            let value: SessionStoreData =
                rmp_serde::from_slice(&data).map_err(RedisStoreError::Decode)?;

            Ok(Some(SessionData::new(
                *session_id,
                value.data,
                value.expiry,
            )))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let hashed_id = hex::encode(Sha256::digest(session_id.to_string()));

        self.client
            .del(hashed_id)
            .await
            .map_err(RedisStoreError::Redis)?;
        Ok(())
    }

    async fn new_id(&self) -> session_store::Result<Id> {
        let mut slice = [0_u8; 16];
        self.csprng.lock().await.try_fill_bytes(&mut slice)?;
        Ok(Id(i128::from_le_bytes(slice)))
    }
}
