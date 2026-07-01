//! Shared ranking and fuzzy-match helpers for discovery indexes.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use tracing::debug;

/// Boost applied when every query token appears in the document haystack.
const AND_MATCH_BOOST: f64 = 1.0;

/// Common stop tokens dropped from lexical matching on both query and document sides.
const STOPWORDS: &[&str] = &["a", "an", "the", "on", "in", "for", "of", "to", "with"];

/// Query-side synonym groups for intent phrasing variants (tools, resources, prompts).
const SYNONYM_MAP: &[(&str, &[&str])] = &[
    ("ticket", &["issue"]),
    ("tickets", &["issues"]),
    ("jira", &["atlassian"]),
    ("fetch", &["get"]),
    ("find", &["search", "get"]),
    ("retrieve", &["get"]),
    ("create", &["add", "post"]),
    ("make", &["create", "add"]),
    ("delete", &["remove"]),
    ("remove", &["delete"]),
];

/// Optional tracing context for tool search ranking.
pub struct RankTraceContext<'a> {
    pub query_id: &'a str,
}

/// Tokenize text for TF-IDF scoring.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty() && !STOPWORDS.contains(token))
        .map(String::from)
        .collect()
}

/// Expand query tokens with synonym variants while preserving first-seen order.
pub(crate) fn expand_query_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut expanded: Vec<String> = Vec::with_capacity(tokens.len() * 2);

    for token in tokens {
        if seen.insert(token.clone()) {
            expanded.push(token.clone());
        }
        for (key, synonyms) in SYNONYM_MAP {
            if token == *key {
                for syn in *synonyms {
                    let synonym = syn.to_string();
                    if seen.insert(synonym.clone()) {
                        expanded.push(synonym);
                    }
                }
            }
        }
    }

    expanded
}

/// Tokenize and expand a search query for lexical and hybrid ranking.
pub(crate) fn prepare_query_tokens(query: &str) -> Vec<String> {
    expand_query_tokens(tokenize(query))
}

/// Return true when at least one query token appears in `haystack`.
fn matches_token_overlap(query_tokens: &[String], haystack: &str) -> bool {
    if query_tokens.is_empty() {
        return true;
    }
    let doc_tokens: HashSet<String> = tokenize(haystack).into_iter().collect();
    query_tokens.iter().any(|token| doc_tokens.contains(token))
}

/// Return true when every query token appears in `haystack`.
fn all_tokens_present(query_tokens: &[String], haystack: &str) -> bool {
    if query_tokens.is_empty() {
        return false;
    }
    let doc_tokens: HashSet<String> = tokenize(haystack).into_iter().collect();
    query_tokens.iter().all(|token| doc_tokens.contains(token))
}

/// Build a corpus-level document-frequency map from a slice of haystack strings.
///
/// Returns `(corpus_size, doc_freq)` where `doc_freq[token]` is the number of documents
/// containing that token at least once. Amortises tokenization to O(N) so callers avoid
/// repeating it O(N log N) times inside a sort comparator.
pub(crate) fn build_corpus_doc_freq(corpus: &[String]) -> (usize, HashMap<String, usize>) {
    let corpus_size = corpus.len();
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for doc in corpus {
        let tokens: HashSet<String> = tokenize(doc).into_iter().collect();
        for token in tokens {
            *doc_freq.entry(token).or_default() += 1;
        }
    }
    (corpus_size, doc_freq)
}

/// TF-IDF score from precomputed corpus statistics and a pre-tokenized document.
///
/// Separating the precomputed path from the corpus-building step lets
/// `filter_and_rank_inner` call this once per candidate rather than rebuilding
/// corpus statistics on every comparator invocation.
fn tf_idf_score_precomputed(
    query_tokens: &[String],
    doc_tokens: &[String],
    corpus_size: usize,
    corpus_doc_freq: &HashMap<String, usize>,
) -> f64 {
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }

    let doc_len = doc_tokens.len() as f64;
    let corpus_size_f = corpus_size.max(1) as f64;

    let mut doc_term_freq: HashMap<String, usize> = HashMap::new();
    for token in doc_tokens {
        *doc_term_freq.entry(token.clone()).or_default() += 1;
    }

    let mut idf_cache: HashMap<String, f64> = HashMap::new();
    let mut score = 0.0;

    for token in query_tokens {
        let tf = doc_term_freq.get(token).copied().unwrap_or(0) as f64 / doc_len;
        if tf == 0.0 {
            continue;
        }

        let idf = *idf_cache.entry(token.clone()).or_insert_with(|| {
            let docs_with_term = corpus_doc_freq.get(token).copied().unwrap_or(0) as f64;
            ((corpus_size_f + 1.0) / (docs_with_term + 1.0)).ln() + 1.0
        });

        score += tf * idf;
    }

    score
}

/// Lexical relevance score (TF-IDF + AND-match boost) from precomputed corpus statistics.
pub(crate) fn lexical_score_precomputed(
    query_tokens: &[String],
    doc_tokens: &[String],
    corpus_size: usize,
    corpus_doc_freq: &HashMap<String, usize>,
) -> f64 {
    let base = tf_idf_score_precomputed(query_tokens, doc_tokens, corpus_size, corpus_doc_freq);
    let doc_token_set: HashSet<&str> = doc_tokens.iter().map(String::as_str).collect();
    let all_present = !query_tokens.is_empty()
        && query_tokens
            .iter()
            .all(|t| doc_token_set.contains(t.as_str()));
    if all_present {
        base + AND_MATCH_BOOST
    } else {
        base
    }
}

/// Filter haystacks by optional token-overlap query and optional server id, then rank.
pub fn filter_and_rank<'a, T, FServer, FHaystack>(
    entries: &'a [T],
    query: Option<&str>,
    server_id: Option<&str>,
    server_id_fn: FServer,
    haystack_fn: FHaystack,
) -> Vec<&'a T>
where
    FServer: Fn(&T) -> &str,
    FHaystack: Fn(&T) -> String,
{
    filter_and_rank_inner(entries, query, server_id, server_id_fn, haystack_fn, None).0
}

/// Like [`filter_and_rank`] but emits a lexical-pass `[search]` trace event.
pub(crate) fn filter_and_rank_traced<'a, T, FServer, FHaystack>(
    entries: &'a [T],
    query: Option<&str>,
    server_id: Option<&str>,
    server_id_fn: FServer,
    haystack_fn: FHaystack,
    trace: &RankTraceContext<'_>,
) -> (Vec<&'a T>, Option<f64>)
where
    FServer: Fn(&T) -> &str,
    FHaystack: Fn(&T) -> String,
{
    filter_and_rank_inner(
        entries,
        query,
        server_id,
        server_id_fn,
        haystack_fn,
        Some(trace),
    )
}

/// Shared filter-and-rank implementation with optional lexical-pass tracing.
fn filter_and_rank_inner<'a, T, FServer, FHaystack>(
    entries: &'a [T],
    query: Option<&str>,
    server_id: Option<&str>,
    server_id_fn: FServer,
    haystack_fn: FHaystack,
    trace: Option<&RankTraceContext<'_>>,
) -> (Vec<&'a T>, Option<f64>)
where
    FServer: Fn(&T) -> &str,
    FHaystack: Fn(&T) -> String,
{
    let query_tokens = query.map(prepare_query_tokens).unwrap_or_default();
    let mut and_boost_hits = 0usize;
    let index_entries = entries.len();
    let filter_started = Instant::now();

    let mut matched: Vec<&T> = entries
        .iter()
        .filter(|entry| {
            if let Some(sid) = server_id {
                if server_id_fn(entry) != sid {
                    return false;
                }
            }
            if !query_tokens.is_empty() {
                let haystack = haystack_fn(entry);
                if !matches_token_overlap(&query_tokens, &haystack) {
                    return false;
                }
                if all_tokens_present(&query_tokens, &haystack) {
                    and_boost_hits += 1;
                }
            }
            true
        })
        .collect();
    let filter_ms = filter_started.elapsed().as_millis() as u64;

    let rank_started = Instant::now();
    let top_lexical_score = if query.is_some() {
        let corpus: Vec<String> = matched.iter().map(|entry| haystack_fn(entry)).collect();
        let (corpus_size, corpus_doc_freq) = build_corpus_doc_freq(&corpus);

        // Precompute (entry, haystack, score) once per candidate so sort_by compares
        // cached scores instead of re-tokenizing the whole corpus O(N log N) times.
        let mut scored: Vec<(&T, String, f64)> = matched
            .iter()
            .map(|entry| {
                let haystack = haystack_fn(entry);
                let doc_tokens = tokenize(&haystack);
                let score = lexical_score_precomputed(
                    &query_tokens,
                    &doc_tokens,
                    corpus_size,
                    &corpus_doc_freq,
                );
                (*entry, haystack, score)
            })
            .collect();

        scored.sort_by(|(_, hay_a, score_a), (_, hay_b, score_b)| {
            score_b
                .partial_cmp(score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| hay_a.cmp(hay_b))
        });

        let top_score = scored.first().map(|(_, _, score)| *score);
        matched = scored.into_iter().map(|(entry, _, _)| entry).collect();
        top_score
    } else {
        matched.sort_by_key(|a| haystack_fn(a));
        None
    };
    let rank_ms = rank_started.elapsed().as_millis() as u64;

    if let Some(trace_ctx) = trace {
        if query.is_some() {
            debug!(
                query_id = trace_ctx.query_id,
                index_entries,
                tokens = ?query_tokens,
                candidates_after_filter = matched.len(),
                and_boost_hits,
                filter_ms,
                rank_ms,
                lexical_total_ms = filter_ms + rank_ms,
                "[search] lexical pass"
            );
        }
    }

    (matched, top_lexical_score)
}

/// Return up to `limit` candidates closest to `query` by Levenshtein distance.
pub fn levenshtein_suggestions(query: &str, candidates: &[String], limit: usize) -> Vec<String> {
    if query.is_empty() || candidates.is_empty() || limit == 0 {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(String, usize)> = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.clone(),
                strsim::levenshtein(&query_lower, &candidate.to_lowercase()),
            )
        })
        .collect();

    scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    scored
        .into_iter()
        .take(limit)
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEntry {
        qualified_name: String,
        haystack: String,
    }

    fn test_haystack(entry: &TestEntry) -> String {
        entry.haystack.clone()
    }

    fn test_server_id(_entry: &TestEntry) -> &str {
        "test"
    }

    #[test]
    fn tf_idf_ranks_closer_match_first() {
        let entries = ["github_list_issues", "github_get_me", "jira_list_issues"];
        let corpus: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
        let (corpus_size, corpus_doc_freq) = build_corpus_doc_freq(&corpus);
        let query_tokens = tokenize("list issues");
        let score_list = tf_idf_score_precomputed(
            &query_tokens,
            &tokenize("github_list_issues List issues"),
            corpus_size,
            &corpus_doc_freq,
        );
        let score_get = tf_idf_score_precomputed(
            &query_tokens,
            &tokenize("github_get_me Get current user"),
            corpus_size,
            &corpus_doc_freq,
        );
        assert!(score_list > score_get);
    }

    #[test]
    fn levenshtein_suggests_near_match() {
        let candidates = vec![
            "github_list_issues".to_string(),
            "github_get_me".to_string(),
        ];
        let suggestions = levenshtein_suggestions("list_isses", &candidates, 2);
        assert_eq!(
            suggestions.first().map(String::as_str),
            Some("github_list_issues")
        );
    }

    #[test]
    fn token_overlap_matches_hyphenated_tool_name() {
        let entries = vec![TestEntry {
            qualified_name: "canva_list-folder-items".to_string(),
            haystack: "canva_list-folder-items list-folder-items List folder items".to_string(),
        }];
        let matched = filter_and_rank(
            &entries,
            Some("list folder"),
            None,
            |_| "test",
            test_haystack,
        );
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].qualified_name, "canva_list-folder-items");
    }

    #[test]
    fn token_overlap_returns_zero_for_nonsense_query() {
        let entries = vec![TestEntry {
            qualified_name: "canva_list-folder-items".to_string(),
            haystack: "canva_list-folder-items list-folder-items List folder items".to_string(),
        }];
        let matched = filter_and_rank(
            &entries,
            Some("xyznotreal"),
            None,
            |_| "test",
            test_haystack,
        );
        assert!(matched.is_empty());
    }

    #[test]
    fn multi_token_ranking_favors_all_tokens_present() {
        let entries = vec![
            TestEntry {
                qualified_name: "partial_list".to_string(),
                haystack: "partial_list list something".to_string(),
            },
            TestEntry {
                qualified_name: "full_list_folder".to_string(),
                haystack: "full_list_folder list folder items".to_string(),
            },
        ];
        let matched = filter_and_rank(
            &entries,
            Some("list folder"),
            None,
            test_server_id,
            test_haystack,
        );
        assert_eq!(matched.len(), 2);
        assert_eq!(matched[0].qualified_name, "full_list_folder");
    }

    #[test]
    fn and_boost_increases_lexical_score() {
        let corpus = vec![
            "partial list something".to_string(),
            "full list folder items".to_string(),
        ];
        let (corpus_size, corpus_doc_freq) = build_corpus_doc_freq(&corpus);
        let query_tokens = tokenize("list folder");
        let partial = lexical_score_precomputed(
            &query_tokens,
            &tokenize("partial list something"),
            corpus_size,
            &corpus_doc_freq,
        );
        let full = lexical_score_precomputed(
            &query_tokens,
            &tokenize("full list folder items"),
            corpus_size,
            &corpus_doc_freq,
        );
        assert!(full > partial);
    }

    #[test]
    fn stopwords_filtered_from_tokens() {
        let tokens = tokenize("post a comment on a jira issue");
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"on".to_string()));
        assert!(tokens.contains(&"jira".to_string()));
        assert!(tokens.contains(&"issue".to_string()));
    }

    #[test]
    fn synonym_expansion_jira_ticket_matches_issue_tools() {
        let entries = vec![TestEntry {
            qualified_name: "atlassian_getJiraIssue".to_string(),
            haystack: "getJiraIssue atlassian_getJiraIssue Get a Jira issue".to_string(),
        }];
        let matched = filter_and_rank(
            &entries,
            Some("jira ticket"),
            None,
            test_server_id,
            test_haystack,
        );
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].qualified_name, "atlassian_getJiraIssue");
    }
}
