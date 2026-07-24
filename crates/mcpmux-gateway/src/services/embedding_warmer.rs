//! Background embedding warmer for per-server tool catalogs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use dashmap::{DashMap, DashSet};
use mcpmux_core::{EmbeddingRecord, EmbeddingRepository, FeatureType, ServerFeatureRepository};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::services::{EmbeddingService, EmbeddingState};

/// Event-driven embedding warm worker.
///
/// On server connect/feature-discovery, it embeds the full server tool catalog,
/// skipping vectors already present in storage or memory.
#[derive(Clone)]
pub struct EmbeddingWarmer {
    feature_repo: Arc<dyn ServerFeatureRepository>,
    embedding_repo: Arc<dyn EmbeddingRepository>,
    embedding_store: Arc<DashMap<String, Vec<f32>>>,
    embeddings: Arc<EmbeddingService>,
    in_flight: Arc<DashSet<(Uuid, String)>>,
}

impl EmbeddingWarmer {
    /// Build a warmer.
    pub fn new(
        feature_repo: Arc<dyn ServerFeatureRepository>,
        embedding_repo: Arc<dyn EmbeddingRepository>,
        embedding_store: Arc<DashMap<String, Vec<f32>>>,
        embeddings: Arc<EmbeddingService>,
    ) -> Self {
        Self {
            feature_repo,
            embedding_repo,
            embedding_store,
            embeddings,
            in_flight: Arc::new(DashSet::new()),
        }
    }

    /// Poll until the embedding model is `Ready` or a bounded budget elapses.
    ///
    /// On a cold install the connect-triggered warm fires while the ~67 MB
    /// model is still downloading. Without waiting, every `embed_documents`
    /// returns `None`, the persistent cache stays empty, and nothing
    /// re-warms until some later connect event happens to land after the
    /// model is `Ready`. Polling here lets the existing single warm task
    /// populate vectors once the download finishes. Returns `true` when
    /// ready, `false` if the model `Failed` or the budget ran out.
    async fn await_model_ready(&self) -> bool {
        const BUDGET: Duration = Duration::from_secs(120);
        const POLL: Duration = Duration::from_millis(250);
        self.embeddings.ensure_init_started();
        let deadline = Instant::now() + BUDGET;
        loop {
            match self.embeddings.state() {
                EmbeddingState::Ready => return true,
                EmbeddingState::Failed { .. } => return false,
                _ => {}
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Enqueue warmup for one connected server.
    pub fn warm_server(&self, space_id: Uuid, server_id: String) {
        let key = (space_id, server_id);
        if !self.in_flight.insert(key.clone()) {
            return;
        }

        let warmer = self.clone();
        tokio::spawn(async move {
            if let Err(error) = warmer.warm_server_inner(key.0, &key.1).await {
                warn!(
                    space_id = %key.0,
                    server_id = %key.1,
                    error = %error,
                    "[embed] warmer failed"
                );
            }
            warmer.in_flight.remove(&key);
        });
    }

    async fn warm_server_inner(&self, space_id: Uuid, server_id: &str) -> anyhow::Result<()> {
        let tools = self
            .feature_repo
            .list_for_space(&space_id.to_string())
            .await?
            .into_iter()
            .filter(|feature| {
                feature.feature_type == FeatureType::Tool && feature.server_id.as_str() == server_id
            })
            .collect::<Vec<_>>();

        if tools.is_empty() {
            return Ok(());
        }

        // Kick model load now so the model is ready by the time a search arrives,
        // even when the store is already fully warm and no new embeddings are needed.
        self.embeddings.ensure_init_started();

        let mut haystacks_by_hash: HashMap<String, String> = HashMap::new();
        for tool in tools {
            let haystack = EmbeddingService::embedding_haystack(
                tool.feature_name.as_str(),
                tool.description.as_deref(),
            );
            let content_hash = EmbeddingService::content_hash(
                tool.feature_name.as_str(),
                tool.description.as_deref(),
            );
            haystacks_by_hash.entry(content_hash).or_insert(haystack);
        }

        let catalog_tools = haystacks_by_hash.len();
        let mut missing_hashes = haystacks_by_hash
            .keys()
            .filter(|content_hash| !self.embedding_store.contains_key(*content_hash))
            .cloned()
            .collect::<Vec<_>>();

        let existing = self
            .embedding_repo
            .get_many(&missing_hashes, self.embeddings.model_version())
            .await?;
        let existing_hashes: HashSet<String> = existing
            .iter()
            .map(|record| record.content_hash.clone())
            .collect();

        for record in existing {
            self.embedding_store
                .insert(record.content_hash, record.vector);
        }

        missing_hashes.retain(|content_hash| !existing_hashes.contains(content_hash));
        let missing = missing_hashes.len();
        let skipped_present = catalog_tools.saturating_sub(missing);
        debug!(
            space_id = %space_id,
            server_id,
            catalog_tools,
            missing,
            "[embed] warm enqueue"
        );

        if missing_hashes.is_empty() {
            info!(
                space_id = %space_id,
                server_id,
                embedded = 0,
                skipped_present,
                embed_ms = 0_u64,
                model_version = self.embeddings.model_version(),
                model_state = ?self.embeddings.state(),
                "[embed] warm batch done"
            );
            return Ok(());
        }

        if !self.await_model_ready().await {
            info!(
                space_id = %space_id,
                server_id,
                embedded = 0,
                skipped_present,
                missing,
                model_state = ?self.embeddings.state(),
                "[embed] warm batch skipped (model not ready within budget)"
            );
            return Ok(());
        }

        let mut records = Vec::new();
        let embed_started = Instant::now();
        for content_hash in missing_hashes {
            let Some(haystack) = haystacks_by_hash.get(&content_hash).cloned() else {
                continue;
            };

            let Some(vectors) = self.embeddings.embed_documents(&[haystack], None) else {
                continue;
            };
            let Some(vector) = vectors.into_iter().next() else {
                continue;
            };

            records.push(EmbeddingRecord {
                content_hash: content_hash.clone(),
                model_version: self.embeddings.model_version().to_string(),
                vector: vector.clone(),
            });
            self.embedding_store.insert(content_hash, vector);
        }

        if records.is_empty() {
            info!(
                space_id = %space_id,
                server_id,
                embedded = 0,
                skipped_present,
                missing,
                embed_ms = embed_started.elapsed().as_millis() as u64,
                model_version = self.embeddings.model_version(),
                model_state = ?self.embeddings.state(),
                "[embed] warm batch done"
            );
            return Ok(());
        }

        debug!(
            space_id = %space_id,
            server_id,
            embedded = records.len(),
            "[embed] warmer upserting records"
        );
        self.embedding_repo.upsert_many(&records).await?;
        info!(
            space_id = %space_id,
            server_id,
            embedded = records.len(),
            skipped_present,
            embed_ms = embed_started.elapsed().as_millis() as u64,
            model_version = self.embeddings.model_version(),
            "[embed] warm batch done"
        );
        Ok(())
    }
}
