//! SQLite implementation of ServerFeatureRepository.
//!
//! Manages server_features table - stores discovered MCP features (tools, prompts, resources)
//! from connected servers, scoped to each space.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// Feature type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureType {
    Tool,
    Prompt,
    Resource,
}

impl FeatureType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FeatureType::Tool => "tool",
            FeatureType::Prompt => "prompt",
            FeatureType::Resource => "resource",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "tool" => Some(Self::Tool),
            "prompt" => Some(Self::Prompt),
            "resource" => Some(Self::Resource),
            _ => None,
        }
    }
}

/// A discovered server feature (tool, prompt, or resource)
#[derive(Debug, Clone)]
pub struct ServerFeature {
    pub id: String,
    pub space_id: String,
    pub server_id: String,
    pub feature_type: FeatureType,
    pub feature_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub raw_json: Option<serde_json::Value>,  // Complete JSON from backend
    pub discovered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_available: bool,
}

impl ServerFeature {
    /// Create a new tool feature
    pub fn new_tool(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            space_id: space_id.into(),
            server_id: server_id.into(),
            feature_type: FeatureType::Tool,
            feature_name: name.into(),
            display_name: None,
            description: None,
            raw_json: None,
            discovered_at: now,
            last_seen_at: now,
            is_available: true,
        }
    }

    /// Create a new prompt feature
    pub fn new_prompt(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            space_id: space_id.into(),
            server_id: server_id.into(),
            feature_type: FeatureType::Prompt,
            feature_name: name.into(),
            display_name: None,
            description: None,
            raw_json: None,
            discovered_at: now,
            last_seen_at: now,
            is_available: true,
        }
    }

    /// Create a new resource feature
    pub fn new_resource(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            space_id: space_id.into(),
            server_id: server_id.into(),
            feature_type: FeatureType::Resource,
            feature_name: name.into(),
            display_name: None,
            description: None,
            raw_json: None,
            discovered_at: now,
            last_seen_at: now,
            is_available: true,
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_raw_json(mut self, json: serde_json::Value) -> Self {
        self.raw_json = Some(json);
        self
    }
}

/// Repository trait for server features
#[async_trait]
pub trait ServerFeatureRepository: Send + Sync {
    /// List all features for a space
    async fn list_by_space(&self, space_id: &str) -> Result<Vec<ServerFeature>>;

    /// List features for a specific server in a space
    async fn list_by_server(&self, space_id: &str, server_id: &str) -> Result<Vec<ServerFeature>>;

    /// List features by type for a server in a space
    async fn list_by_type(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
    ) -> Result<Vec<ServerFeature>>;

    /// Get a specific feature
    async fn get(&self, id: &str) -> Result<Option<ServerFeature>>;

    /// Get a feature by name (space + server + type + name)
    async fn get_by_name(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
        name: &str,
    ) -> Result<Option<ServerFeature>>;

    /// Upsert a feature (insert or update last_seen_at)
    async fn upsert(&self, feature: &ServerFeature) -> Result<()>;

    /// Bulk upsert features (for discovery)
    async fn upsert_many(&self, features: &[ServerFeature]) -> Result<()>;

    /// Mark features as unavailable if not in the provided list
    async fn mark_unavailable_except(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
        available_names: &[String],
    ) -> Result<()>;

    /// Delete a feature
    async fn delete(&self, id: &str) -> Result<()>;

    /// Delete all features for a server in a space
    async fn delete_by_server(&self, space_id: &str, server_id: &str) -> Result<()>;
}

/// SQLite-backed implementation of ServerFeatureRepository
pub struct SqliteServerFeatureRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteServerFeatureRepository {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    fn parse_datetime(s: &str) -> DateTime<Utc> {
        // Try RFC3339 first
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        // Try SQLite datetime format
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }

    fn row_to_feature(row: &rusqlite::Row<'_>) -> rusqlite::Result<ServerFeature> {
        let raw_json_str: Option<String> = row.get(7)?;
        Ok(ServerFeature {
            id: row.get(0)?,
            space_id: row.get(1)?,
            server_id: row.get(2)?,
            feature_type: FeatureType::from_str(&row.get::<_, String>(3)?)
                .unwrap_or(FeatureType::Tool),
            feature_name: row.get(4)?,
            display_name: row.get(5)?,
            description: row.get(6)?,
            raw_json: raw_json_str.and_then(|s| serde_json::from_str(&s).ok()),
            discovered_at: Self::parse_datetime(&row.get::<_, String>(8)?),
            last_seen_at: Self::parse_datetime(&row.get::<_, String>(9)?),
            is_available: row.get::<_, i32>(10)? == 1,
        })
    }
}

#[async_trait]
impl ServerFeatureRepository for SqliteServerFeatureRepository {
    async fn list_by_space(&self, space_id: &str) -> Result<Vec<ServerFeature>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, space_id, server_id, feature_type, feature_name, 
                    display_name, description, raw_json, discovered_at, 
                    last_seen_at, is_available
             FROM server_features
             WHERE space_id = ?
             ORDER BY server_id, feature_type, feature_name",
        )?;

        let features = stmt
            .query_map(params![space_id], Self::row_to_feature)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    async fn list_by_server(&self, space_id: &str, server_id: &str) -> Result<Vec<ServerFeature>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, space_id, server_id, feature_type, feature_name, 
                    display_name, description, raw_json, discovered_at, 
                    last_seen_at, is_available
             FROM server_features
             WHERE space_id = ? AND server_id = ?
             ORDER BY feature_type, feature_name",
        )?;

        let features = stmt
            .query_map(params![space_id, server_id], Self::row_to_feature)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    async fn list_by_type(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
    ) -> Result<Vec<ServerFeature>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, space_id, server_id, feature_type, feature_name, 
                    display_name, description, raw_json, discovered_at, 
                    last_seen_at, is_available
             FROM server_features
             WHERE space_id = ? AND server_id = ? AND feature_type = ?
             ORDER BY feature_name",
        )?;

        let features = stmt
            .query_map(
                params![space_id, server_id, feature_type.as_str()],
                Self::row_to_feature,
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    async fn get(&self, id: &str) -> Result<Option<ServerFeature>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, space_id, server_id, feature_type, feature_name, 
                        display_name, description, raw_json, discovered_at, 
                        last_seen_at, is_available
                 FROM server_features
                 WHERE id = ?",
                params![id],
                Self::row_to_feature,
            )
            .optional()?;

        Ok(result)
    }

    async fn get_by_name(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
        name: &str,
    ) -> Result<Option<ServerFeature>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, space_id, server_id, feature_type, feature_name, 
                        display_name, description, raw_json, discovered_at, 
                        last_seen_at, is_available
                 FROM server_features
                 WHERE space_id = ? AND server_id = ? AND feature_type = ? AND feature_name = ?",
                params![space_id, server_id, feature_type.as_str(), name],
                Self::row_to_feature,
            )
            .optional()?;

        Ok(result)
    }

    async fn upsert(&self, feature: &ServerFeature) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let raw_json_str = feature
            .raw_json
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap_or_default());

        conn.execute(
            "INSERT INTO server_features 
                (id, space_id, server_id, feature_type, feature_name, 
                 display_name, description, raw_json, discovered_at, 
                 last_seen_at, is_available)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(space_id, server_id, feature_type, feature_name) DO UPDATE SET
                display_name = COALESCE(?6, display_name),
                description = COALESCE(?7, description),
                raw_json = COALESCE(?8, raw_json),
                last_seen_at = ?10,
                is_available = ?11",
            params![
                feature.id,
                feature.space_id,
                feature.server_id,
                feature.feature_type.as_str(),
                feature.feature_name,
                feature.display_name,
                feature.description,
                raw_json_str,
                feature.discovered_at.to_rfc3339(),
                feature.last_seen_at.to_rfc3339(),
                if feature.is_available { 1 } else { 0 },
            ],
        )?;

        Ok(())
    }

    async fn upsert_many(&self, features: &[ServerFeature]) -> Result<()> {
        for feature in features {
            self.upsert(feature).await?;
        }
        Ok(())
    }

    async fn mark_unavailable_except(
        &self,
        space_id: &str,
        server_id: &str,
        feature_type: FeatureType,
        available_names: &[String],
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        if available_names.is_empty() {
            // Mark all as unavailable
            conn.execute(
                "UPDATE server_features 
                 SET is_available = 0, last_seen_at = datetime('now')
                 WHERE space_id = ? AND server_id = ? AND feature_type = ?",
                params![space_id, server_id, feature_type.as_str()],
            )?;
        } else {
            // Build placeholders for IN clause
            let placeholders: Vec<String> = (0..available_names.len()).map(|_| "?".to_string()).collect();
            let in_clause = placeholders.join(", ");
            
            let sql = format!(
                "UPDATE server_features 
                 SET is_available = 0, last_seen_at = datetime('now')
                 WHERE space_id = ?1 AND server_id = ?2 AND feature_type = ?3 
                   AND feature_name NOT IN ({})",
                in_clause
            );

            let mut stmt = conn.prepare(&sql)?;
            
            // Build params dynamically
            let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            param_values.push(Box::new(space_id.to_string()));
            param_values.push(Box::new(server_id.to_string()));
            param_values.push(Box::new(feature_type.as_str().to_string()));
            for name in available_names {
                param_values.push(Box::new(name.clone()));
            }
            
            let params_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
            stmt.execute(params_refs.as_slice())?;
        }

        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute("DELETE FROM server_features WHERE id = ?", params![id])?;
        Ok(())
    }

    async fn delete_by_server(&self, space_id: &str, server_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM server_features WHERE space_id = ? AND server_id = ?",
            params![space_id, server_id],
        )?;

        Ok(())
    }
}

// Conversions between storage and core ServerFeature types
impl From<ServerFeature> for mcpmux_core::ServerFeature {
    fn from(f: ServerFeature) -> Self {
        mcpmux_core::ServerFeature {
            id: uuid::Uuid::parse_str(&f.id).unwrap_or_else(|_| uuid::Uuid::new_v4()),
            space_id: f.space_id,
            server_id: f.server_id,
            server_alias: None, // Enriched later with prefix from cache
            feature_type: match f.feature_type {
                FeatureType::Tool => mcpmux_core::FeatureType::Tool,
                FeatureType::Prompt => mcpmux_core::FeatureType::Prompt,
                FeatureType::Resource => mcpmux_core::FeatureType::Resource,
            },
            feature_name: f.feature_name,
            display_name: f.display_name,
            description: f.description,
            raw_json: f.raw_json,
            discovered_at: f.discovered_at,
            last_seen_at: f.last_seen_at,
            is_available: f.is_available,
        }
    }
}

impl From<mcpmux_core::ServerFeature> for ServerFeature {
    fn from(f: mcpmux_core::ServerFeature) -> Self {
        ServerFeature {
            id: f.id.to_string(),
            space_id: f.space_id,
            server_id: f.server_id,
            feature_type: match f.feature_type {
                mcpmux_core::FeatureType::Tool => FeatureType::Tool,
                mcpmux_core::FeatureType::Prompt => FeatureType::Prompt,
                mcpmux_core::FeatureType::Resource => FeatureType::Resource,
            },
            feature_name: f.feature_name,
            display_name: f.display_name,
            description: f.description,
            raw_json: f.raw_json,
            discovered_at: f.discovered_at,
            last_seen_at: f.last_seen_at,
            is_available: f.is_available,
        }
    }
}

// Implement mcpmux_core::ServerFeatureRepository for compatibility with gateway services
// This converts between the storage's ServerFeature and core's ServerFeature types
#[async_trait]
impl mcpmux_core::ServerFeatureRepository for SqliteServerFeatureRepository {
    async fn list_for_space(&self, space_id: &str) -> mcpmux_core::RepoResult<Vec<mcpmux_core::ServerFeature>> {
        let features = self.list_by_space(space_id).await?;
        Ok(features.into_iter().map(|f| f.into()).collect())
    }

    async fn list_for_server(&self, space_id: &str, server_id: &str) -> mcpmux_core::RepoResult<Vec<mcpmux_core::ServerFeature>> {
        let features = self.list_by_server(space_id, server_id).await?;
        Ok(features.into_iter().map(|f| f.into()).collect())
    }

    async fn get(&self, id: &uuid::Uuid) -> mcpmux_core::RepoResult<Option<mcpmux_core::ServerFeature>> {
        let result = ServerFeatureRepository::get(self, &id.to_string()).await?;
        Ok(result.map(|f| f.into()))
    }

    async fn upsert(&self, feature: &mcpmux_core::ServerFeature) -> mcpmux_core::RepoResult<()> {
        let storage_feature: ServerFeature = feature.clone().into();
        ServerFeatureRepository::upsert(self, &storage_feature).await
    }

    async fn upsert_many(&self, features: &[mcpmux_core::ServerFeature]) -> mcpmux_core::RepoResult<()> {
        let storage_features: Vec<ServerFeature> = features.iter().map(|f| f.clone().into()).collect();
        ServerFeatureRepository::upsert_many(self, &storage_features).await
    }

    async fn delete(&self, id: &uuid::Uuid) -> mcpmux_core::RepoResult<()> {
        ServerFeatureRepository::delete(self, &id.to_string()).await
    }

    async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> mcpmux_core::RepoResult<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        conn.execute(
            "UPDATE server_features SET is_available = 0 WHERE space_id = ? AND server_id = ?",
            params![space_id, server_id],
        )?;
        
        Ok(())
    }

    async fn delete_for_server(&self, space_id: &str, server_id: &str) -> mcpmux_core::RepoResult<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        conn.execute(
            "DELETE FROM server_features WHERE space_id = ? AND server_id = ?",
            params![space_id, server_id],
        )?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn setup_test_db() -> Arc<Mutex<Database>> {
        let db = Database::open_in_memory().expect("Failed to create in-memory database");
        Arc::new(Mutex::new(db))
    }

    #[tokio::test]
    async fn test_upsert_and_get() {
        let db = setup_test_db().await;
        let repo = SqliteServerFeatureRepository::new(db);

        // First, we need to create the space and server (would be done via fixtures normally)
        // For this test, we'll just test the feature operations assuming FKs are disabled

        let feature = ServerFeature::new_tool("space_default", "server1", "read_file")
            .with_display_name("Read File")
            .with_description("Reads a file from the filesystem");

        repo.upsert(&feature).await.unwrap();

        let retrieved = repo.get(&feature.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.feature_name, "read_file");
        assert_eq!(retrieved.display_name, Some("Read File".to_string()));
    }
}
