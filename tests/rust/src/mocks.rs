//! Mock repository implementations for testing
//!
//! In-memory implementations of all repository traits for fast, isolated tests.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

use mcpmux_core::{
    domain::{
        Client, Credential, CredentialType, FeatureSet, FeatureSetMember, FeatureSetType,
        InstalledServer, MemberMode, MemberType, OutboundOAuthRegistration, ServerFeature, Space,
    },
    repository::{
        AppSettingsRepository, CredentialRepository, FeatureSetRepository,
        InboundMcpClientRepository, InstalledServerRepository, OutboundOAuthRepository, RepoResult,
        ServerFeatureRepository, SpaceRepository,
    },
};

// ============================================================================
// MockSpaceRepository
// ============================================================================

#[derive(Default)]
pub struct MockSpaceRepository {
    spaces: RwLock<HashMap<Uuid, Space>>,
    default_id: RwLock<Option<Uuid>>,
}

impl MockSpaceRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_space(self, space: Space) -> Self {
        self.spaces.write().unwrap().insert(space.id, space);
        self
    }
}

#[async_trait]
impl SpaceRepository for MockSpaceRepository {
    async fn list(&self) -> RepoResult<Vec<Space>> {
        Ok(self.spaces.read().unwrap().values().cloned().collect())
    }

    async fn get(&self, id: &Uuid) -> RepoResult<Option<Space>> {
        Ok(self.spaces.read().unwrap().get(id).cloned())
    }

    async fn create(&self, space: &Space) -> RepoResult<()> {
        self.spaces.write().unwrap().insert(space.id, space.clone());
        Ok(())
    }

    async fn update(&self, space: &Space) -> RepoResult<()> {
        self.spaces.write().unwrap().insert(space.id, space.clone());
        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> RepoResult<()> {
        self.spaces.write().unwrap().remove(id);
        Ok(())
    }

    async fn get_default(&self) -> RepoResult<Option<Space>> {
        let default_id = *self.default_id.read().unwrap();
        if let Some(id) = default_id {
            return self.get(&id).await;
        }
        Ok(None)
    }

    async fn set_default(&self, id: &Uuid) -> RepoResult<()> {
        *self.default_id.write().unwrap() = Some(*id);
        Ok(())
    }
}

// ============================================================================
// MockInstalledServerRepository
// ============================================================================

#[derive(Default)]
pub struct MockInstalledServerRepository {
    servers: RwLock<HashMap<Uuid, InstalledServer>>,
}

impl MockInstalledServerRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_server(self, server: InstalledServer) -> Self {
        self.servers.write().unwrap().insert(server.id, server);
        self
    }
}

#[async_trait]
impl InstalledServerRepository for MockInstalledServerRepository {
    async fn list(&self) -> RepoResult<Vec<InstalledServer>> {
        Ok(self.servers.read().unwrap().values().cloned().collect())
    }

    async fn list_for_space(&self, space_id: &str) -> RepoResult<Vec<InstalledServer>> {
        Ok(self
            .servers
            .read()
            .unwrap()
            .values()
            .filter(|s| s.space_id == space_id)
            .cloned()
            .collect())
    }

    async fn list_by_source_file(
        &self,
        _file_path: &std::path::Path,
    ) -> RepoResult<Vec<InstalledServer>> {
        Ok(vec![])
    }

    async fn get(&self, id: &Uuid) -> RepoResult<Option<InstalledServer>> {
        Ok(self.servers.read().unwrap().get(id).cloned())
    }

    async fn get_by_server_id(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Option<InstalledServer>> {
        Ok(self
            .servers
            .read()
            .unwrap()
            .values()
            .find(|s| s.space_id == space_id && s.server_id == server_id)
            .cloned())
    }

    async fn install(&self, server: &InstalledServer) -> RepoResult<()> {
        self.servers
            .write()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }

    async fn update(&self, server: &InstalledServer) -> RepoResult<()> {
        self.servers
            .write()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }

    async fn uninstall(&self, id: &Uuid) -> RepoResult<()> {
        self.servers.write().unwrap().remove(id);
        Ok(())
    }

    async fn list_enabled(&self, space_id: &str) -> RepoResult<Vec<InstalledServer>> {
        Ok(self
            .servers
            .read()
            .unwrap()
            .values()
            .filter(|s| s.space_id == space_id && s.enabled)
            .cloned()
            .collect())
    }

    async fn list_enabled_all(&self) -> RepoResult<Vec<InstalledServer>> {
        Ok(self
            .servers
            .read()
            .unwrap()
            .values()
            .filter(|s| s.enabled)
            .cloned()
            .collect())
    }

    async fn set_enabled(&self, id: &Uuid, enabled: bool) -> RepoResult<()> {
        if let Some(server) = self.servers.write().unwrap().get_mut(id) {
            server.enabled = enabled;
        }
        Ok(())
    }

    async fn set_oauth_connected(&self, id: &Uuid, connected: bool) -> RepoResult<()> {
        if let Some(server) = self.servers.write().unwrap().get_mut(id) {
            server.oauth_connected = connected;
        }
        Ok(())
    }

    async fn update_inputs(
        &self,
        id: &Uuid,
        input_values: HashMap<String, String>,
    ) -> RepoResult<()> {
        if let Some(server) = self.servers.write().unwrap().get_mut(id) {
            server.input_values = input_values;
        }
        Ok(())
    }

    async fn update_cached_definition(
        &self,
        id: &Uuid,
        server_name: Option<String>,
        cached_definition: Option<String>,
    ) -> RepoResult<()> {
        if let Some(server) = self.servers.write().unwrap().get_mut(id) {
            if let Some(name) = server_name {
                server.server_name = Some(name);
            }
            server.cached_definition = cached_definition;
        }
        Ok(())
    }
}

// ============================================================================
// MockServerFeatureRepository
// ============================================================================

#[derive(Default)]
pub struct MockServerFeatureRepository {
    features: RwLock<HashMap<Uuid, ServerFeature>>,
}

impl MockServerFeatureRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_feature(self, feature: ServerFeature) -> Self {
        self.features.write().unwrap().insert(feature.id, feature);
        self
    }
}

#[async_trait]
impl ServerFeatureRepository for MockServerFeatureRepository {
    async fn list_for_space(&self, space_id: &str) -> RepoResult<Vec<ServerFeature>> {
        Ok(self
            .features
            .read()
            .unwrap()
            .values()
            .filter(|f| f.space_id == space_id)
            .cloned()
            .collect())
    }

    async fn list_for_server(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Vec<ServerFeature>> {
        Ok(self
            .features
            .read()
            .unwrap()
            .values()
            .filter(|f| f.space_id == space_id && f.server_id == server_id)
            .cloned()
            .collect())
    }

    async fn get(&self, id: &Uuid) -> RepoResult<Option<ServerFeature>> {
        Ok(self.features.read().unwrap().get(id).cloned())
    }

    async fn upsert(&self, feature: &ServerFeature) -> RepoResult<()> {
        self.features
            .write()
            .unwrap()
            .insert(feature.id, feature.clone());
        Ok(())
    }

    async fn upsert_many(&self, features: &[ServerFeature]) -> RepoResult<()> {
        let mut map = self.features.write().unwrap();
        for feature in features {
            map.insert(feature.id, feature.clone());
        }
        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> RepoResult<()> {
        self.features.write().unwrap().remove(id);
        Ok(())
    }

    async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> RepoResult<()> {
        let mut map = self.features.write().unwrap();
        for feature in map.values_mut() {
            if feature.space_id == space_id && feature.server_id == server_id {
                feature.is_available = false;
            }
        }
        Ok(())
    }

    async fn delete_for_server(&self, space_id: &str, server_id: &str) -> RepoResult<()> {
        let mut map = self.features.write().unwrap();
        map.retain(|_, f| !(f.space_id == space_id && f.server_id == server_id));
        Ok(())
    }
}

// ============================================================================
// MockFeatureSetRepository
// ============================================================================

#[derive(Default)]
pub struct MockFeatureSetRepository {
    sets: RwLock<HashMap<String, FeatureSet>>,
    members: RwLock<HashMap<String, Vec<FeatureSetMember>>>,
}

impl MockFeatureSetRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_set(self, set: FeatureSet) -> Self {
        self.sets.write().unwrap().insert(set.id.clone(), set);
        self
    }
}

#[async_trait]
impl FeatureSetRepository for MockFeatureSetRepository {
    async fn list(&self) -> RepoResult<Vec<FeatureSet>> {
        Ok(self.sets.read().unwrap().values().cloned().collect())
    }

    async fn list_by_space(&self, space_id: &str) -> RepoResult<Vec<FeatureSet>> {
        Ok(self
            .sets
            .read()
            .unwrap()
            .values()
            .filter(|s| s.space_id.as_deref() == Some(space_id))
            .cloned()
            .collect())
    }

    async fn get(&self, id: &str) -> RepoResult<Option<FeatureSet>> {
        Ok(self.sets.read().unwrap().get(id).cloned())
    }

    async fn get_with_members(&self, id: &str) -> RepoResult<Option<FeatureSet>> {
        let mut set = self.sets.read().unwrap().get(id).cloned();
        if let Some(ref mut s) = set {
            s.members = self
                .members
                .read()
                .unwrap()
                .get(id)
                .cloned()
                .unwrap_or_default();
        }
        Ok(set)
    }

    async fn create(&self, feature_set: &FeatureSet) -> RepoResult<()> {
        self.sets
            .write()
            .unwrap()
            .insert(feature_set.id.clone(), feature_set.clone());
        // Also store members if present (for tests that pre-populate members)
        if !feature_set.members.is_empty() {
            self.members
                .write()
                .unwrap()
                .insert(feature_set.id.clone(), feature_set.members.clone());
        }
        Ok(())
    }

    async fn update(&self, feature_set: &FeatureSet) -> RepoResult<()> {
        self.sets
            .write()
            .unwrap()
            .insert(feature_set.id.clone(), feature_set.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> RepoResult<()> {
        self.sets.write().unwrap().remove(id);
        self.members.write().unwrap().remove(id);
        Ok(())
    }

    async fn list_builtin(&self, space_id: &str) -> RepoResult<Vec<FeatureSet>> {
        Ok(self
            .sets
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.space_id.as_deref() == Some(space_id)
                    && matches!(
                        s.feature_set_type,
                        FeatureSetType::All | FeatureSetType::Default
                    )
            })
            .cloned()
            .collect())
    }

    async fn get_server_all(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Option<FeatureSet>> {
        Ok(self
            .sets
            .read()
            .unwrap()
            .values()
            .find(|s| {
                s.space_id.as_deref() == Some(space_id)
                    && s.feature_set_type == FeatureSetType::ServerAll
                    && s.server_id.as_deref() == Some(server_id)
            })
            .cloned())
    }

    async fn ensure_server_all(
        &self,
        space_id: &str,
        server_id: &str,
        server_name: &str,
    ) -> RepoResult<FeatureSet> {
        if let Some(existing) = self.get_server_all(space_id, server_id).await? {
            return Ok(existing);
        }
        let set = FeatureSet::new_server_all(space_id, server_id, server_name);
        self.create(&set).await?;
        Ok(set)
    }

    async fn get_default_for_space(&self, space_id: &str) -> RepoResult<Option<FeatureSet>> {
        Ok(self
            .sets
            .read()
            .unwrap()
            .values()
            .find(|s| {
                s.space_id.as_deref() == Some(space_id)
                    && s.feature_set_type == FeatureSetType::Default
            })
            .cloned())
    }

    async fn get_all_for_space(&self, space_id: &str) -> RepoResult<Option<FeatureSet>> {
        Ok(self
            .sets
            .read()
            .unwrap()
            .values()
            .find(|s| {
                s.space_id.as_deref() == Some(space_id) && s.feature_set_type == FeatureSetType::All
            })
            .cloned())
    }

    async fn ensure_builtin_for_space(&self, space_id: &str) -> RepoResult<()> {
        if self.get_all_for_space(space_id).await?.is_none() {
            self.create(&FeatureSet::new_all(space_id)).await?;
        }
        if self.get_default_for_space(space_id).await?.is_none() {
            self.create(&FeatureSet::new_default(space_id)).await?;
        }
        Ok(())
    }

    async fn delete_server_all(&self, space_id: &str, server_id: &str) -> RepoResult<()> {
        if let Some(set) = self.get_server_all(space_id, server_id).await? {
            self.delete(&set.id).await?;
        }
        Ok(())
    }

    async fn add_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
        mode: MemberMode,
    ) -> RepoResult<()> {
        let member = FeatureSetMember {
            id: Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::Feature,
            member_id: feature_id.to_string(),
            mode,
        };
        self.members
            .write()
            .unwrap()
            .entry(feature_set_id.to_string())
            .or_default()
            .push(member);
        Ok(())
    }

    async fn remove_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
    ) -> RepoResult<()> {
        if let Some(members) = self.members.write().unwrap().get_mut(feature_set_id) {
            members
                .retain(|m| !(m.member_type == MemberType::Feature && m.member_id == feature_id));
        }
        Ok(())
    }

    async fn get_feature_members(&self, feature_set_id: &str) -> RepoResult<Vec<FeatureSetMember>> {
        Ok(self
            .members
            .read()
            .unwrap()
            .get(feature_set_id)
            .cloned()
            .unwrap_or_default())
    }
}

// ============================================================================
// MockInboundMcpClientRepository
// ============================================================================

#[derive(Default)]
pub struct MockInboundMcpClientRepository {
    clients: RwLock<HashMap<Uuid, Client>>,
    grants: RwLock<HashMap<(Uuid, String), Vec<String>>>, // (client_id, space_id) -> feature_set_ids
}

impl MockInboundMcpClientRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_client(self, client: Client) -> Self {
        self.clients.write().unwrap().insert(client.id, client);
        self
    }
}

#[async_trait]
impl InboundMcpClientRepository for MockInboundMcpClientRepository {
    async fn list(&self) -> RepoResult<Vec<Client>> {
        Ok(self.clients.read().unwrap().values().cloned().collect())
    }

    async fn get(&self, id: &Uuid) -> RepoResult<Option<Client>> {
        Ok(self.clients.read().unwrap().get(id).cloned())
    }

    async fn get_by_access_key(&self, key: &str) -> RepoResult<Option<Client>> {
        Ok(self
            .clients
            .read()
            .unwrap()
            .values()
            .find(|c| c.access_key.as_deref() == Some(key))
            .cloned())
    }

    async fn create(&self, client: &Client) -> RepoResult<()> {
        self.clients
            .write()
            .unwrap()
            .insert(client.id, client.clone());
        Ok(())
    }

    async fn update(&self, client: &Client) -> RepoResult<()> {
        self.clients
            .write()
            .unwrap()
            .insert(client.id, client.clone());
        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> RepoResult<()> {
        self.clients.write().unwrap().remove(id);
        Ok(())
    }

    async fn grant_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> RepoResult<()> {
        self.grants
            .write()
            .unwrap()
            .entry((*client_id, space_id.to_string()))
            .or_default()
            .push(feature_set_id.to_string());
        Ok(())
    }

    async fn revoke_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> RepoResult<()> {
        if let Some(sets) = self
            .grants
            .write()
            .unwrap()
            .get_mut(&(*client_id, space_id.to_string()))
        {
            sets.retain(|s| s != feature_set_id);
        }
        Ok(())
    }

    async fn get_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> RepoResult<Vec<String>> {
        Ok(self
            .grants
            .read()
            .unwrap()
            .get(&(*client_id, space_id.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    async fn get_all_grants(&self, client_id: &Uuid) -> RepoResult<HashMap<String, Vec<String>>> {
        let grants = self.grants.read().unwrap();
        let mut result = HashMap::new();
        for ((cid, space_id), sets) in grants.iter() {
            if cid == client_id {
                result.insert(space_id.clone(), sets.clone());
            }
        }
        Ok(result)
    }

    async fn set_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> RepoResult<()> {
        self.grants
            .write()
            .unwrap()
            .insert((*client_id, space_id.to_string()), feature_set_ids.to_vec());
        Ok(())
    }

    async fn has_grants_for_space(&self, client_id: &Uuid, space_id: &str) -> RepoResult<bool> {
        Ok(self
            .grants
            .read()
            .unwrap()
            .get(&(*client_id, space_id.to_string()))
            .map(|v| !v.is_empty())
            .unwrap_or(false))
    }
}

// ============================================================================
// MockCredentialRepository
// ============================================================================

#[derive(Default)]
pub struct MockCredentialRepository {
    credentials: RwLock<HashMap<(Uuid, String, String), Credential>>,
}

impl MockCredentialRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_credential(self, cred: Credential) -> Self {
        self.credentials.write().unwrap().insert(
            (
                cred.space_id,
                cred.server_id.clone(),
                cred.credential_type.as_str().to_string(),
            ),
            cred,
        );
        self
    }
}

#[async_trait]
impl CredentialRepository for MockCredentialRepository {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> RepoResult<Option<Credential>> {
        Ok(self
            .credentials
            .read()
            .unwrap()
            .get(&(
                *space_id,
                server_id.to_string(),
                credential_type.as_str().to_string(),
            ))
            .cloned())
    }

    async fn get_all(&self, space_id: &Uuid, server_id: &str) -> RepoResult<Vec<Credential>> {
        Ok(self
            .credentials
            .read()
            .unwrap()
            .values()
            .filter(|c| c.space_id == *space_id && c.server_id == server_id)
            .cloned()
            .collect())
    }

    async fn save(&self, credential: &Credential) -> RepoResult<()> {
        self.credentials.write().unwrap().insert(
            (
                credential.space_id,
                credential.server_id.clone(),
                credential.credential_type.as_str().to_string(),
            ),
            credential.clone(),
        );
        Ok(())
    }

    async fn delete(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> RepoResult<()> {
        self.credentials.write().unwrap().remove(&(
            *space_id,
            server_id.to_string(),
            credential_type.as_str().to_string(),
        ));
        Ok(())
    }

    async fn delete_all(&self, space_id: &Uuid, server_id: &str) -> RepoResult<()> {
        self.credentials
            .write()
            .unwrap()
            .retain(|k, _| !(k.0 == *space_id && k.1 == server_id));
        Ok(())
    }

    async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> RepoResult<bool> {
        let mut creds = self.credentials.write().unwrap();
        let before = creds.len();
        creds.retain(|_, c| {
            !(c.space_id == *space_id && c.server_id == server_id && c.credential_type.is_oauth())
        });
        Ok(creds.len() < before)
    }

    async fn list_for_space(&self, space_id: &Uuid) -> RepoResult<Vec<Credential>> {
        Ok(self
            .credentials
            .read()
            .unwrap()
            .values()
            .filter(|c| c.space_id == *space_id)
            .cloned()
            .collect())
    }
}

// ============================================================================
// MockOutboundOAuthRepository
// ============================================================================

#[derive(Default)]
pub struct MockOutboundOAuthRepository {
    registrations: RwLock<HashMap<(Uuid, String), OutboundOAuthRegistration>>,
}

impl MockOutboundOAuthRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_registration(self, reg: OutboundOAuthRegistration) -> Self {
        self.registrations
            .write()
            .unwrap()
            .insert((reg.space_id, reg.server_id.clone()), reg);
        self
    }
}

#[async_trait]
impl OutboundOAuthRepository for MockOutboundOAuthRepository {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
    ) -> RepoResult<Option<OutboundOAuthRegistration>> {
        Ok(self
            .registrations
            .read()
            .unwrap()
            .get(&(*space_id, server_id.to_string()))
            .cloned())
    }

    async fn save(&self, registration: &OutboundOAuthRegistration) -> RepoResult<()> {
        self.registrations.write().unwrap().insert(
            (registration.space_id, registration.server_id.clone()),
            registration.clone(),
        );
        Ok(())
    }

    async fn delete(&self, space_id: &Uuid, server_id: &str) -> RepoResult<()> {
        self.registrations
            .write()
            .unwrap()
            .remove(&(*space_id, server_id.to_string()));
        Ok(())
    }

    async fn list_for_space(&self, space_id: &Uuid) -> RepoResult<Vec<OutboundOAuthRegistration>> {
        Ok(self
            .registrations
            .read()
            .unwrap()
            .values()
            .filter(|r| r.space_id == *space_id)
            .cloned()
            .collect())
    }
}

// ============================================================================
// MockAppSettingsRepository
// ============================================================================

#[derive(Default)]
pub struct MockAppSettingsRepository {
    settings: RwLock<HashMap<String, String>>,
}

impl MockAppSettingsRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_setting(self, key: &str, value: &str) -> Self {
        self.settings
            .write()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        self
    }
}

#[async_trait]
impl AppSettingsRepository for MockAppSettingsRepository {
    async fn get(&self, key: &str) -> RepoResult<Option<String>> {
        Ok(self.settings.read().unwrap().get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> RepoResult<()> {
        self.settings
            .write()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn delete(&self, key: &str) -> RepoResult<()> {
        self.settings.write().unwrap().remove(key);
        Ok(())
    }

    async fn list(&self) -> RepoResult<Vec<(String, String)>> {
        Ok(self
            .settings
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn list_by_prefix(&self, prefix: &str) -> RepoResult<Vec<(String, String)>> {
        Ok(self
            .settings
            .read()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
}

// ============================================================================
// Test Helper: Create all mock repos as Arc
// ============================================================================

use std::sync::Arc;

/// Collection of all mock repositories for test setup
pub struct MockRepositories {
    pub spaces: Arc<MockSpaceRepository>,
    pub installed_servers: Arc<MockInstalledServerRepository>,
    pub features: Arc<MockServerFeatureRepository>,
    pub feature_sets: Arc<MockFeatureSetRepository>,
    pub clients: Arc<MockInboundMcpClientRepository>,
    pub credentials: Arc<MockCredentialRepository>,
    pub oauth: Arc<MockOutboundOAuthRepository>,
    pub settings: Arc<MockAppSettingsRepository>,
}

impl MockRepositories {
    /// Create a fresh set of empty mock repositories
    pub fn new() -> Self {
        Self {
            spaces: Arc::new(MockSpaceRepository::new()),
            installed_servers: Arc::new(MockInstalledServerRepository::new()),
            features: Arc::new(MockServerFeatureRepository::new()),
            feature_sets: Arc::new(MockFeatureSetRepository::new()),
            clients: Arc::new(MockInboundMcpClientRepository::new()),
            credentials: Arc::new(MockCredentialRepository::new()),
            oauth: Arc::new(MockOutboundOAuthRepository::new()),
            settings: Arc::new(MockAppSettingsRepository::new()),
        }
    }
}

impl Default for MockRepositories {
    fn default() -> Self {
        Self::new()
    }
}
