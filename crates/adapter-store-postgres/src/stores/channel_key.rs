use app::{ChannelKeyStore, StoreError};
use async_trait::async_trait;
use domain::{ChannelId, ChannelKeyGrant, UserId};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl ChannelKeyStore for PgStore {
    async fn put_grant(&self, grant: ChannelKeyGrant) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO channel_key_grants (channel_id, member_id, epoch, data) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (channel_id, member_id, epoch) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(grant.channel_id.0 as i64)
        .bind(grant.member.0 as i64)
        .bind(grant.epoch as i64)
        .bind(to_json(&grant))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "channel_grants", ChangeOp::Upsert, &grant).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn grants_for_member(
        &self,
        channel: ChannelId,
        member: UserId,
    ) -> Result<Vec<ChannelKeyGrant>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM channel_key_grants WHERE channel_id = $1 AND member_id = $2 ORDER BY epoch",
        )
        .bind(channel.0 as i64)
        .bind(member.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
