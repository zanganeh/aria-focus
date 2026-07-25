use super::{PersistenceError, PreferencesRepository};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    Draft,
    Quoted,
    Authorized,
    Running,
    Paused,
    Cancelled,
    Failed,
    Validated,
    Staged,
    Activated,
    RolledBack,
}

impl BatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Quoted => "quoted",
            Self::Authorized => "authorized",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Validated => "validated",
            Self::Staged => "staged",
            Self::Activated => "activated",
            Self::RolledBack => "rolled_back",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "draft" => Self::Draft,
            "quoted" => Self::Quoted,
            "authorized" => Self::Authorized,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            "validated" => Self::Validated,
            "staged" => Self::Staged,
            "activated" => Self::Activated,
            "rolled_back" => Self::RolledBack,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicGenerationBatch {
    pub batch_id: String,
    pub state: BatchState,
    pub target_count: u16,
    pub budget_microdollars: u64,
    pub reserved_microdollars: u64,
    pub actual_microdollars: u64,
    pub catalog_snapshot_json: String,
    pub activation_version: Option<String>,
    pub previous_activation_version: Option<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemState {
    Queued,
    Refining,
    GeneratingAudio,
    GeneratingCover,
    Validating,
    Validated,
    Failed,
    Cancelled,
    Activated,
}

impl BatchItemState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Refining => "refining",
            Self::GeneratingAudio => "generating_audio",
            Self::GeneratingCover => "generating_cover",
            Self::Validating => "validating",
            Self::Validated => "validated",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Activated => "activated",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "refining" => Self::Refining,
            "generating_audio" => Self::GeneratingAudio,
            "generating_cover" => Self::GeneratingCover,
            "validating" => Self::Validating,
            "validated" => Self::Validated,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "activated" => Self::Activated,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicGenerationBatchItem {
    pub item_id: String,
    pub batch_id: String,
    pub ordinal: u16,
    pub activity: String,
    pub state: BatchItemState,
    pub idempotency_key: String,
    pub prompt_json: String,
    pub refined_prompt: Option<String>,
    pub audio_model: String,
    pub text_model: Option<String>,
    pub image_model: Option<String>,
    pub audio_request_id: Option<String>,
    pub text_request_id: Option<String>,
    pub image_request_id: Option<String>,
    pub audio_path: Option<String>,
    pub cover_path: Option<String>,
    pub audio_sha256: Option<String>,
    pub cover_sha256: Option<String>,
    pub estimated_microdollars: u64,
    pub actual_microdollars: u64,
    pub validation_json: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Text,
    Audio,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicGenerationAttempt {
    pub attempt_id: String,
    pub item_id: String,
    pub kind: AttemptKind,
    pub request_id: Option<String>,
    pub estimated_microdollars: u64,
    pub actual_microdollars: u64,
    pub state: String,
    pub error_code: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl AttemptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Audio => "audio",
            Self::Image => "image",
        }
    }
}

fn checked_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::Storage("numeric value is too large".into()))
}

fn checked_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .map_err(|_| PersistenceError::Storage("stored numeric value is negative".into()))
}

fn conversion(index: usize, message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(message.to_owned())),
    )
}

fn batch_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MusicGenerationBatch> {
    let state: String = row.get(1)?;
    Ok(MusicGenerationBatch {
        batch_id: row.get(0)?,
        state: BatchState::parse(&state).ok_or_else(|| conversion(1, "invalid batch state"))?,
        target_count: row
            .get::<_, i64>(2)?
            .try_into()
            .map_err(|_| conversion(2, "invalid target count"))?,
        budget_microdollars: checked_u64(row.get(3)?)
            .map_err(|_| conversion(3, "invalid budget"))?,
        reserved_microdollars: checked_u64(row.get(4)?)
            .map_err(|_| conversion(4, "invalid reserve"))?,
        actual_microdollars: checked_u64(row.get(5)?)
            .map_err(|_| conversion(5, "invalid actual cost"))?,
        catalog_snapshot_json: row.get(6)?,
        activation_version: row.get(7)?,
        previous_activation_version: row.get(8)?,
        revision: checked_u64(row.get(9)?).map_err(|_| conversion(9, "invalid revision"))?,
        created_at_ms: checked_u64(row.get(10)?)
            .map_err(|_| conversion(10, "invalid created time"))?,
        updated_at_ms: checked_u64(row.get(11)?)
            .map_err(|_| conversion(11, "invalid updated time"))?,
        error_code: row.get(12)?,
        error_message: row.get(13)?,
    })
}

impl PreferencesRepository {
    pub fn create_music_generation_batch(
        &mut self,
        batch: &MusicGenerationBatch,
        items: &[MusicGenerationBatchItem],
    ) -> Result<(), PersistenceError> {
        if items.is_empty()
            || items.len() != usize::from(batch.target_count)
            || batch.target_count > 100
            || serde_json::from_str::<serde_json::Value>(&batch.catalog_snapshot_json).is_err()
        {
            return Err(PersistenceError::Storage(
                "invalid music generation batch".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO music_generation_batches(batch_id,state,target_count,budget_microdollars,reserved_microdollars,actual_microdollars,currency,catalog_snapshot_json,activation_version,previous_activation_version,revision,created_at_ms,updated_at_ms,error_code,error_message) VALUES(?1,?2,?3,?4,?5,?6,'USD',?7,?8,?9,0,?10,?10,NULL,NULL)", params![batch.batch_id, batch.state.as_str(), batch.target_count, checked_i64(batch.budget_microdollars)?, checked_i64(batch.reserved_microdollars)?, checked_i64(batch.actual_microdollars)?, batch.catalog_snapshot_json, batch.activation_version, batch.previous_activation_version, checked_i64(batch.created_at_ms)?])?;
        for item in items {
            tx.execute("INSERT INTO music_generation_batch_items(item_id,batch_id,ordinal,activity,state,idempotency_key,prompt_json,refined_prompt,audio_model,text_model,image_model,audio_request_id,text_request_id,image_request_id,audio_path,cover_path,audio_sha256,cover_sha256,estimated_microdollars,actual_microdollars,validation_json,error_code,error_message,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,NULL,NULL,NULL,NULL,NULL,NULL,?12,0,NULL,NULL,NULL,0,?13,?13)", params![item.item_id,item.batch_id,item.ordinal,item.activity,item.state.as_str(),item.idempotency_key,item.prompt_json,item.refined_prompt,item.audio_model,item.text_model,item.image_model,checked_i64(item.estimated_microdollars)?,checked_i64(item.created_at_ms)?])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_music_generation_batch(
        &mut self,
        batch_id: &str,
    ) -> Result<Option<MusicGenerationBatch>, PersistenceError> {
        Ok(self.connection.query_row("SELECT batch_id,state,target_count,budget_microdollars,reserved_microdollars,actual_microdollars,catalog_snapshot_json,activation_version,previous_activation_version,revision,created_at_ms,updated_at_ms,error_code,error_message FROM music_generation_batches WHERE batch_id=?1", [batch_id], batch_row).optional()?)
    }

    pub fn list_music_generation_batches(
        &mut self,
        limit: usize,
    ) -> Result<Vec<MusicGenerationBatch>, PersistenceError> {
        let limit = i64::try_from(limit.min(50))
            .map_err(|_| PersistenceError::Storage("invalid batch limit".into()))?;
        let mut stmt = self.connection.prepare("SELECT batch_id,state,target_count,budget_microdollars,reserved_microdollars,actual_microdollars,catalog_snapshot_json,activation_version,previous_activation_version,revision,created_at_ms,updated_at_ms,error_code,error_message FROM music_generation_batches ORDER BY updated_at_ms DESC LIMIT ?1")?;
        let rows = stmt
            .query_map([limit], batch_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_music_generation_items(
        &mut self,
        batch_id: &str,
    ) -> Result<Vec<MusicGenerationBatchItem>, PersistenceError> {
        let mut stmt = self.connection.prepare("SELECT item_id,batch_id,ordinal,activity,state,idempotency_key,prompt_json,refined_prompt,audio_model,text_model,image_model,audio_request_id,text_request_id,image_request_id,audio_path,cover_path,audio_sha256,cover_sha256,estimated_microdollars,actual_microdollars,validation_json,error_code,error_message,revision,created_at_ms,updated_at_ms FROM music_generation_batch_items WHERE batch_id=?1 ORDER BY ordinal")?;
        let rows = stmt
            .query_map([batch_id], item_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_music_generation_batch(
        &mut self,
        batch_id: &str,
        expected_revision: u64,
        state: BatchState,
        reserved: u64,
        actual: u64,
        activation_version: Option<&str>,
        previous_version: Option<&str>,
        error: Option<(&str, &str)>,
        now_ms: u64,
    ) -> Result<MusicGenerationBatch, PersistenceError> {
        let changed = self.connection.execute("UPDATE music_generation_batches SET state=?2,reserved_microdollars=?3,actual_microdollars=?4,activation_version=?5,previous_activation_version=?6,error_code=?7,error_message=?8,revision=revision+1,updated_at_ms=?9 WHERE batch_id=?1 AND revision=?10", params![batch_id,state.as_str(),checked_i64(reserved)?,checked_i64(actual)?,activation_version,previous_version,error.map(|e|e.0),error.map(|e|e.1),checked_i64(now_ms)?,checked_i64(expected_revision)?])?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "batch revision is stale or missing".into(),
            ));
        }
        self.load_music_generation_batch(batch_id)?
            .ok_or_else(|| PersistenceError::Storage("batch disappeared".into()))
    }

    pub fn update_music_generation_budget(
        &mut self,
        batch_id: &str,
        expected_revision: u64,
        budget_microdollars: u64,
        now_ms: u64,
    ) -> Result<MusicGenerationBatch, PersistenceError> {
        let changed = self.connection.execute(
            "UPDATE music_generation_batches SET budget_microdollars=?2,revision=revision+1,updated_at_ms=?3 WHERE batch_id=?1 AND revision=?4 AND budget_microdollars<=?2",
            params![
                batch_id,
                checked_i64(budget_microdollars)?,
                checked_i64(now_ms)?,
                checked_i64(expected_revision)?
            ],
        )?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "batch budget update is stale, missing, or would reduce the authorized budget"
                    .into(),
            ));
        }
        self.load_music_generation_batch(batch_id)?
            .ok_or_else(|| PersistenceError::Storage("batch disappeared".into()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_music_generation_item(
        &mut self,
        item_id: &str,
        expected_revision: u64,
        state: BatchItemState,
        refined_prompt: Option<&str>,
        request_ids: (Option<&str>, Option<&str>, Option<&str>),
        paths: (Option<&str>, Option<&str>),
        hashes: (Option<&str>, Option<&str>),
        actual: u64,
        validation_json: Option<&str>,
        error: Option<(&str, &str)>,
        now_ms: u64,
    ) -> Result<(), PersistenceError> {
        let changed = self.connection.execute("UPDATE music_generation_batch_items SET state=?2,refined_prompt=COALESCE(?3,refined_prompt),audio_request_id=COALESCE(?4,audio_request_id),text_request_id=COALESCE(?5,text_request_id),image_request_id=COALESCE(?6,image_request_id),audio_path=COALESCE(?7,audio_path),cover_path=COALESCE(?8,cover_path),audio_sha256=COALESCE(?9,audio_sha256),cover_sha256=COALESCE(?10,cover_sha256),actual_microdollars=?11,validation_json=COALESCE(?12,validation_json),error_code=?13,error_message=?14,revision=revision+1,updated_at_ms=?15 WHERE item_id=?1 AND revision=?16", params![item_id,state.as_str(),refined_prompt,request_ids.0,request_ids.1,request_ids.2,paths.0,paths.1,hashes.0,hashes.1,checked_i64(actual)?,validation_json,error.map(|e|e.0),error.map(|e|e.1),checked_i64(now_ms)?,checked_i64(expected_revision)?])?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "batch item revision is stale or missing".into(),
            ));
        }
        Ok(())
    }

    pub fn reset_music_generation_item_for_retry(
        &mut self,
        item_id: &str,
        expected_revision: u64,
        estimated_microdollars: u64,
        now_ms: u64,
    ) -> Result<(), PersistenceError> {
        let changed = self.connection.execute(
            "UPDATE music_generation_batch_items SET state='queued',estimated_microdollars=?3,refined_prompt=NULL,audio_request_id=NULL,text_request_id=NULL,image_request_id=NULL,audio_path=NULL,cover_path=NULL,audio_sha256=NULL,cover_sha256=NULL,actual_microdollars=0,validation_json=NULL,error_code=NULL,error_message=NULL,revision=revision+1,updated_at_ms=?4 WHERE item_id=?1 AND revision=?2",
            params![
                item_id,
                checked_i64(expected_revision)?,
                checked_i64(estimated_microdollars)?,
                checked_i64(now_ms)?
            ],
        )?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "batch item revision is stale or missing".into(),
            ));
        }
        Ok(())
    }

    pub fn sum_music_generation_attempt_costs(
        &mut self,
        batch_id: &str,
    ) -> Result<u64, PersistenceError> {
        let total: i64 = self.connection.query_row(
            "SELECT COALESCE(SUM(actual_microdollars),0) FROM music_generation_attempts WHERE item_id IN (SELECT item_id FROM music_generation_batch_items WHERE batch_id=?1)",
            [batch_id],
            |row| row.get(0),
        )?;
        u64::try_from(total)
            .map_err(|_| PersistenceError::Storage("generation costs cannot be negative".into()))
    }

    pub fn reserve_music_generation_budget(
        &mut self,
        batch_id: &str,
        expected_revision: u64,
        amount: u64,
        now_ms: u64,
    ) -> Result<MusicGenerationBatch, PersistenceError> {
        let changed = self.connection.execute("UPDATE music_generation_batches SET reserved_microdollars=reserved_microdollars+?2,revision=revision+1,updated_at_ms=?3 WHERE batch_id=?1 AND revision=?4 AND reserved_microdollars+actual_microdollars+?2 <= budget_microdollars", params![batch_id,checked_i64(amount)?,checked_i64(now_ms)?,checked_i64(expected_revision)?])?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "generation budget exhausted or batch revision is stale".into(),
            ));
        }
        self.load_music_generation_batch(batch_id)?
            .ok_or_else(|| PersistenceError::Storage("batch disappeared".into()))
    }

    pub fn record_music_generation_attempt(
        &mut self,
        attempt: &MusicGenerationAttempt,
    ) -> Result<(), PersistenceError> {
        if attempt.attempt_id.len() < 16
            || attempt.attempt_id.len() > 128
            || attempt.item_id.len() < 16
            || attempt.item_id.len() > 128
            || !matches!(
                attempt.state.as_str(),
                "reserved" | "submitted" | "succeeded" | "failed" | "unknown"
            )
            || attempt.updated_at_ms < attempt.created_at_ms
        {
            return Err(PersistenceError::Storage(
                "invalid music generation attempt".into(),
            ));
        }
        self.connection.execute(
            "INSERT INTO music_generation_attempts(attempt_id,item_id,kind,request_id,estimated_microdollars,actual_microdollars,state,error_code,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                attempt.attempt_id,
                attempt.item_id,
                attempt.kind.as_str(),
                attempt.request_id,
                checked_i64(attempt.estimated_microdollars)?,
                checked_i64(attempt.actual_microdollars)?,
                attempt.state,
                attempt.error_code,
                checked_i64(attempt.created_at_ms)?,
                checked_i64(attempt.updated_at_ms)?,
            ],
        )?;
        Ok(())
    }
}

fn item_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MusicGenerationBatchItem> {
    let state: String = row.get(4)?;
    Ok(MusicGenerationBatchItem {
        item_id: row.get(0)?,
        batch_id: row.get(1)?,
        ordinal: row
            .get::<_, i64>(2)?
            .try_into()
            .map_err(|_| conversion(2, "invalid ordinal"))?,
        activity: row.get(3)?,
        state: BatchItemState::parse(&state).ok_or_else(|| conversion(4, "invalid item state"))?,
        idempotency_key: row.get(5)?,
        prompt_json: row.get(6)?,
        refined_prompt: row.get(7)?,
        audio_model: row.get(8)?,
        text_model: row.get(9)?,
        image_model: row.get(10)?,
        audio_request_id: row.get(11)?,
        text_request_id: row.get(12)?,
        image_request_id: row.get(13)?,
        audio_path: row.get(14)?,
        cover_path: row.get(15)?,
        audio_sha256: row.get(16)?,
        cover_sha256: row.get(17)?,
        estimated_microdollars: checked_u64(row.get(18)?)
            .map_err(|_| conversion(18, "invalid estimate"))?,
        actual_microdollars: checked_u64(row.get(19)?)
            .map_err(|_| conversion(19, "invalid actual"))?,
        validation_json: row.get(20)?,
        error_code: row.get(21)?,
        error_message: row.get(22)?,
        revision: checked_u64(row.get(23)?).map_err(|_| conversion(23, "invalid revision"))?,
        created_at_ms: checked_u64(row.get(24)?).map_err(|_| conversion(24, "invalid created"))?,
        updated_at_ms: checked_u64(row.get(25)?).map_err(|_| conversion(25, "invalid updated"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_and_round_trips_batch() {
        let mut repo = PreferencesRepository::in_memory().unwrap();
        let now = 10_000;
        let batch = MusicGenerationBatch {
            batch_id: "batch_test_123456".into(),
            state: BatchState::Draft,
            target_count: 1,
            budget_microdollars: 100,
            reserved_microdollars: 0,
            actual_microdollars: 0,
            catalog_snapshot_json: "{}".into(),
            activation_version: None,
            previous_activation_version: None,
            revision: 0,
            created_at_ms: now,
            updated_at_ms: now,
            error_code: None,
            error_message: None,
        };
        let item = MusicGenerationBatchItem {
            item_id: "batch_item_123456".into(),
            batch_id: batch.batch_id.clone(),
            ordinal: 0,
            activity: "motivation".into(),
            state: BatchItemState::Queued,
            idempotency_key: "idem_batch_item_123456".into(),
            prompt_json: "{}".into(),
            refined_prompt: None,
            audio_model: "google/lyria-3-pro-preview".into(),
            text_model: None,
            image_model: None,
            audio_request_id: None,
            text_request_id: None,
            image_request_id: None,
            audio_path: None,
            cover_path: None,
            audio_sha256: None,
            cover_sha256: None,
            estimated_microdollars: 100,
            actual_microdollars: 0,
            validation_json: None,
            error_code: None,
            error_message: None,
            revision: 0,
            created_at_ms: now,
            updated_at_ms: now,
        };
        repo.create_music_generation_batch(&batch, &[item]).unwrap();
        assert_eq!(
            repo.load_music_generation_batch(&batch.batch_id)
                .unwrap()
                .unwrap()
                .target_count,
            1
        );
        assert_eq!(
            repo.list_music_generation_items(&batch.batch_id)
                .unwrap()
                .len(),
            1
        );
        assert!(repo
            .reserve_music_generation_budget(&batch.batch_id, 0, 100, now + 1)
            .is_ok());
        repo.record_music_generation_attempt(&MusicGenerationAttempt {
            attempt_id: "attempt_batch_item_123456_audio_1".into(),
            item_id: "batch_item_123456".into(),
            kind: AttemptKind::Audio,
            request_id: Some("req_test".into()),
            estimated_microdollars: 80,
            actual_microdollars: 80,
            state: "succeeded".into(),
            error_code: None,
            created_at_ms: now,
            updated_at_ms: now + 1,
        })
        .unwrap();
        assert_eq!(
            repo.connection
                .query_row(
                    "SELECT COUNT(*) FROM music_generation_attempts WHERE item_id=?1",
                    [&"batch_item_123456"],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
