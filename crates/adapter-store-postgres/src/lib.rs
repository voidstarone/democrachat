//! A Postgres driving-side adapter over the `app` store ports — the production
//! persistence backend behind `DATABASE_URL`. It knows nothing about the use-cases;
//! the composition root injects it as the [`Stores`] bundle in place of the
//! in-memory store.
//!
//! Every aggregate is one JSONB row keyed by its id (see [`schema`]); the store
//! serializes the domain struct with the same serde model the dev snapshot uses and
//! lifts only the lookup/scan keys into real columns. Postgres is the source of
//! truth. Queries are all runtime (`sqlx::query`), never the compile-time macros, so
//! the crate builds with no live database.
//!
//! Media blobs do **not** live here — they stay on the filesystem media tier; the
//! composition root passes that store (and the image codec) into [`PgStore::as_stores`].

use std::sync::Arc;

use app::{ImageTranscoder, MediaStore, StoreError, Stores};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::Row;

mod schema;
mod stores;

/// The Postgres store. One connection pool, shared by every port impl — the same
/// `Arc<PgStore>` satisfies all of them, exactly like the in-memory store.
#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// Connect to `database_url` and run the idempotent schema DDL. The pool is
    /// bounded so a burst of requests queues rather than exhausting Postgres.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Arc<Self>, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await
            .map_err(to_store_err)?;
        let store = Arc::new(Self { pool });
        store.migrate().await?;
        Ok(store)
    }

    /// Run the schema DDL. Idempotent — safe on every boot.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::raw_sql(schema::DDL).execute(&self.pool).await.map_err(to_store_err)?;
        Ok(())
    }

    /// Present this one store as the full [`Stores`] bundle. Media and the image
    /// codec are not Postgres concerns, so the composition root supplies them.
    pub fn as_stores(
        self: &Arc<Self>,
        media: Arc<dyn MediaStore>,
        image: Arc<dyn ImageTranscoder>,
    ) -> Stores {
        Stores {
            users: self.clone(),
            servers: self.clone(),
            memberships: self.clone(),
            channels: self.clone(),
            messages: self.clone(),
            reactions: self.clone(),
            proposals: self.clone(),
            votes: self.clone(),
            emojis: self.clone(),
            emoji_votes: self.clone(),
            rules: self.clone(),
            dms: self.clone(),
            blocks: self.clone(),
            friends: self.clone(),
            roles: self.clone(),
            role_color_votes: self.clone(),
            keys: self.clone(),
            channel_keys: self.clone(),
            invites: self.clone(),
            media,
            image,
        }
    }

    /// The shared pool, for the port impls in [`stores`].
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Map a `sqlx` failure onto the port-level [`StoreError`] taxonomy so an outage,
/// timeout, or constraint clash surfaces as a typed error the services already
/// handle — never a panic.
pub(crate) fn to_store_err(e: sqlx::Error) -> StoreError {
    match &e {
        sqlx::Error::PoolTimedOut => StoreError::Timeout,
        sqlx::Error::Database(db) if db.is_unique_violation() => StoreError::Conflict,
        _ => StoreError::Unavailable(e.to_string()),
    }
}

/// Decode a JSONB `data` column into its domain type, mapping a shape mismatch to
/// [`StoreError::Corrupt`] — a row that no longer deserializes is corruption, not an
/// outage.
pub(crate) fn from_json<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, StoreError> {
    serde_json::from_value(value).map_err(|e| StoreError::Corrupt(e.to_string()))
}

/// Wrap a domain value for binding into a JSONB parameter.
pub(crate) fn to_json<T: Serialize>(value: &T) -> Json<&T> {
    Json(value)
}

/// Decode the `data` JSONB column of a fetched row into its domain type — the read
/// counterpart of [`to_json`]. Every `SELECT` in this crate projects the aggregate
/// as a single `data` column, so the port impls all decode through here.
pub(crate) fn decode<T: DeserializeOwned>(row: &PgRow) -> Result<T, StoreError> {
    let value: serde_json::Value = row.try_get("data").map_err(to_store_err)?;
    from_json(value)
}

/// Pull the next value from a Postgres sequence — the id-minting primitive behind
/// every `next_*_id`. Atomic and gap-tolerant, unlike the in-memory counter.
pub(crate) async fn next_seq(pool: &PgPool, seq: &str) -> Result<u64, StoreError> {
    // `seq` is a fixed literal from this crate, never user input — safe to inline.
    let row = sqlx::query(&format!("SELECT nextval('{seq}') AS id"))
        .fetch_one(pool)
        .await
        .map_err(to_store_err)?;
    let id: i64 = row.try_get("id").map_err(to_store_err)?;
    Ok(id as u64)
}
