//! Prior credential discovery and reuse service (Issue #227)
//!
//! Enables detecting and reusing credential inputs from prior installations
//! in other Spaces without duplicating secret storage into an unencrypted or shared entity.

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::PriorCredentialSummary;
use crate::repository::{CredentialRepository, InstalledServerRepository, SpaceRepository};

/// Find if the same server has been installed/configured with non-empty credentials in another Space.
///
/// Returns the most recently updated candidate without decrypting or leaking secret values to the UI.
pub async fn find_prior_credential(
    installed_server_repo: &dyn InstalledServerRepository,
    credential_repo: Option<&dyn CredentialRepository>,
    space_repo: &dyn SpaceRepository,
    current_space_id: &str,
    server_id: &str,
    target_key: Option<&str>,
) -> anyhow::Result<Option<PriorCredentialSummary>> {
    let all_installed = installed_server_repo.list().await?;

    struct Candidate {
        space_id: String,
        available_keys: Vec<String>,
        updated_at: Option<DateTime<Utc>>,
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for installed in all_installed {
        if installed.server_id != server_id {
            continue;
        }
        if installed.space_id == current_space_id {
            continue;
        }

        let mut available_keys: Vec<String> = installed
            .input_values
            .iter()
            .filter(|(k, v)| {
                if let Some(target) = target_key {
                    if k.as_str() != target {
                        return false;
                    }
                }
                !v.trim().is_empty()
            })
            .map(|(k, _)| k.clone())
            .collect();

        // Also check credential_repo for this space if provided
        if let Some(creds) = credential_repo {
            if let Ok(space_uuid) = Uuid::parse_str(&installed.space_id) {
                if let Ok(cred_list) = creds.get_all(&space_uuid, server_id).await {
                    for cred in cred_list {
                        let cred_key = cred.credential_type.as_str();
                        if let Some(target) = target_key {
                            if cred_key != target {
                                continue;
                            }
                        }
                        if !cred.value.trim().is_empty()
                            && !available_keys.iter().any(|k| k == cred_key)
                        {
                            available_keys.push(cred_key.to_string());
                        }
                    }
                }
            }
        }

        if !available_keys.is_empty() {
            available_keys.sort();
            candidates.push(Candidate {
                space_id: installed.space_id,
                available_keys,
                updated_at: Some(installed.updated_at),
            });
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    // Sort by updated_at descending (most recent first)
    candidates.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
    let best = candidates.remove(0);

    // Resolve Space name
    let space_name = if let Ok(uuid) = Uuid::parse_str(&best.space_id) {
        space_repo
            .get(&uuid)
            .await?
            .map(|s| s.name)
            .unwrap_or_else(|| best.space_id.clone())
    } else {
        best.space_id.clone()
    };

    Ok(Some(PriorCredentialSummary {
        source_space_id: best.space_id,
        source_space_name: space_name,
        server_id: server_id.to_string(),
        available_keys: best.available_keys,
        updated_at: best.updated_at,
    }))
}

/// Fetch decrypted input/credential values from the source Space for the given server.
/// This is invoked ONLY when the user clicks "[ Use ]" on the reuse nudge.
pub async fn get_prior_credential_values(
    installed_server_repo: &dyn InstalledServerRepository,
    credential_repo: Option<&dyn CredentialRepository>,
    source_space_id: &str,
    server_id: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let mut values = HashMap::new();

    // 1. Get from installed_server_repository
    if let Some(installed) = installed_server_repo
        .get_by_server_id(source_space_id, server_id)
        .await?
    {
        for (k, v) in installed.input_values {
            if !v.trim().is_empty() {
                values.insert(k, v);
            }
        }
    }

    // 2. Get from credential_repo if present
    if let Some(creds) = credential_repo {
        if let Ok(space_uuid) = Uuid::parse_str(source_space_id) {
            if let Ok(cred_list) = creds.get_all(&space_uuid, server_id).await {
                for cred in cred_list {
                    if !cred.value.trim().is_empty() {
                        values.insert(cred.credential_type.as_str().to_string(), cred.value);
                    }
                }
            }
        }
    }

    Ok(values)
}
