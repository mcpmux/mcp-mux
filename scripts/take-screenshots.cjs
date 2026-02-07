/**
 * Comprehensive screenshot script with Tauri IPC mocking.
 * Produces realistic light-theme screenshots for the README.
 */
const { chromium } = require('/home/user/mcp-mux/node_modules/.pnpm/playwright-core@1.58.1/node_modules/playwright-core');

const BASE_URL = 'http://localhost:1420';
const DIR = '/home/user/mcp-mux/docs/screenshots';

// ── Mock data ──────────────────────────────────────────────

const SPACE_DEFAULT = {
  id: '00000000-0000-0000-0000-000000000001',
  name: 'My Space',
  icon: '🌐',
  description: null,
  is_default: true,
  sort_order: 0,
  created_at: '2026-01-15T10:00:00Z',
  updated_at: '2026-01-15T10:00:00Z',
};

const SPACE_WORK = {
  id: '00000000-0000-0000-0000-000000000002',
  name: 'Work Projects',
  icon: '💼',
  description: 'Production servers for work',
  is_default: false,
  sort_order: 1,
  created_at: '2026-01-20T10:00:00Z',
  updated_at: '2026-01-20T10:00:00Z',
};

const SPACE_PERSONAL = {
  id: '00000000-0000-0000-0000-000000000003',
  name: 'Personal',
  icon: '🏠',
  description: 'Side projects and experiments',
  is_default: false,
  sort_order: 2,
  created_at: '2026-01-22T10:00:00Z',
  updated_at: '2026-01-22T10:00:00Z',
};

const SPACES = [SPACE_DEFAULT, SPACE_WORK, SPACE_PERSONAL];

function mkInstalled(id, instId, serverId, name, desc, alias, icon, categories, transportType, auth, publisher, caps, inputDefs, inputVals, enabled, oauthConnected) {
  const transport = transportType === 'stdio'
    ? { type: 'stdio', command: 'npx', args: ['-y', `@modelcontextprotocol/server-${alias}`], metadata: { inputs: inputDefs || [] } }
    : { type: 'http', url: `https://mcp.${alias}.com/sse`, headers: {}, metadata: { inputs: inputDefs || [] } };
  return {
    id: instId, space_id: SPACE_DEFAULT.id, server_id: serverId,
    server_name: name, cached_definition: JSON.stringify({
      id: serverId, name, description: desc, alias, icon, categories, transport, auth,
      publisher, capabilities: caps,
    }),
    input_values: inputVals || {}, enabled, env_overrides: {}, args_append: [],
    extra_headers: {}, oauth_connected: oauthConnected || false, source: 'Registry',
    created_at: '2026-01-16T10:00:00Z', updated_at: '2026-01-16T10:00:00Z',
  };
}

const INSTALLED_SERVERS = [
  mkInstalled(1, 'inst-1', 'github-mcp', 'GitHub', 'GitHub API integration — issues, PRs, repos, and code search', 'github', '🐙', ['developer-tools'], 'stdio',
    { type: 'api_key', instructions: null }, { name: 'Anthropic', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [{ id: 'GITHUB_TOKEN', label: 'GitHub Token', type: 'password', required: true, secret: true }], { GITHUB_TOKEN: 'ghp_***' }, true, false),
  mkInstalled(2, 'inst-2', 'atlassian-mcp', 'Atlassian', 'Jira issues, Confluence pages, and project management', 'atlassian', '🔷', ['productivity'], 'http',
    { type: 'oauth' }, { name: 'Atlassian', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [], {}, true, true),
  mkInstalled(3, 'inst-3', 'cloudflare-mcp', 'Cloudflare', 'Manage Workers, KV, R2, and DNS via Cloudflare API', 'cloudflare', '☁️', ['cloud'], 'http',
    { type: 'oauth' }, { name: 'Cloudflare', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [], {}, true, true),
  mkInstalled(4, 'inst-4', 'postgres-mcp', 'PostgreSQL', 'Query databases, inspect schemas, and run SQL', 'postgres', '🐘', ['developer-tools'], 'stdio',
    { type: 'api_key', instructions: null }, { name: 'Anthropic', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [{ id: 'DATABASE_URL', label: 'Database URL', type: 'url', required: true, secret: true }], { DATABASE_URL: 'postgres://***' }, true, false),
  mkInstalled(5, 'inst-5', 'azure-mcp', 'Azure DevOps', 'Azure DevOps work items, repos, and pipelines', 'azure', '🔵', ['cloud'], 'http',
    { type: 'oauth' }, { name: 'Microsoft', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [], {}, true, true),
  mkInstalled(6, 'inst-6', 'filesystem-mcp', 'Filesystem', 'Read, write, and search files on your local machine', 'filesystem', '📂', ['file-system'], 'stdio',
    { type: 'none' }, { name: 'Anthropic', verified: true, official: true }, { tools: true, resources: true, prompts: false },
    [{ id: 'ALLOWED_DIR', label: 'Allowed Directory', type: 'text', required: true, secret: false }], { ALLOWED_DIR: '/home/user/projects' }, true, false),
];

function mkFeature(serverId, serverName, type, name, desc) {
  return { id: `${serverId}-${name}`, space_id: SPACE_DEFAULT.id, server_id: serverId, feature_type: type, feature_name: name, display_name: name, description: desc, input_schema: null, discovered_at: '2026-01-16T10:00:00Z', last_seen_at: '2026-02-07T10:00:00Z', is_available: true };
}

const SERVER_FEATURES = [
  // GitHub
  mkFeature('github-mcp', 'GitHub', 'tool', 'create_issue', 'Create a new GitHub issue'),
  mkFeature('github-mcp', 'GitHub', 'tool', 'search_code', 'Search code across repositories'),
  mkFeature('github-mcp', 'GitHub', 'tool', 'list_pull_requests', 'List pull requests for a repository'),
  mkFeature('github-mcp', 'GitHub', 'tool', 'create_pull_request', 'Create a new pull request'),
  mkFeature('github-mcp', 'GitHub', 'resource', 'repo://contents', 'Repository file contents'),
  // Atlassian
  mkFeature('atlassian-mcp', 'Atlassian', 'tool', 'create_jira_issue', 'Create a Jira issue'),
  mkFeature('atlassian-mcp', 'Atlassian', 'tool', 'search_issues', 'Search Jira issues with JQL'),
  mkFeature('atlassian-mcp', 'Atlassian', 'tool', 'get_confluence_page', 'Fetch a Confluence page'),
  mkFeature('atlassian-mcp', 'Atlassian', 'tool', 'update_issue_status', 'Transition a Jira issue'),
  mkFeature('atlassian-mcp', 'Atlassian', 'resource', 'jira://boards', 'Jira boards listing'),
  // Cloudflare
  mkFeature('cloudflare-mcp', 'Cloudflare', 'tool', 'deploy_worker', 'Deploy a Cloudflare Worker'),
  mkFeature('cloudflare-mcp', 'Cloudflare', 'tool', 'list_kv_namespaces', 'List KV namespaces'),
  mkFeature('cloudflare-mcp', 'Cloudflare', 'tool', 'manage_r2_bucket', 'Manage R2 storage buckets'),
  mkFeature('cloudflare-mcp', 'Cloudflare', 'tool', 'update_dns_record', 'Update DNS records'),
  mkFeature('cloudflare-mcp', 'Cloudflare', 'resource', 'workers://bindings', 'Worker bindings config'),
  // PostgreSQL
  mkFeature('postgres-mcp', 'PostgreSQL', 'tool', 'query', 'Execute a SQL query'),
  mkFeature('postgres-mcp', 'PostgreSQL', 'tool', 'describe_table', 'Get table schema'),
  mkFeature('postgres-mcp', 'PostgreSQL', 'resource', 'schema://tables', 'Database table listing'),
  // Azure DevOps
  mkFeature('azure-mcp', 'Azure DevOps', 'tool', 'create_work_item', 'Create an Azure DevOps work item'),
  mkFeature('azure-mcp', 'Azure DevOps', 'tool', 'list_pipelines', 'List CI/CD pipelines'),
  mkFeature('azure-mcp', 'Azure DevOps', 'tool', 'trigger_pipeline', 'Trigger a pipeline run'),
  mkFeature('azure-mcp', 'Azure DevOps', 'resource', 'repos://list', 'Azure repos listing'),
  // Filesystem
  mkFeature('filesystem-mcp', 'Filesystem', 'tool', 'read_file', 'Read a file from disk'),
  mkFeature('filesystem-mcp', 'Filesystem', 'tool', 'write_file', 'Write content to a file'),
  mkFeature('filesystem-mcp', 'Filesystem', 'tool', 'search_files', 'Search files by pattern'),
  mkFeature('filesystem-mcp', 'Filesystem', 'tool', 'list_directory', 'List directory contents'),
  mkFeature('filesystem-mcp', 'Filesystem', 'resource', 'file://contents', 'File contents'),
];

const SERVER_STATUSES = {
  'github-mcp': { server_id: 'github-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
  'atlassian-mcp': { server_id: 'atlassian-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
  'cloudflare-mcp': { server_id: 'cloudflare-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
  'postgres-mcp': { server_id: 'postgres-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
  'azure-mcp': { server_id: 'azure-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
  'filesystem-mcp': { server_id: 'filesystem-mcp', status: 'connected', flow_id: 1, has_connected_before: true, message: null },
};

const FEATURE_SETS = [
  { id: 'fs-all', name: 'All Features', description: 'Access to all tools, prompts, and resources', icon: null, space_id: null, feature_set_type: 'all', server_id: null, is_builtin: true, is_deleted: false, members: [] },
  { id: 'fs-default', name: 'Default', description: 'Default feature set for new clients', icon: null, space_id: null, feature_set_type: 'default', server_id: null, is_builtin: true, is_deleted: false, members: [] },
  { id: 'fs-readonly', name: 'Read Only', description: 'Only read operations — no writes or deletes', icon: '🔒', space_id: SPACE_DEFAULT.id, feature_set_type: 'custom', server_id: null, is_builtin: false, is_deleted: false, members: [] },
  { id: 'fs-dev', name: 'Dev Tools', description: 'GitHub + PostgreSQL + Filesystem access', icon: '🛠️', space_id: SPACE_DEFAULT.id, feature_set_type: 'custom', server_id: null, is_builtin: false, is_deleted: false, members: [] },
];

const OAUTH_CLIENTS = [
  { client_id: 'cursor-001', registration_type: 'dcr', client_name: 'Cursor', client_alias: null, redirect_uris: ['http://localhost:9315/callback'], scope: null, approved: true, logo_uri: null, client_uri: null, software_id: 'cursor', software_version: '0.45.0', metadata_url: null, metadata_cached_at: null, metadata_cache_ttl: null, connection_mode: 'follow_active', locked_space_id: null, last_seen: '2026-02-07T09:30:00Z', created_at: '2026-01-20T10:00:00Z', has_active_tokens: true },
  { client_id: 'vscode-001', registration_type: 'dcr', client_name: 'VS Code', client_alias: null, redirect_uris: ['http://localhost:9315/callback'], scope: null, approved: true, logo_uri: null, client_uri: null, software_id: 'vscode', software_version: '1.96.0', metadata_url: null, metadata_cached_at: null, metadata_cache_ttl: null, connection_mode: 'follow_active', locked_space_id: null, last_seen: '2026-02-07T08:45:00Z', created_at: '2026-01-22T10:00:00Z', has_active_tokens: true },
];

function mkRegistry(id, name, desc, alias, icon, categories, auth, transportType, publisher, caps, hostingType, badges) {
  const source = { type: 'Registry', url: 'https://registry.mcpmux.com', name: 'Official Registry' };
  const transport = transportType === 'stdio'
    ? { type: 'stdio', command: 'npx', args: ['-y', `@modelcontextprotocol/server-${alias}`], env: {}, metadata: { inputs: [] } }
    : { type: 'http', url: `https://mcp.${alias}.com/sse`, headers: {}, metadata: { inputs: [] } };
  return { id, name, description: desc, alias, icon, categories, auth, publisher, transport, source, capabilities: caps, hosting_type: hostingType || 'local', badges: badges || [], license: 'MIT', installation: { difficulty: 'easy' } };
}

const REGISTRY_SERVERS = [
  // Installed ones (will show "Installed" in discover) — IDs match INSTALLED_SERVERS
  mkRegistry('github-mcp', 'GitHub', 'GitHub API integration — issues, PRs, repos, and code search', 'github', '🐙', ['developer-tools'], { type: 'api_key', instructions: null }, 'stdio', { name: 'Anthropic', domain: 'anthropic.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'local', ['official', 'verified', 'popular']),
  mkRegistry('atlassian-mcp', 'Atlassian', 'Jira issues, Confluence pages, and project management', 'atlassian', '🔷', ['productivity'], { type: 'oauth' }, 'http', { name: 'Atlassian', domain: 'atlassian.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'remote', ['official', 'verified']),
  mkRegistry('cloudflare-mcp', 'Cloudflare', 'Manage Workers, KV, R2, and DNS via Cloudflare API', 'cloudflare', '☁️', ['cloud'], { type: 'oauth' }, 'http', { name: 'Cloudflare', domain: 'cloudflare.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'remote', ['official', 'verified']),
  mkRegistry('postgres-mcp', 'PostgreSQL', 'Query databases, inspect schemas, and run SQL', 'postgres', '🐘', ['developer-tools'], { type: 'api_key', instructions: null }, 'stdio', { name: 'Anthropic', domain: 'anthropic.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'local', ['official', 'popular']),
  mkRegistry('azure-mcp', 'Azure DevOps', 'Azure DevOps work items, repos, and pipelines', 'azure', '🔵', ['cloud'], { type: 'oauth' }, 'http', { name: 'Microsoft', domain: 'microsoft.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'remote', ['official', 'verified']),
  mkRegistry('filesystem-mcp', 'Filesystem', 'Read, write, and search files on your local machine', 'filesystem', '📂', ['file-system'], { type: 'none' }, 'stdio', { name: 'Anthropic', domain: 'anthropic.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'local', ['official']),
  // Not installed — popular servers in the registry
  mkRegistry('slack-mcp', 'Slack', 'Send messages, search channels, and manage Slack workspaces', 'slack', '💬', ['productivity'], { type: 'oauth' }, 'http', { name: 'Slack', domain: 'slack.com', verified: true, official: true }, { tools: true, resources: false, prompts: true }, 'remote', ['official', 'verified', 'popular']),
  mkRegistry('gdrive-mcp', 'Google Drive', 'Search and read files from Google Drive', 'gdrive', '📁', ['cloud'], { type: 'oauth' }, 'stdio', { name: 'Google', domain: 'google.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'remote', ['official', 'popular']),
  mkRegistry('stripe-mcp', 'Stripe', 'Manage payments, subscriptions, and invoices', 'stripe', '💳', ['developer-tools'], { type: 'api_key', instructions: null }, 'http', { name: 'Stripe', domain: 'stripe.com', verified: true, official: true }, { tools: true, resources: false, prompts: false }, 'remote', ['official', 'verified']),
  mkRegistry('linear-mcp', 'Linear', 'Manage issues, projects, and teams in Linear', 'linear', '📐', ['productivity'], { type: 'api_key', instructions: null }, 'http', { name: 'Linear', domain: 'linear.app', verified: true, official: true }, { tools: true, resources: false, prompts: false }, 'remote', ['verified']),
  mkRegistry('sentry-mcp', 'Sentry', 'Search and analyze errors and performance issues', 'sentry', '🔥', ['developer-tools'], { type: 'api_key', instructions: null }, 'stdio', { name: 'Sentry', domain: 'sentry.io', verified: true, official: false }, { tools: true, resources: false, prompts: false }, 'local', ['verified']),
  mkRegistry('notion-mcp', 'Notion', 'Read and search Notion pages and databases', 'notion', '📝', ['productivity'], { type: 'api_key', instructions: null }, 'stdio', { name: 'Notion', domain: 'notion.so', verified: true, official: false }, { tools: true, resources: true, prompts: false }, 'local', ['verified', 'popular']),
  mkRegistry('datadog-mcp', 'Datadog', 'Monitor infrastructure, APM traces, and logs', 'datadog', '🐶', ['developer-tools'], { type: 'api_key', instructions: null }, 'http', { name: 'Datadog', domain: 'datadoghq.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'remote', ['official']),
  mkRegistry('mongodb-mcp', 'MongoDB', 'Query collections, manage indexes, and run aggregations', 'mongodb', '🍃', ['developer-tools'], { type: 'api_key', instructions: null }, 'stdio', { name: 'MongoDB', domain: 'mongodb.com', verified: true, official: true }, { tools: true, resources: true, prompts: false }, 'local', ['official', 'verified']),
  mkRegistry('puppeteer-mcp', 'Puppeteer', 'Browser automation — navigate, screenshot, and interact with web pages', 'puppeteer', '🎭', ['developer-tools'], { type: 'none' }, 'stdio', { name: 'Anthropic', domain: 'anthropic.com', verified: true, official: true }, { tools: true, resources: false, prompts: false }, 'local', ['official']),
  mkRegistry('memory-mcp', 'Memory', 'Persistent memory using a knowledge graph', 'memory', '🧠', ['developer-tools'], { type: 'none' }, 'stdio', { name: 'Anthropic', domain: 'anthropic.com', verified: true, official: true }, { tools: true, resources: false, prompts: false }, 'local', ['official']),
];

const REGISTRY_CATEGORIES = [
  { id: 'developer-tools', name: 'Developer Tools', icon: '💻' },
  { id: 'file-system', name: 'File System', icon: '📂' },
  { id: 'cloud', name: 'Cloud', icon: '☁️' },
  { id: 'productivity', name: 'Productivity', icon: '⚡' },
  { id: 'search', name: 'Search', icon: '🔍' },
];

const UI_CONFIG = {
  filters: [
    { id: 'category', label: 'Category', type: 'single', options: [
      { id: 'all', label: 'All Categories' },
      { id: 'developer-tools', label: 'Developer Tools', icon: '💻', match: { field: 'categories', operator: 'contains', value: 'developer-tools' } },
      { id: 'cloud', label: 'Cloud', icon: '☁️', match: { field: 'categories', operator: 'contains', value: 'cloud' } },
      { id: 'productivity', label: 'Productivity', icon: '⚡', match: { field: 'categories', operator: 'contains', value: 'productivity' } },
    ]},
    { id: 'auth', label: 'Auth Required', type: 'single', options: [
      { id: 'all', label: 'All' },
      { id: 'none', label: 'No Auth', match: { field: 'auth.type', operator: 'eq', value: 'none' } },
      { id: 'api_key', label: 'API Key', match: { field: 'auth.type', operator: 'eq', value: 'api_key' } },
      { id: 'oauth', label: 'OAuth', match: { field: 'auth.type', operator: 'eq', value: 'oauth' } },
    ]},
  ],
  sort_options: [{ id: 'name_asc', label: 'Name A-Z', rules: [{ field: 'name', direction: 'asc' }] }],
  default_sort: 'name_asc',
  items_per_page: 24,
};

const HOME_CONFIG = { featured_server_ids: ['github-mcp', 'slack-mcp', 'postgres-mcp', 'cloudflare-mcp', 'stripe-mcp', 'atlassian-mcp'] };

// ── IPC Mock Handler ───────────────────────────────────────

function buildMockHandler() {
  return `
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = window.__TAURI_EVENT_PLUGIN_INTERNALS__ || {};

    const SPACES = ${JSON.stringify(SPACES)};
    const INSTALLED = ${JSON.stringify(INSTALLED_SERVERS)};
    const FEATURES = ${JSON.stringify(SERVER_FEATURES)};
    const STATUSES = ${JSON.stringify(SERVER_STATUSES)};
    const FSETS = ${JSON.stringify(FEATURE_SETS)};
    const OAUTH = ${JSON.stringify(OAUTH_CLIENTS)};
    const REGISTRY = ${JSON.stringify(REGISTRY_SERVERS)};
    const CATEGORIES = ${JSON.stringify(REGISTRY_CATEGORIES)};
    const UI_CFG = ${JSON.stringify(UI_CONFIG)};
    const HOME_CFG = ${JSON.stringify(HOME_CONFIG)};

    const listeners = new Map();

    window.__TAURI_INTERNALS__.invoke = async function(cmd, args) {
      switch (cmd) {
        case 'list_spaces': return SPACES;
        case 'get_active_space': return SPACES[0];
        case 'get_space': return SPACES.find(s => s.id === args?.id) || SPACES[0];
        case 'set_active_space': return null;
        case 'create_space': return { id: crypto.randomUUID(), name: args?.name, icon: args?.icon, description: null, is_default: false, sort_order: 3, created_at: new Date().toISOString(), updated_at: new Date().toISOString() };
        case 'get_gateway_status': return { running: true, url: 'http://localhost:9315', active_sessions: 2, connected_backends: 6 };
        case 'start_gateway': return null;
        case 'stop_gateway': return null;
        case 'list_installed_servers': return INSTALLED;
        case 'discover_servers': return REGISTRY;
        case 'get_registry_ui_config': return UI_CFG;
        case 'get_registry_home_config': return HOME_CFG;
        case 'is_registry_offline': return false;
        case 'list_registry_categories': return CATEGORIES;
        case 'get_server_definition': return REGISTRY.find(s => s.id === args?.serverId);
        case 'install_server': return null;
        case 'uninstall_server': return null;
        case 'get_server_statuses': return STATUSES;
        case 'list_server_features': return FEATURES;
        case 'list_server_features_by_server': return FEATURES.filter(f => f.server_id === args?.serverId);
        case 'list_server_features_by_type': return FEATURES.filter(f => f.feature_type === args?.featureType);
        case 'list_feature_sets': return FSETS;
        case 'list_feature_sets_by_space': return FSETS;
        case 'get_builtin_feature_sets': return FSETS.filter(f => f.is_builtin);
        case 'get_oauth_clients': return OAUTH;
        case 'list_clients': return [];
        case 'get_all_client_grants': return {};
        case 'get_oauth_client_grants': return {};
        case 'refresh_oauth_tokens_on_startup': return { servers_checked: 2, tokens_refreshed: 1, refresh_failed: 0 };
        case 'connect_all_enabled_servers': return { connected: 6, reused: 0, failed: 0, oauth_required: 0, errors: [] };
        case 'get_pool_stats': return { total_instances: 6, connected_instances: 6, total_space_server_mappings: 6 };
        case 'get_server_logs': return [];
        case 'get_server_log_file': return '/home/user/.local/share/com.mcpmux.desktop/logs';
        case 'export_config': return '{}';
        case 'init_preset_clients': return null;
        case 'get_logs_path': return '/home/user/.local/share/com.mcpmux.desktop/logs';
        case 'get_startup_settings': return { autoLaunch: true, startMinimized: false, closeToTray: true };
        case 'update_startup_settings': return null;
        case 'open_logs_folder': return null;
        case 'get_version': return '0.1.0';
        case 'plugin:updater|check': throw new Error('no update');
        case 'plugin:updater|download_and_install': throw new Error('no update');
        default:
          if (cmd.startsWith('plugin:')) return null;
          console.log('[mock] unhandled:', cmd, args);
          return null;
      }
    };

    window.__TAURI_INTERNALS__.transformCallback = function(cb, once) {
      const id = Math.floor(Math.random() * 2147483647);
      window.__TAURI_INTERNALS__._cbs = window.__TAURI_INTERNALS__._cbs || {};
      window.__TAURI_INTERNALS__._cbs[id] = cb;
      return id;
    };
    window.__TAURI_INTERNALS__.unregisterCallback = function(id) {
      if (window.__TAURI_INTERNALS__._cbs) delete window.__TAURI_INTERNALS__._cbs[id];
    };
    window.__TAURI_INTERNALS__.runCallback = function(id, data) {
      const cb = window.__TAURI_INTERNALS__._cbs?.[id];
      if (cb) cb(data);
    };
    window.__TAURI_INTERNALS__.callbacks = new Map();

    window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener = function() {};
    window.__TAURI_INTERNALS__.metadata = { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } };
  `;
}

// ── Screenshot logic ───────────────────────────────────────

(async () => {
  const browser = await chromium.launch({
    headless: true,
    executablePath: '/root/.cache/ms-playwright/chromium-1194/chrome-linux/chrome',
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--disable-software-rasterizer', '--single-process', '--no-zygote'],
  });

  const ctx = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    colorScheme: 'light',
  });

  const page = await ctx.newPage();
  await page.addInitScript(buildMockHandler());

  // Logo mapping: emoji → local SVG path served by Vite
  const LOGO_MAP = {
    '🐙': '/logos/github.svg',
    '🔷': '/logos/atlassian.svg',
    '☁️': '/logos/cloudflare.svg',
    '🐘': '/logos/postgresql.svg',
    '🔵': '/logos/azure.svg',
    '📂': '/logos/filesystem.svg',
    '💬': '/logos/slack.svg',
    '📁': '/logos/gdrive.svg',
    '💳': '/logos/stripe.svg',
    '📐': '/logos/linear.svg',
    '🔥': '/logos/sentry.svg',
    '📝': '/logos/notion.svg',
    '🐶': '/logos/datadog.svg',
    '🍃': '/logos/mongodb.svg',
    '🎭': '/logos/puppeteer.svg',
    '🧠': '/logos/memory.svg',
    '🔍': '/logos/filesystem.svg',
  };

  async function injectLogos() {
    await page.evaluate((logoMap) => {
      const allDivs = document.querySelectorAll('div');
      for (const div of allDivs) {
        const text = div.textContent?.trim();
        if (!text || !logoMap[text]) continue;
        // Only target leaf divs (no child elements, just text)
        if (div.children.length > 0) continue;
        const rect = div.getBoundingClientRect();
        // Icon containers are small: Servers page ~40px, Discover page ~30px
        if (rect.width < 10 || rect.width > 55) continue;
        const logoUrl = logoMap[text];
        const size = Math.min(rect.width, rect.height) * 0.7;
        div.innerHTML = `<img src="${logoUrl}" style="width:${size}px;height:${size}px;object-fit:contain;" />`;
        div.style.display = 'flex';
        div.style.alignItems = 'center';
        div.style.justifyContent = 'center';
        div.style.overflow = 'hidden';
      }
    }, LOGO_MAP);
    await page.waitForTimeout(500);
  }

  async function shot(name) {
    await page.waitForTimeout(800);
    await injectLogos();
    await page.screenshot({ path: `${DIR}/${name}.png` });
    console.log(`  ok  ${name}`);
  }

  console.log('Taking screenshots (light theme)...\n');

  // 1. Dashboard
  await page.goto(BASE_URL);
  await page.waitForLoadState('networkidle');
  await shot('dashboard');

  // 2. My Servers
  await page.locator('nav button:has-text("My Servers")').click({ force: true });
  await page.waitForLoadState('networkidle');
  await shot('servers');

  // 3. Discover / Registry
  await page.locator('nav button:has-text("Discover")').click({ force: true });
  await page.waitForLoadState('networkidle');
  await shot('discover');

  // 4. Spaces
  await page.locator('nav button:has-text("Spaces")').last().click({ force: true });
  await page.waitForLoadState('networkidle');
  await shot('spaces');

  // 5. Feature Sets
  await page.locator('nav button:has-text("FeatureSets")').click({ force: true });
  await page.waitForLoadState('networkidle');
  await shot('featuresets');

  // 6. Clients
  await page.locator('nav button:has-text("Clients")').click({ force: true });
  await page.waitForLoadState('networkidle');
  await shot('clients');

  // 7. Settings
  await page.locator('nav button:has-text("Settings")').click({ force: true });
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(1000);
  await shot('settings');

  await browser.close();
  console.log('\nDone!');
})();
