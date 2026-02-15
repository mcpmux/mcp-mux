/**
 * Bundle Fixtures for E2E Testing
 *
 * These fixtures define realistic MCP servers that point to our stub MCP servers,
 * covering all transport types, auth modes, and input configurations.
 *
 * For screenshots, the names/icons/descriptions are realistic (GitHub, Slack, etc.)
 * but the actual transport commands/urls point to our local stub servers.
 */

// Stub server ports
const STUB_HTTP_PORT = 3457;
const STUB_OAUTH_PORT = 3458;

export interface ServerDefinition {
  id: string;
  name: string;
  alias: string;
  description: string;
  icon: string;
  schema_version: string;
  categories: string[];
  tags: string[];
  transport: {
    type: 'stdio' | 'http';
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string;
    metadata: {
      inputs: InputDefinition[];
    };
  };
  auth: {
    type: 'none' | 'api_key' | 'oauth';
    instructions?: string;
  };
  publisher: {
    name: string;
    domain?: string;
    url?: string;
    verified: boolean;
    domain_verified: boolean;
    official: boolean;
  };
  links?: {
    repository?: string;
    documentation?: string;
    homepage?: string;
  };
  platforms: string[];
  capabilities: {
    tools: boolean;
    resources: boolean;
    prompts: boolean;
  };
}

export interface InputDefinition {
  id: string;
  label: string;
  description?: string;
  type: 'text' | 'password' | 'number' | 'boolean' | 'url';
  required: boolean;
  secret: boolean;
  placeholder?: string;
  obtain?: {
    url: string;
    instructions: string;
    button_label: string;
  };
}

export interface Category {
  id: string;
  name: string;
  icon: string;
}

export interface RegistryBundle {
  version: string;
  updated_at: string;
  servers: ServerDefinition[];
  categories: Category[];
  ui: {
    filters: unknown[];
    sort_options: unknown[];
    default_sort: string;
    items_per_page: number;
  };
  home?: {
    featured_server_ids: string[];
  };
}

// Realistic MCP servers pointing to our stub servers for E2E
const TEST_SERVERS: ServerDefinition[] = [
  // 1. GitHub — stdio, no auth, official
  {
    id: 'github-server',
    name: 'GitHub',
    alias: 'github',
    description: 'Repository management, issues, pull requests, and code search via the GitHub API',
    icon: 'https://cdn.simpleicons.org/github',
    schema_version: '2.0',
    categories: ['developer-tools'],
    tags: ['github', 'git', 'repository', 'issues'],
    transport: {
      type: 'stdio',
      command: 'node',
      args: ['--import', 'tsx', 'tests/e2e/mocks/stub-mcp-server/stdio-server.ts'],
      env: {},
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'none',
    },
    publisher: {
      name: 'Model Context Protocol',
      domain: 'modelcontextprotocol.io',
      verified: true,
      domain_verified: true,
      official: true,
    },
    links: {
      repository: 'https://github.com/modelcontextprotocol/servers',
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: true,
    },
  },

  // 2. Filesystem — stdio, no auth, official
  {
    id: 'filesystem-server',
    name: 'Filesystem',
    alias: 'fs',
    description: 'Secure file operations with configurable access controls and sandboxing',
    icon: 'https://cdn.simpleicons.org/files/4285F4',
    schema_version: '2.0',
    categories: ['file-system'],
    tags: ['filesystem', 'files', 'directory', 'local'],
    transport: {
      type: 'stdio',
      command: 'node',
      args: ['--import', 'tsx', 'tests/e2e/mocks/stub-mcp-server/stdio-server.ts', '${input:DIRECTORY}'],
      env: {},
      metadata: {
        inputs: [
          {
            id: 'DIRECTORY',
            label: 'Allowed Directory',
            description: 'Directory the server can access',
            type: 'text',
            required: true,
            secret: false,
            placeholder: 'C:\\Users\\Projects',
          },
        ],
      },
    },
    auth: {
      type: 'none',
    },
    publisher: {
      name: 'Model Context Protocol',
      domain: 'modelcontextprotocol.io',
      verified: true,
      domain_verified: true,
      official: true,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 3. PostgreSQL — stdio, api_key, official
  {
    id: 'postgres-server',
    name: 'PostgreSQL',
    alias: 'postgres',
    description: 'Query databases, manage schemas, inspect tables, and run migrations',
    icon: 'https://cdn.simpleicons.org/postgresql',
    schema_version: '2.0',
    categories: ['database'],
    tags: ['postgres', 'database', 'sql', 'schema'],
    transport: {
      type: 'stdio',
      command: 'node',
      args: ['--import', 'tsx', 'tests/e2e/mocks/stub-mcp-server/stdio-server.ts'],
      env: {
        DATABASE_URL: '${input:DATABASE_URL}',
      },
      metadata: {
        inputs: [
          {
            id: 'DATABASE_URL',
            label: 'Connection String',
            description: 'PostgreSQL connection URL',
            type: 'password',
            required: true,
            secret: true,
            placeholder: 'postgresql://user:pass@localhost:5432/db',
          },
        ],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Provide your PostgreSQL connection string',
    },
    publisher: {
      name: 'Model Context Protocol',
      domain: 'modelcontextprotocol.io',
      verified: true,
      domain_verified: true,
      official: true,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 4. Slack — http, oauth
  {
    id: 'slack-server',
    name: 'Slack',
    alias: 'slack',
    description: 'Send messages, manage channels, and search conversations across workspaces',
    icon: 'https://github.com/slackapi.png?size=128',
    schema_version: '2.0',
    categories: ['productivity'],
    tags: ['slack', 'messaging', 'chat', 'team'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_OAUTH_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'oauth',
    },
    publisher: {
      name: 'Slack',
      domain: 'slack.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: false,
      prompts: true,
    },
  },

  // 5. Brave Search — http, api_key
  {
    id: 'brave-search',
    name: 'Brave Search',
    alias: 'brave',
    description: 'Web search with privacy-focused results and AI-ready summaries',
    icon: 'https://cdn.simpleicons.org/brave',
    schema_version: '2.0',
    categories: ['search'],
    tags: ['search', 'web', 'brave', 'privacy'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Get your API key from search.brave.com',
    },
    publisher: {
      name: 'Brave',
      domain: 'brave.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: false,
      prompts: false,
    },
  },

  // 6. Docker — stdio, no auth
  {
    id: 'docker-server',
    name: 'Docker',
    alias: 'docker',
    description: 'Manage containers, images, networks, and volumes from your AI client',
    icon: 'https://cdn.simpleicons.org/docker',
    schema_version: '2.0',
    categories: ['developer-tools'],
    tags: ['docker', 'containers', 'devops', 'infrastructure'],
    transport: {
      type: 'stdio',
      command: 'node',
      args: ['--import', 'tsx', 'tests/e2e/mocks/stub-mcp-server/stdio-server.ts'],
      env: {},
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'none',
    },
    publisher: {
      name: 'Docker',
      domain: 'docker.com',
      verified: true,
      domain_verified: false,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 7. Notion — http, oauth
  {
    id: 'notion-server',
    name: 'Notion',
    alias: 'notion',
    description: 'Create pages, query databases, and manage workspaces programmatically',
    icon: 'https://cdn.simpleicons.org/notion',
    schema_version: '2.0',
    categories: ['productivity'],
    tags: ['notion', 'notes', 'wiki', 'database'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_OAUTH_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'oauth',
    },
    publisher: {
      name: 'Notion',
      domain: 'notion.so',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 8. AWS — http, api_key
  {
    id: 'aws-server',
    name: 'AWS',
    alias: 'aws',
    description: 'Interact with S3, Lambda, DynamoDB, and other AWS services',
    icon: 'https://github.com/aws.png?size=128',
    schema_version: '2.0',
    categories: ['cloud'],
    tags: ['aws', 'cloud', 's3', 'lambda'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Provide your AWS access key and secret',
    },
    publisher: {
      name: 'Amazon Web Services',
      domain: 'aws.amazon.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 9. SQLite — stdio, no auth
  {
    id: 'sqlite-server',
    name: 'SQLite',
    alias: 'sqlite',
    description: 'Local database queries with read/write access control and schema inspection',
    icon: 'https://cdn.simpleicons.org/sqlite',
    schema_version: '2.0',
    categories: ['database'],
    tags: ['sqlite', 'database', 'local', 'sql'],
    transport: {
      type: 'stdio',
      command: 'node',
      args: ['--import', 'tsx', 'tests/e2e/mocks/stub-mcp-server/stdio-server.ts'],
      env: {},
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'none',
    },
    publisher: {
      name: 'Model Context Protocol',
      domain: 'modelcontextprotocol.io',
      verified: true,
      domain_verified: true,
      official: true,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 10. Sentry — http, api_key
  {
    id: 'sentry-server',
    name: 'Sentry',
    alias: 'sentry',
    description: 'Error tracking, performance monitoring, and release management',
    icon: 'https://cdn.simpleicons.org/sentry',
    schema_version: '2.0',
    categories: ['developer-tools'],
    tags: ['sentry', 'errors', 'monitoring', 'performance'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Get your auth token from sentry.io',
    },
    publisher: {
      name: 'Sentry',
      domain: 'sentry.io',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: false,
      prompts: false,
    },
  },

  // 11. Linear — http, oauth
  {
    id: 'linear-server',
    name: 'Linear',
    alias: 'linear',
    description: 'Issue tracking, project management, and sprint workflows',
    icon: 'https://cdn.simpleicons.org/linear',
    schema_version: '2.0',
    categories: ['productivity'],
    tags: ['linear', 'issues', 'project-management', 'sprints'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_OAUTH_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'oauth',
    },
    publisher: {
      name: 'Linear',
      domain: 'linear.app',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: false,
      prompts: true,
    },
  },

  // 12. Cloudflare — http, no auth
  {
    id: 'cloudflare-server',
    name: 'Cloudflare',
    alias: 'cf',
    description: 'Search and browse Cloudflare documentation and API references',
    icon: 'https://cdn.simpleicons.org/cloudflare',
    schema_version: '2.0',
    categories: ['cloud'],
    tags: ['cloudflare', 'docs', 'cdn', 'workers'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'none',
    },
    publisher: {
      name: 'Cloudflare',
      domain: 'cloudflare.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 13. Cloudflare Workers — http, api_key
  {
    id: 'cloudflare-workers-server',
    name: 'Cloudflare Workers',
    alias: 'cf-workers',
    description: 'Deploy, manage, and monitor Cloudflare Workers and KV namespaces',
    icon: 'https://cdn.simpleicons.org/cloudflareworkers',
    schema_version: '2.0',
    categories: ['cloud'],
    tags: ['cloudflare', 'workers', 'serverless', 'edge'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Provide your Cloudflare API token',
    },
    publisher: {
      name: 'Cloudflare',
      domain: 'cloudflare.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },

  // 14. Azure — http, api_key
  {
    id: 'azure-server',
    name: 'Azure',
    alias: 'azure',
    description: 'Manage Azure resources, deploy services, and monitor infrastructure',
    icon: 'https://github.com/azure.png?size=128',
    schema_version: '2.0',
    categories: ['cloud'],
    tags: ['azure', 'cloud', 'microsoft', 'infrastructure'],
    transport: {
      type: 'http',
      url: `http://localhost:${STUB_HTTP_PORT}/mcp`,
      metadata: {
        inputs: [],
      },
    },
    auth: {
      type: 'api_key',
      instructions: 'Provide your Azure subscription credentials',
    },
    publisher: {
      name: 'Microsoft',
      domain: 'azure.microsoft.com',
      verified: true,
      domain_verified: true,
      official: false,
    },
    platforms: ['all'],
    capabilities: {
      tools: true,
      resources: true,
      prompts: false,
    },
  },
];

const TEST_CATEGORIES: Category[] = [
  { id: 'developer-tools', name: 'Developer Tools', icon: '💻' },
  { id: 'file-system', name: 'File System', icon: '📂' },
  { id: 'database', name: 'Database', icon: '🗄️' },
  { id: 'cloud', name: 'Cloud', icon: '☁️' },
  { id: 'productivity', name: 'Productivity', icon: '⚡' },
  { id: 'search', name: 'Search', icon: '🔍' },
];

export const BUNDLE_DATA: RegistryBundle = {
  version: '2.0.0-test',
  updated_at: new Date().toISOString().split('T')[0],
  servers: TEST_SERVERS,
  categories: TEST_CATEGORIES,
  ui: {
    filters: [
      {
        id: 'category',
        label: 'Category',
        type: 'single',
        options: [
          { id: 'all', label: 'All Categories' },
          { id: 'developer-tools', label: 'Developer Tools', icon: '💻', match: { field: 'categories', operator: 'contains', value: 'developer-tools' } },
          { id: 'file-system', label: 'File System', icon: '📂', match: { field: 'categories', operator: 'contains', value: 'file-system' } },
          { id: 'database', label: 'Database', icon: '🗄️', match: { field: 'categories', operator: 'contains', value: 'database' } },
          { id: 'cloud', label: 'Cloud', icon: '☁️', match: { field: 'categories', operator: 'contains', value: 'cloud' } },
          { id: 'productivity', label: 'Productivity', icon: '⚡', match: { field: 'categories', operator: 'contains', value: 'productivity' } },
          { id: 'search', label: 'Search', icon: '🔍', match: { field: 'categories', operator: 'contains', value: 'search' } },
        ],
      },
      {
        id: 'auth',
        label: 'Auth Required',
        type: 'single',
        options: [
          { id: 'all', label: 'All' },
          { id: 'none', label: 'No Auth', match: { field: 'auth.type', operator: 'eq', value: 'none' } },
          { id: 'api_key', label: 'API Key', match: { field: 'auth.type', operator: 'eq', value: 'api_key' } },
          { id: 'oauth', label: 'OAuth', match: { field: 'auth.type', operator: 'eq', value: 'oauth' } },
        ],
      },
      {
        id: 'transport',
        label: 'Transport',
        type: 'single',
        options: [
          { id: 'all', label: 'All' },
          { id: 'http', label: 'Remote (HTTP)', match: { field: 'transport.type', operator: 'eq', value: 'http' } },
          { id: 'stdio', label: 'Local (Stdio)', match: { field: 'transport.type', operator: 'eq', value: 'stdio' } },
        ],
      },
    ],
    sort_options: [
      {
        id: 'name_asc',
        label: 'Name A-Z',
        rules: [{ field: 'name', direction: 'asc' }],
      },
    ],
    default_sort: 'name_asc',
    items_per_page: 24,
  },
  home: {
    featured_server_ids: ['github-server', 'postgres-server', 'slack-server'],
  },
};
