use app::{EmojiVoteStore, StoreError};
use async_trait::async_trait;
use domain::{EmojiId, EmojiVote, ServerId, UserId};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl EmojiVoteStore for PgStore {
    async fn upsert_emoji_vote(&self, vote: EmojiVote) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO emoji_votes (emoji_id, voter_id, server_id, data) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (emoji_id, voter_id) DO UPDATE \
             SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
        )
        .bind(vote.emoji_id.0 as i64)
        .bind(vote.voter.0 as i64)
        .bind(vote.server_id.0 as i64)
        .bind(to_json(&vote))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "emoji_votes", ChangeOp::Upsert, &vote).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn emoji_votes_for_server(&self, server: ServerId) -> Result<Vec<EmojiVote>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM emoji_votes WHERE server_id = $1 ORDER BY emoji_id, voter_id",
        )
        .bind(server.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn my_emoji_vote(&self, emoji: EmojiId, voter: UserId) -> Result<Option<bool>, StoreError> {
        let row = sqlx::query("SELECT data FROM emoji_votes WHERE emoji_id = $1 AND voter_id = $2")
            .bind(emoji.0 as i64)
            .bind(voter.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        let vote: Option<EmojiVote> = row.map(|r| decode(&r)).transpose()?;
        Ok(vote.map(|v| v.is_up))
    }
}
