//! SQLite implementation of EmbeddingRepository.

use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use ring::digest::{digest, SHA256};
use rusqlite::params;
use tokio::sync::Mutex;

use crate::Database;

/// SQLite-backed implementation of EmbeddingRepository.
pub struct SqliteEmbeddingRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteEmbeddingRepository {
    /// Create a new SQLite embedding repository.
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }
}

/// Compute the stable SHA-256 content hash for embedding text.
pub fn hash_embedding_content(content: &str) -> String {
    let hash = digest(&SHA256, content.as_bytes());
    hex::encode(hash.as_ref())
}

/// Encode an embedding vector as little-endian f32 bytes.
fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decode a little-endian f32 byte buffer into an embedding vector.
fn decode_vector(blob: &[u8]) -> Result<Vec<f32>> {
    if !blob.len().is_multiple_of(std::mem::size_of::<f32>()) {
        bail!(
            "Embedding vector blob length {} is not divisible by {}",
            blob.len(),
            std::mem::size_of::<f32>()
        );
    }

    Ok(blob
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

#[async_trait]
impl mcpmux_core::EmbeddingRepository for SqliteEmbeddingRepository {
    async fn get_many(
        &self,
        content_hashes: &[String],
        model_version: &str,
    ) -> mcpmux_core::RepoResult<Vec<mcpmux_core::EmbeddingRecord>> {
        if content_hashes.is_empty() {
            return Ok(Vec::new());
        }

        let db = self.db.lock().await;
        let conn = db.connection();

        let mut records = Vec::new();
        // Fetch in chunks with a single `IN (...)` query per chunk instead of
        // one round-trip per hash. SQLite caps bound variables (~999 on older
        // builds); 800 + the shared model_version stays clear of that limit.
        const CHUNK: usize = 800;
        for chunk in content_hashes.chunks(CHUNK) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT content_hash, vector
                 FROM tool_embeddings
                 WHERE model_version = ? AND content_hash IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;

            let mut bound: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + 1);
            bound.push(&model_version);
            for content_hash in chunk {
                bound.push(content_hash);
            }

            let rows = stmt.query_map(bound.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            for row in rows {
                let (content_hash, vector_blob) = row?;
                records.push(mcpmux_core::EmbeddingRecord {
                    content_hash,
                    model_version: model_version.to_string(),
                    vector: decode_vector(&vector_blob)?,
                });
            }
        }

        Ok(records)
    }

    async fn upsert_many(
        &self,
        records: &[mcpmux_core::EmbeddingRecord],
    ) -> mcpmux_core::RepoResult<()> {
        if records.is_empty() {
            return Ok(());
        }

        let db = self.db.lock().await;
        let conn = db.connection();

        for record in records {
            conn.execute(
                "INSERT INTO tool_embeddings (content_hash, model_version, vector, dims, created_at)
                 VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s', 'now') AS INTEGER))
                 ON CONFLICT(content_hash, model_version) DO UPDATE SET
                     vector = excluded.vector,
                     dims = excluded.dims",
                params![
                    record.content_hash,
                    record.model_version,
                    encode_vector(&record.vector),
                    record.vector.len() as i64,
                ],
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{hash_embedding_content, SqliteEmbeddingRepository};
    use crate::Database;
    use mcpmux_core::{EmbeddingRecord, EmbeddingRepository};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Create an in-memory database for repository tests.
    async fn setup_test_db() -> Arc<Mutex<Database>> {
        let db = Database::open_in_memory().expect("Failed to create in-memory database");
        Arc::new(Mutex::new(db))
    }

    #[tokio::test]
    async fn upsert_and_get_round_trip() {
        let db = setup_test_db().await;
        let repository = SqliteEmbeddingRepository::new(db);

        let content_hash = hash_embedding_content("tool: read_file\nReads files from disk.");
        let model_version = "bge-small-en-v1.5";
        let expected_vector = vec![0.125, -2.5, 7.75, 0.0];

        repository
            .upsert_many(&[EmbeddingRecord {
                content_hash: content_hash.clone(),
                model_version: model_version.to_string(),
                vector: expected_vector.clone(),
            }])
            .await
            .expect("Upsert should succeed");

        let records = repository
            .get_many(&[content_hash], model_version)
            .await
            .expect("Get should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].vector, expected_vector);
    }

    #[tokio::test]
    async fn upsert_overwrites_on_primary_key_conflict() {
        let db = setup_test_db().await;
        let repository = SqliteEmbeddingRepository::new(db);

        let content_hash = hash_embedding_content("tool: write_file\nWrites files to disk.");
        let model_version = "bge-small-en-v1.5";

        repository
            .upsert_many(&[EmbeddingRecord {
                content_hash: content_hash.clone(),
                model_version: model_version.to_string(),
                vector: vec![1.0, 2.0],
            }])
            .await
            .expect("Initial upsert should succeed");

        repository
            .upsert_many(&[EmbeddingRecord {
                content_hash: content_hash.clone(),
                model_version: model_version.to_string(),
                vector: vec![3.5, 4.5, 5.5],
            }])
            .await
            .expect("Conflict upsert should succeed");

        let records = repository
            .get_many(&[content_hash], model_version)
            .await
            .expect("Get should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].vector, vec![3.5, 4.5, 5.5]);
    }

    #[tokio::test]
    async fn get_many_returns_empty_for_missing_hashes() {
        let db = setup_test_db().await;
        let repository = SqliteEmbeddingRepository::new(db);
        let model_version = "bge-small-en-v1.5";

        let records = repository
            .get_many(
                &[
                    hash_embedding_content("missing one"),
                    hash_embedding_content("missing two"),
                ],
                model_version,
            )
            .await
            .expect("Get should succeed");

        assert!(records.is_empty());
    }
}
