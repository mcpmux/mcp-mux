//! Local ONNX embedding service for hybrid tool search ranking.
//!
//! Downloads `bge-small-en-v1.5` on first use into the app data directory and
//! exposes non-blocking state so callers can fall back to lexical-only search.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use mcpmux_storage::hash_embedding_content;
use parking_lot::{Mutex, RwLock};
use tracing::{info, warn};

#[cfg(any(test, feature = "test-utils"))]
use std::collections::HashMap;

/// BGE retrieval prefix for user queries.
const QUERY_PREFIX: &str = "query: ";

/// BGE retrieval prefix for document/passage text.
const PASSAGE_PREFIX: &str = "passage: ";

/// Default embedding model — CPU ONNX, downloaded on first use (~67 MB).
const DEFAULT_MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15;

/// Lifecycle state of the embedding model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingState {
    /// Model has not been requested yet.
    NotDownloaded,
    /// Model download / ONNX init is in progress.
    Downloading,
    /// Model is loaded and ready for inference.
    Ready,
    /// Download or init failed; lexical-only fallback applies.
    Failed {
        /// Sanitized error message (no secrets).
        error: String,
    },
}

/// Local embedding inference with lazy model download.
pub struct EmbeddingService {
    cache_dir: PathBuf,
    model_name: &'static str,
    state: Arc<RwLock<EmbeddingState>>,
    model: Arc<Mutex<Option<TextEmbedding>>>,
    init_started: Arc<AtomicBool>,
    /// Deterministic vectors for CI relevance eval (no model download).
    #[cfg(any(test, feature = "test-utils"))]
    test_vectors: Arc<RwLock<HashMap<String, Vec<f32>>>>,
}

impl EmbeddingService {
    /// Create a service that stores models under `{data_dir}/embeddings`.
    pub fn new(data_dir: PathBuf) -> Self {
        let cache_dir = data_dir.join("embeddings");
        Self {
            cache_dir,
            model_name: "bge-small-en-v1.5",
            state: Arc::new(RwLock::new(EmbeddingState::NotDownloaded)),
            model: Arc::new(Mutex::new(None)),
            init_started: Arc::new(AtomicBool::new(false)),
            #[cfg(any(test, feature = "test-utils"))]
            test_vectors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Install deterministic embedding vectors and mark the model ready (CI / integration tests).
    #[cfg(any(test, feature = "test-utils"))]
    pub fn install_test_vectors(&self, vectors: HashMap<String, Vec<f32>>) {
        *self.test_vectors.write() = vectors;
        *self.state.write() = EmbeddingState::Ready;
    }

    /// Return the current model lifecycle state without blocking.
    pub fn state(&self) -> EmbeddingState {
        self.state.read().clone()
    }

    /// Cache directory passed to fastembed for model artifacts.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Stable model version used for persisted embedding keys.
    pub fn model_version(&self) -> &'static str {
        self.model_name
    }

    /// Build alias-free text used for embedding and content hashing.
    pub fn embedding_haystack(feature_name: &str, description: Option<&str>) -> String {
        match description {
            Some(description) if !description.is_empty() => {
                format!("{feature_name} {description}")
            }
            _ => feature_name.to_string(),
        }
    }

    /// Stable content hash for alias-free embedding text.
    pub fn content_hash(feature_name: &str, description: Option<&str>) -> String {
        let haystack = Self::embedding_haystack(feature_name, description);
        hash_embedding_content(&haystack)
    }

    /// Start background model download/init when still `NotDownloaded`.
    ///
    /// Idempotent — subsequent calls are no-ops while downloading or after terminal states.
    pub fn ensure_init_started(&self) {
        if !matches!(self.state(), EmbeddingState::NotDownloaded) {
            return;
        }

        if self.init_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let mut state = self.state.write();
        if !matches!(*state, EmbeddingState::NotDownloaded) {
            return;
        }

        *state = EmbeddingState::Downloading;
        drop(state);

        info!(
            target: "embed",
            "[embed] model = {}, state = Downloading",
            self.model_name
        );

        let cache_dir = self.cache_dir.clone();
        let model_name = self.model_name;
        let state = Arc::clone(&self.state);
        let model_slot = Arc::clone(&self.model);

        std::thread::spawn(move || {
            let started = Instant::now();
            match load_text_embedding(&cache_dir) {
                Ok(embedding) => {
                    let download_ms = started.elapsed().as_millis() as u64;
                    *model_slot.lock() = Some(embedding);
                    *state.write() = EmbeddingState::Ready;
                    info!(
                        target: "embed",
                        "[embed] model = {}, state = Ready, download_ms = {}",
                        model_name,
                        download_ms
                    );
                }
                Err(error) => {
                    let download_ms = started.elapsed().as_millis() as u64;
                    let message = error.to_string();
                    *state.write() = EmbeddingState::Failed {
                        error: message.clone(),
                    };
                    info!(
                        target: "embed",
                        "[embed] model = {}, state = Failed, download_ms = {}, error = {}",
                        model_name,
                        download_ms,
                        message
                    );
                }
            }
        });
    }

    /// Embed a search query. Returns `None` when the model is not `Ready` (never blocks on download).
    pub fn embed_query(&self, query: &str, query_id: Option<&str>) -> Option<Vec<f32>> {
        self.embed_prefixed(&format!("{QUERY_PREFIX}{query}"), query_id, 1)
    }

    /// Embed document texts for ranking. Returns `None` when the model is not `Ready`.
    pub fn embed_documents(
        &self,
        documents: &[String],
        query_id: Option<&str>,
    ) -> Option<Vec<Vec<f32>>> {
        if documents.is_empty() {
            return Some(Vec::new());
        }

        let prefixed: Vec<String> = documents
            .iter()
            .map(|doc| format!("{PASSAGE_PREFIX}{doc}"))
            .collect();
        let refs: Vec<&str> = prefixed.iter().map(String::as_str).collect();
        self.embed_prefixed_batch(&refs, query_id, documents.len())
    }

    /// Cosine similarity between two equal-length embedding vectors.
    pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot / (norm_a * norm_b)
    }

    fn embed_prefixed(
        &self,
        text: &str,
        query_id: Option<&str>,
        docs_embedded: usize,
    ) -> Option<Vec<f32>> {
        let vectors = self.embed_prefixed_batch(&[text], query_id, docs_embedded)?;
        vectors.into_iter().next()
    }

    fn embed_prefixed_batch(
        &self,
        texts: &[&str],
        query_id: Option<&str>,
        docs_embedded: usize,
    ) -> Option<Vec<Vec<f32>>> {
        #[cfg(any(test, feature = "test-utils"))]
        {
            let stub = self.test_vectors.read();
            if !stub.is_empty() {
                let vectors: Vec<Vec<f32>> = texts
                    .iter()
                    .map(|text| {
                        stub.get(*text)
                            .cloned()
                            .unwrap_or_else(|| panic!("missing test embedding vector for `{text}`"))
                    })
                    .collect();
                self.log_embedding_state(query_id, "ready", docs_embedded, Some(0));
                return Some(vectors);
            }
        }

        let state = self.state();
        let model_state = model_state_label(&state);

        if !matches!(state, EmbeddingState::Ready) {
            self.ensure_init_started();
            self.log_embedding_state(query_id, model_state, docs_embedded, None);
            return None;
        }

        let started = Instant::now();
        let vectors = self.embed_with_spawn_blocking(texts)?;
        let embed_ms = started.elapsed().as_millis() as u64;
        self.log_embedding_state(query_id, "ready", docs_embedded, Some(embed_ms));
        Some(vectors)
    }

    fn embed_with_spawn_blocking(&self, texts: &[&str]) -> Option<Vec<Vec<f32>>> {
        let model_slot = Arc::clone(&self.model);
        let inputs: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
        let result = run_spawn_blocking(move || {
            let mut guard = model_slot.lock();
            let embedding = guard.as_mut()?;
            let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
            match embedding.embed(&refs, None) {
                Ok(raw) => Some(raw.into_iter().map(to_f32_vector).collect()),
                Err(_) => None,
            }
        });
        result
    }

    fn log_embedding_state(
        &self,
        query_id: Option<&str>,
        model_state: &'static str,
        docs_embedded: usize,
        embed_ms: Option<u64>,
    ) {
        match (query_id, embed_ms) {
            (Some(query_id), Some(embed_ms)) => {
                info!(
                    target: "embed",
                    "[embed] query_id = {}, model_state = {}, docs_embedded = {}, embed_ms = {}",
                    query_id,
                    model_state,
                    docs_embedded,
                    embed_ms
                );
            }
            (Some(query_id), None) => {
                info!(
                    target: "embed",
                    "[embed] query_id = {}, model_state = {}, docs_embedded = {}",
                    query_id,
                    model_state,
                    docs_embedded
                );
            }
            (None, Some(embed_ms)) => {
                info!(
                    target: "embed",
                    "[embed] model_state = {}, docs_embedded = {}, embed_ms = {}",
                    model_state,
                    docs_embedded,
                    embed_ms
                );
            }
            (None, None) => {
                info!(
                    target: "embed",
                    "[embed] model_state = {}, docs_embedded = {}",
                    model_state,
                    docs_embedded
                );
            }
        }
    }
}

fn load_text_embedding(cache_dir: &Path) -> anyhow::Result<TextEmbedding> {
    std::fs::create_dir_all(cache_dir)?;
    let options = InitOptions::new(DEFAULT_MODEL)
        .with_cache_dir(cache_dir.to_path_buf())
        .with_show_download_progress(false);
    TextEmbedding::try_new(options)
}

fn model_state_label(state: &EmbeddingState) -> &'static str {
    match state {
        EmbeddingState::NotDownloaded => "absent",
        EmbeddingState::Downloading => "downloading",
        EmbeddingState::Ready => "ready",
        EmbeddingState::Failed { .. } => "failed",
    }
}

fn to_f32_vector(embedding: fastembed::Embedding) -> Vec<f32> {
    embedding
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn log_spawn_blocking_join_error(context: &'static str, error: tokio::task::JoinError) {
    if error.is_panic() {
        let message = panic_payload_message(error.into_panic());
        warn!(
            target: "embed",
            context,
            panic = %message,
            "[embed] spawn_blocking panicked"
        );
        return;
    }
    if error.is_cancelled() {
        warn!(target: "embed", context, "[embed] spawn_blocking cancelled");
        return;
    }
    warn!(
        target: "embed",
        context,
        error = %error,
        "[embed] spawn_blocking join failed"
    );
}

fn await_spawn_blocking<T, F>(handle: tokio::runtime::Handle, task: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    match handle.block_on(tokio::task::spawn_blocking(task)) {
        Ok(value) => value,
        Err(error) => {
            log_spawn_blocking_join_error("spawn_blocking", error);
            None
        }
    }
}

/// Run a blocking embed `task` without stalling the async scheduler.
///
/// Inside a Tokio context this uses `block_in_place`, which **requires the
/// multi-thread runtime** — it panics on a `current_thread` runtime. The
/// gateway runs on Tokio's multi-thread scheduler (Axum/Tauri), so that
/// invariant holds in production. Outside any runtime (e.g. some unit
/// tests) it spins up a temporary multi-thread runtime instead.
fn run_spawn_blocking<T, F>(task: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return tokio::task::block_in_place(|| await_spawn_blocking(handle, task));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .ok()?;
    await_spawn_blocking(runtime.handle().clone(), task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_unit_vectors_is_one() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![1.0_f32, 0.0, 0.0];
        assert!((EmbeddingService::cosine(&a, &b) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cosine_orthogonal_vectors_is_zero() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        assert!(EmbeddingService::cosine(&a, &b).abs() < f32::EPSILON);
    }

    #[test]
    fn cosine_opposite_vectors_is_negative_one() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![-1.0_f32, 0.0];
        assert!((EmbeddingService::cosine(&a, &b) + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cosine_mismatched_lengths_returns_zero() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![1.0_f32];
        assert_eq!(EmbeddingService::cosine(&a, &b), 0.0);
    }

    #[test]
    fn cosine_known_vectors_matches_hand_computed_score() {
        let a = vec![3.0_f32, 4.0];
        let b = vec![4.0_f32, 3.0];
        let expected = 24.0_f32 / (5.0 * 5.0);
        assert!((EmbeddingService::cosine(&a, &b) - expected).abs() < 1e-6);
    }

    #[test]
    fn embed_query_returns_none_while_model_not_ready() {
        let service = EmbeddingService::new(std::env::temp_dir().join("mcpmux-embed-test"));
        assert_eq!(service.state(), EmbeddingState::NotDownloaded);
        assert!(service.embed_query("hello", Some("q-test")).is_none());
        assert!(matches!(
            service.state(),
            EmbeddingState::NotDownloaded | EmbeddingState::Downloading
        ));
    }

    #[test]
    fn ensure_init_started_is_idempotent() {
        let service = EmbeddingService::new(std::env::temp_dir().join("mcpmux-embed-idempotent"));
        service.ensure_init_started();
        service.ensure_init_started();
        assert!(matches!(
            service.state(),
            EmbeddingState::Downloading | EmbeddingState::Ready | EmbeddingState::Failed { .. }
        ));
    }

    #[test]
    fn content_hash_changes_when_description_changes() {
        let hash_before = EmbeddingService::content_hash("search_issues", Some("Find Jira issues"));
        let hash_after =
            EmbeddingService::content_hash("search_issues", Some("Find open Jira issues"));
        assert_ne!(hash_before, hash_after);
    }

    /// Requires network + ~67 MB model download; run locally with `cargo test -- --ignored`.
    #[test]
    #[ignore = "downloads bge-small-en-v1.5 from HuggingFace"]
    fn semantic_matching_doc_scores_higher_than_unrelated() {
        let service = EmbeddingService::new(std::env::temp_dir().join("mcpmux-embed-semantic"));
        service.ensure_init_started();

        let deadline = Instant::now() + std::time::Duration::from_secs(120);
        while !matches!(
            service.state(),
            EmbeddingState::Ready | EmbeddingState::Failed { .. }
        ) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for embedding model"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        if let EmbeddingState::Failed { error } = service.state() {
            panic!("model init failed: {error}");
        }

        let query = service
            .embed_query("post a comment on an issue", None)
            .expect("query embedding");
        let matching = service
            .embed_documents(
                &["create_issue_comment Create a comment on a Jira issue".to_string()],
                None,
            )
            .expect("matching doc embedding");
        let unrelated = service
            .embed_documents(
                &["list_calendar_events List upcoming calendar events".to_string()],
                None,
            )
            .expect("unrelated doc embedding");

        let matching_score = EmbeddingService::cosine(&query, &matching[0]);
        let unrelated_score = EmbeddingService::cosine(&query, &unrelated[0]);
        assert!(
            matching_score > unrelated_score,
            "matching={matching_score}, unrelated={unrelated_score}"
        );
    }
}
