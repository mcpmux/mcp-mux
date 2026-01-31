# Dependency Injection Pattern

## Overview

McpMux Gateway uses **Constructor Injection** with a **Builder Pattern** for clean, testable, and flexible dependency management.

## Pattern Implementation

### 1. Dependencies Container

**Location:** `server/dependencies.rs`

```rust
/// All external dependencies needed by the Gateway
#[derive(Clone)]
pub struct GatewayDependencies {
    // Data Layer (Repositories)
    pub installed_server_repo: Arc<dyn InstalledServerRepository>,
    pub credential_repo: Arc<dyn CredentialRepository>,
    pub backend_oauth_repo: Arc<dyn BackendOAuthRepository>,
    pub feature_repo: Arc<dyn ServerFeatureRepository>,
    pub feature_set_repo: Arc<dyn FeatureSetRepository>,
    
    // Services
    pub registry_service: Arc<RegistryService>,
    pub log_manager: Arc<ServerLogManager>,
    
    // Infrastructure
    pub database: Arc<Mutex<Database>>,
    pub jwt_secret: Option<Zeroizing<[u8; 32]>>,
}
```

**Key Principles:**
- ✅ All dependencies are **traits** (not concrete types)
- ✅ Dependencies are **explicit** (passed in constructor)
- ✅ Container is **immutable** after creation
- ✅ Container is **cloneable** (Arc for shared ownership)

### 2. Builder Pattern

**Purpose:** Make dependency construction ergonomic and validate required fields.

```rust
pub struct DependenciesBuilder {
    installed_server_repo: Option<Arc<dyn InstalledServerRepository>>,
    credential_repo: Option<Arc<dyn CredentialRepository>>,
    // ... other fields
}

impl DependenciesBuilder {
    pub fn new() -> Self { /* ... */ }
    
    pub fn installed_server_repo(mut self, repo: Arc<dyn InstalledServerRepository>) -> Self {
        self.installed_server_repo = Some(repo);
        self
    }
    
    // ... other builder methods
    
    pub fn build(self) -> Result<GatewayDependencies, String> {
        Ok(GatewayDependencies {
            installed_server_repo: self.installed_server_repo
                .ok_or("installed_server_repo is required")?,
            // ... validate all required fields
        })
    }
}
```

**Benefits:**
- ✅ Fluent API (chainable methods)
- ✅ Compile-time safety (required fields checked at build())
- ✅ Clear error messages for missing dependencies
- ✅ Optional fields supported (jwt_secret)

### 3. Service Initialization

**Location:** `server/service_container.rs`

```rust
pub struct ServiceContainer {
    pub pool_services: PoolServices,
    pub server_manager: Arc<ServerManager>,
    pub startup_orchestrator: Arc<StartupOrchestrator>,
}

impl ServiceContainer {
    pub fn initialize(deps: &GatewayDependencies) -> Self {
        // Create all services from dependencies
        let pool_services = ServiceFactory::create_pool_services(deps);
        
        let startup_orchestrator = Arc::new(StartupOrchestrator::new(
            pool_services.pool_service.clone(),
            deps.clone(),
        ));
        
        Self {
            pool_services,
            server_manager: pool_services.server_manager.clone(),
            startup_orchestrator,
        }
    }
}
```

**Key Points:**
- ✅ Single initialization method
- ✅ All services created in one place
- ✅ Dependencies wired automatically
- ✅ No hidden dependencies

### 4. Service Factory (DRY)

**Location:** `pool/service_factory.rs`

```rust
pub struct ServiceFactory;

impl ServiceFactory {
    pub fn create_pool_services(deps: &GatewayDependencies) -> PoolServices {
        // TokenService
        let token_service = Arc::new(TokenService::new(
            deps.credential_repo.clone(),
            deps.backend_oauth_repo.clone(),
        ));
        
        // BackendOAuthManager
        let oauth_manager = Arc::new(
            BackendOAuthManager::new()
                .with_log_manager(deps.log_manager.clone())
        );
        
        // ConnectionService
        let connection_service = Arc::new(
            ConnectionService::new(
                token_service.clone(),
                oauth_manager.clone(),
                deps.credential_repo.clone(),
                deps.backend_oauth_repo.clone(),
            )
            .with_log_manager(deps.log_manager.clone())
        );
        
        // FeatureService
        let feature_service = Arc::new(FeatureService::new(
            deps.feature_repo.clone(),
            deps.feature_set_repo.clone(),
        ));
        
        // PoolService
        let pool_service = Arc::new(PoolService::new(
            connection_service.clone(),
            feature_service.clone(),
            token_service.clone(),
        ));
        
        // ServerManager
        let server_manager = Arc::new(ServerManager::new(
            pool_service.clone(),
            feature_service.clone(),
            connection_service.clone(),
        ));
        
        PoolServices {
            pool_service,
            connection_service,
            feature_service,
            token_service,
            oauth_manager,
            server_manager,
        }
    }
}
```

**Benefits:**
- ✅ DRY - single place for service wiring
- ✅ Consistent initialization (Desktop, CLI, tests)
- ✅ Easy to refactor (change wiring in one place)

## Usage Examples

### Desktop App (Tauri)

```rust
// apps/desktop/src-tauri/src/commands/gateway.rs

fn create_gateway_dependencies(app_state: &AppState) 
    -> Result<GatewayDependencies, String> 
{
    // Load JWT secret from keychain
    let jwt_secret = KeychainJwtSecretProvider::new()?
        .get_or_create_secret()?;
    
    // Build dependencies
    let dependencies = DependenciesBuilder::new()
        .installed_server_repo(app_state.installed_server_repository.clone())
        .credential_repo(app_state.credential_repository.clone())
        .backend_oauth_repo(app_state.backend_oauth_repository.clone())
        .feature_repo(app_state.server_feature_repository_core.clone())
        .feature_set_repo(app_state.feature_set_repository.clone())
        .registry_service(app_state.registry_service.clone())
        .log_manager(app_state.server_log_manager.clone())
        .database(app_state.database())
        .jwt_secret(jwt_secret)
        .build()?;
    
    Ok(dependencies)
}

#[tauri::command]
pub async fn start_gateway(app_state: State<'_, AppState>) 
    -> Result<String, String> 
{
    let dependencies = create_gateway_dependencies(&app_state)?;
    
    let config = GatewayConfig {
        host: "127.0.0.1".to_string(),
        port: 3100,
        enable_cors: true,
    };
    
    let server = GatewayServer::new(config, dependencies);
    let handle = server.spawn();
    
    Ok("http://127.0.0.1:3100".to_string())
}
```

### Future CLI

```rust
// Future: crates/mcpmux-cli/src/main.rs

fn main() -> Result<()> {
    let args = CliArgs::parse();
    
    // Initialize database
    let db = Database::open(&args.data_dir)?;
    let db = Arc::new(Mutex::new(db));
    
    // Create repositories
    let installed_server_repo = Arc::new(
        InstalledServerRepositoryImpl::new(db.clone())
    ) as Arc<dyn InstalledServerRepository>;
    
    let credential_repo = Arc::new(
        CredentialRepositoryImpl::new(db.clone())
    ) as Arc<dyn CredentialRepository>;
    
    // ... create other repos
    
    // Build dependencies (SAME pattern as Desktop!)
    let dependencies = DependenciesBuilder::new()
        .installed_server_repo(installed_server_repo)
        .credential_repo(credential_repo)
        .backend_oauth_repo(backend_oauth_repo)
        .feature_repo(feature_repo)
        .feature_set_repo(feature_set_repo)
        .registry_service(registry_service)
        .log_manager(log_manager)
        .database(db)
        .build()?;
    
    // Create and run gateway (SAME API as Desktop!)
    let config = GatewayConfig {
        host: args.host,
        port: args.port,
        enable_cors: args.cors,
    };
    
    let server = GatewayServer::new(config, dependencies);
    
    tokio::runtime::Runtime::new()?
        .block_on(server.run())?;
    
    Ok(())
}
```

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // Mock repository for testing
    struct MockInstalledServerRepo;
    
    impl InstalledServerRepository for MockInstalledServerRepo {
        async fn list(&self) -> Result<Vec<InstalledServer>> {
            Ok(vec![/* test data */])
        }
        // ... implement other methods
    }
    
    #[tokio::test]
    async fn test_gateway_with_mocks() {
        // Create mock dependencies
        let mock_repo = Arc::new(MockInstalledServerRepo) 
            as Arc<dyn InstalledServerRepository>;
        
        let dependencies = DependenciesBuilder::new()
            .installed_server_repo(mock_repo)
            // ... inject other mocks
            .build()
            .unwrap();
        
        let config = GatewayConfig {
            host: "127.0.0.1".to_string(),
            port: 0, // random port
            enable_cors: false,
        };
        
        let server = GatewayServer::new(config, dependencies);
        
        // Test server behavior...
    }
}
```

## Benefits of This Pattern

### 1. **Testability** ✅
- Easy to inject mocks/fakes for testing
- No hidden dependencies
- Each component testable in isolation

### 2. **Flexibility** ✅
- Can swap implementations without changing code
- Different configs for Desktop vs CLI vs tests
- Easy to add new dependencies

### 3. **Clarity** ✅
- Dependencies are explicit in constructor
- No magic/hidden initialization
- Clear dependency graph

### 4. **Maintainability** ✅
- Single place to change wiring (ServiceFactory)
- Compile-time safety (missing deps = compile error)
- Easy to refactor

### 5. **CLI-Ready** ✅
- Same pattern works for Desktop and CLI
- No business logic changes needed
- Just wire up dependencies differently

## Anti-Patterns to Avoid

### ❌ Service Locator
```rust
// BAD - hidden dependencies
impl MyService {
    fn new() -> Self {
        let repo = ServiceLocator::get::<Repository>(); // Hidden!
        Self { repo }
    }
}
```

### ❌ Global State
```rust
// BAD - global mutable state
static mut REPO: Option<Arc<Repository>> = None;

impl MyService {
    fn new() -> Self {
        let repo = unsafe { REPO.as_ref().unwrap() }; // Unsafe!
        Self { repo }
    }
}
```

### ❌ Concrete Dependencies
```rust
// BAD - depends on concrete type
impl MyService {
    fn new(repo: SqliteRepository) -> Self { // Can't test with mock!
        Self { repo }
    }
}
```

### ✅ Correct Pattern
```rust
// GOOD - trait dependency, explicit injection
impl MyService {
    fn new(repo: Arc<dyn Repository>) -> Self {
        Self { repo }
    }
}
```

## Dependency Graph

```
GatewayServer
├─ ServiceContainer
│  ├─ PoolServices (from ServiceFactory)
│  │  ├─ TokenService
│  │  │  ├─ CredentialRepository
│  │  │  └─ BackendOAuthRepository
│  │  ├─ BackendOAuthManager
│  │  │  └─ ServerLogManager
│  │  ├─ ConnectionService
│  │  │  ├─ TokenService
│  │  │  ├─ BackendOAuthManager
│  │  │  ├─ CredentialRepository
│  │  │  ├─ BackendOAuthRepository
│  │  │  └─ ServerLogManager
│  │  ├─ FeatureService
│  │  │  ├─ ServerFeatureRepository
│  │  │  └─ FeatureSetRepository
│  │  ├─ PoolService
│  │  │  ├─ ConnectionService
│  │  │  ├─ FeatureService
│  │  │  └─ TokenService
│  │  └─ ServerManager
│  │     ├─ PoolService
│  │     ├─ FeatureService
│  │     └─ ConnectionService
│  └─ StartupOrchestrator
│     ├─ PoolService
│     └─ GatewayDependencies
└─ GatewayState (runtime state, not DI)
```

## Summary

| Aspect | Implementation |
|--------|----------------|
| **Pattern** | Constructor Injection + Builder |
| **Container** | `GatewayDependencies` (immutable) |
| **Builder** | `DependenciesBuilder` (fluent API) |
| **Factory** | `ServiceFactory` (DRY wiring) |
| **Dependencies** | Traits (not concrete types) |
| **Lifetime** | `Arc<dyn Trait>` (shared ownership) |
| **Validation** | At `build()` time |
| **Testing** | Easy (inject mocks) |
| **CLI Support** | Ready (same pattern) |

---

**Last Updated:** 2025-12-20
**Version:** 1.0


