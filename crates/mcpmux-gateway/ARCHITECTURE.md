# McpMux Gateway Architecture

## Overview

McpMux Gateway is a local MCP (Model Context Protocol) proxy server that:
- Aggregates multiple MCP servers into a single endpoint
- Manages OAuth 2.1 authentication for remote servers
- Handles automatic token refresh
- Provides server connection pooling and lifecycle management
- Logs all operations for debugging

## Core Design Principles

### 1. **Pool-Based Connection Management** ✅

**Decision:** Use `PoolService` for active connection management, NOT lazy loading.

**Rationale:**
- MCP servers need to be connected upfront to discover their capabilities (tools, prompts, resources)
- Clients need to know what's available before making calls
- Auto-connect on startup provides better UX (servers are ready immediately)
- Token refresh can happen proactively, not reactively

**Architecture:**
```
PoolService
├─ ServerInstance (per server)
│  ├─ McpClient (active connection)
│  ├─ DiscoveredFeatures (tools, prompts, resources)
│  └─ InstanceState (Connecting, Connected, Disconnected, Failed)
├─ ConnectionService (connect/disconnect logic)
├─ TokenService (OAuth token management & auto-refresh)
└─ FeatureService (feature discovery & caching)
```

**Rejected Alternative:** LazyLoadingManager (on-demand connection)
- Would require clients to wait for connection on first tool call
- Harder to handle OAuth flows (user might not be present)
- No benefit since we need features upfront anyway

### 2. **Dependency Injection Pattern** ✅

**Decision:** Use constructor injection with builder pattern.

**Implementation:**
```rust
// Clean DI - all dependencies explicit
let dependencies = DependenciesBuilder::new()
    .installed_server_repo(repo)
    .credential_repo(cred_repo)
    .backend_oauth_repo(oauth_repo)
    .feature_repo(feature_repo)
    .feature_set_repo(feature_set_repo)
    .registry_service(registry)
    .log_manager(log_mgr)
    .database(db)
    .build()?;

let server = GatewayServer::new(config, dependencies);
```

**Benefits:**
- Testable (can inject mocks)
- Flexible (works with any implementation)
- Clear dependencies (explicit, not hidden)
- CLI-ready (same pattern for Desktop and future CLI)

**Location:** `server/dependencies.rs` (single source of truth)

### 3. **Event-Driven Server State** ✅

**Decision:** Use `ServerManager` for event-driven connection orchestration.

**Features:**
- Broadcasts events to UI (connecting, connected, failed, disconnected)
- Tracks per-server state independently
- Non-blocking operations (all async)
- Automatic feature refresh on reconnect

**Events:**
```rust
pub enum ServerEvent {
    Connecting { space_id, server_id },
    Connected { space_id, server_id, features },
    Disconnected { space_id, server_id },
    Failed { space_id, server_id, error },
    FeaturesRefreshed { space_id, server_id, added, removed },
}
```

### 4. **Automatic Token Refresh** ✅

**Decision:** Token refresh handled automatically by RMCP's `AuthClient` with `DatabaseCredentialStore`.

**Implementation:**
```rust
// HttpTransport uses RMCP's AuthClient which automatically:
// 1. Calls CredentialStore.load() to get stored tokens
// 2. Checks token expiration
// 3. Refreshes automatically if expired (via CredentialStore)
// 4. Saves refreshed tokens back to database

let credential_store = DatabaseCredentialStore::new(
    space_id, server_id, server_url,
    credential_repo, backend_oauth_repo,
);
auth_manager.set_credential_store(credential_store);
let auth_client = AuthClient::new(reqwest::Client::default(), auth_manager);
```

**Benefits:**
- Transparent to callers - handled per-request by RMCP
- No preemptive refresh logic needed
- Uses refresh tokens efficiently

### 5. **Comprehensive Logging** ✅

**Decision:** Log all operations to disk per server.

**Log Sources:**
```rust
pub enum LogSource {
    Connection,  // connect, disconnect, reconnect
    OAuth,       // token fetch, refresh, errors
    Transport,   // stdio/HTTP transport events
    MCP,         // MCP protocol messages
    Feature,     // feature discovery
}
```

**Storage:** `~/.mcpmux/logs/{space_id}/{server_id}/`

**Access:** `ServerLogManager::read_logs()` for UI display

## Module Structure

```
mcpmux-gateway/
├── lib.rs                    # Public API exports
├── server/                   # Gateway server (HTTP/MCP endpoint)
│   ├── dependencies.rs       # DI container (GatewayDependencies)
│   ├── service_container.rs  # Service initialization
│   ├── startup.rs            # Auto-connect orchestration
│   ├── state.rs              # Gateway state (sessions, OAuth)
│   ├── handlers.rs           # HTTP/MCP request handlers
│   └── mod.rs                # GatewayServer (main entry point)
├── pool/                     # Connection pool & lifecycle
│   ├── service.rs            # PoolService (main orchestrator)
│   ├── connection.rs         # ConnectionService (connect/disconnect)
│   ├── token.rs              # TokenService (OAuth token management)
│   ├── feature.rs            # FeatureService (feature discovery)
│   ├── server_manager.rs     # ServerManager (event-driven state)
│   ├── instance.rs           # ServerInstance (per-server state)
│   ├── oauth.rs              # BackendOAuthManager (OAuth flows)
│   ├── routing.rs            # RoutingService (tool call routing)
│   ├── service_factory.rs    # ServiceFactory (DRY initialization)
│   └── transport/            # Transport implementations
│       ├── stdio.rs          # STDIO transport
│       ├── http.rs           # HTTP transport
│       └── mod.rs            # Transport trait & factory
├── oauth/                    # OAuth 2.1 implementation
│   ├── discovery.rs          # RFC 8414 discovery
│   ├── dcr.rs                # RFC 7591 dynamic client registration
│   ├── flow.rs               # Authorization code flow
│   ├── pkce.rs               # PKCE (RFC 7636)
│   └── token.rs              # Token types
├── auth/                     # Access key authentication
└── permissions/              # FeatureSet filtering
```

## Data Flow

### Startup Flow
```
1. Desktop/CLI creates GatewayDependencies
2. GatewayServer::new(config, dependencies)
   ├─ ServiceContainer::initialize(dependencies)
   │  ├─ ServiceFactory::create_pool_services()
   │  │  ├─ TokenService
   │  │  ├─ BackendOAuthManager
   │  │  ├─ ConnectionService
   │  │  ├─ FeatureService
   │  │  ├─ PoolService
   │  │  └─ ServerManager
   │  └─ StartupOrchestrator::new()
   └─ HTTP server initialized
3. GatewayServer::run()
   ├─ HTTP server starts listening
   └─ tokio::spawn(auto_connect_servers())
       ├─ Load enabled servers from DB
       ├─ For each server:
       │  ├─ Check/refresh OAuth token
       │  ├─ Connect transport
       │  ├─ Discover features
       │  └─ Log everything
       └─ Report results
```

### Tool Call Flow
```
1. Client → Gateway: POST /mcp (tools/call)
2. RoutingService::route_tool_call()
   ├─ Parse tool name (prefix or alias)
   ├─ Resolve to server_id
   ├─ Get ServerInstance from PoolService
   ├─ Call tool via McpClient (AuthClient handles token refresh)
   └─ If auth error: Tell user to reconnect
3. Gateway → Client: Response
```

### Connection Flow
```
1. ConnectionService::connect()
   ├─ Check for stored credentials
   ├─ TransportFactory::create()
   │  ├─ STDIO: spawn child process
   │  └─ HTTP: create with DatabaseCredentialStore
   ├─ Transport::connect()
   │  ├─ Create AuthClient with CredentialStore
   │  ├─ RMCP handles token refresh per-request
   │  └─ MCP initialize handshake
   ├─ FeatureService::discover_features()
   │  ├─ tools/list
   │  ├─ prompts/list
   │  └─ resources/list
   └─ ServerManager::emit(Connected event)
```

## Future CLI Support

The architecture is already CLI-ready:

```rust
// Future: crates/mcpmux-cli/src/main.rs
fn main() -> Result<()> {
    // 1. Parse CLI args
    let args = CliArgs::parse();
    
    // 2. Initialize database & repos (SAME as Desktop)
    let db = Database::open(&args.data_dir)?;
    let repos = initialize_repositories(&db)?;
    
    // 3. Build dependencies (SAME pattern)
    let dependencies = DependenciesBuilder::new()
        .installed_server_repo(repos.installed_server)
        // ... same as Desktop
        .build()?;
    
    // 4. Create & run gateway (SAME API)
    let config = GatewayConfig {
        host: args.host,
        port: args.port,
        enable_cors: args.cors,
    };
    
    let server = GatewayServer::new(config, dependencies);
    tokio::runtime::Runtime::new()?.block_on(server.run())?;
    
    Ok(())
}
```

**No business logic changes needed!**

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Pool-based (not lazy) | Need features upfront, better UX, proactive token refresh |
| Constructor DI | Testable, flexible, CLI-ready |
| Event-driven state | Non-blocking, reactive UI updates |
| Auto token refresh | Transparent, minimizes 401s |
| Per-server logging | Debugging, audit trail, UI display |
| Library crate | Reusable by Desktop and future CLI |

## Performance Characteristics

- **Startup:** O(n) where n = enabled servers (parallel connections)
- **Tool calls:** O(1) lookup in pool, then network call
- **Token refresh:** O(1) per server, cached in memory
- **Feature discovery:** O(1) cached, refreshed on reconnect
- **Memory:** ~1MB per connected server (client + features)

## Security

- **Secrets:** Stored in OS keychain (JWT secret) + encrypted DB (OAuth tokens)
- **Token refresh:** Automatic, uses refresh tokens securely
- **PKCE:** Required for all OAuth flows (S256)
- **Logging:** Sanitizes sensitive data (tokens, secrets)

## Testing Strategy

- **Unit tests:** Individual services with mocked dependencies
- **Integration tests:** Full gateway with test repositories
- **E2E tests:** Real MCP servers (stdio test servers)

---

**Last Updated:** 2025-12-20
**Version:** 1.0

