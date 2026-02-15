/**
 * Screenshot Preseed Data
 *
 * Single source of truth for all mock data used in screenshot capture.
 * Edit this file to change what appears in screenshots, then re-run:
 *   pnpm exec wdio run tests/e2e/wdio.conf.ts --spec tests/e2e/specs/capture-screenshots.manual.ts
 */

import type { SeedFeatureInput } from '../helpers/tauri-api';

export const PRESEED: {
  spaces: { name: string; icon: string }[];
  serversToInstall: string[];
  featureSets: { name: string; description: string; icon?: string }[];
  serverFeatures: (spaceId: string) => SeedFeatureInput[];
} = {
  /** Additional spaces to create (default space is created automatically) */
  spaces: [
    { name: 'Work', icon: '💼' },
    { name: 'Personal', icon: '🏠' },
    { name: 'Open Source', icon: '🌍' },
  ],

  /** Server IDs to install in the default space (must match IDs in fixtures.ts).
   *  Order matters — GitHub first for screenshot prominence. */
  serversToInstall: [
    'github-server',
    'filesystem-server',
    'postgres-server',
    'slack-server',
    'brave-search',
    'docker-server',
    'notion-server',
    'aws-server',
    'cloudflare-workers-server',
    'azure-server',
  ],

  /** Custom feature sets to create in the default space */
  featureSets: [
    { name: 'Read Only', description: 'Read-only access — no writes, deletes, or mutations allowed', icon: '🔒' },
    { name: 'React Development', description: 'GitHub, Filesystem, and search tools for React projects', icon: '⚛️' },
    { name: 'Cloudflare Workers', description: 'Cloudflare Workers, KV, and deployment tools', icon: '☁️' },
    { name: 'Research & Analysis', description: 'Web search, Notion, and database query access', icon: '🔍' },
    { name: 'Full Stack Dev', description: 'All development servers — GitHub, Docker, Postgres, Filesystem', icon: '🚀' },
  ],

  /**
   * Mock server features (tools, prompts, resources) to seed into the DB.
   * These appear in the server expanded view and feature set detail panel.
   * Based on real MCP server capabilities.
   */
  serverFeatures: (spaceId: string): SeedFeatureInput[] => [
    // ── GitHub Server ──────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'create_or_update_file', display_name: 'Create or Update File', description: 'Create or update a single file in a GitHub repository' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'search_repositories', display_name: 'Search Repositories', description: 'Search for GitHub repositories by keyword, language, or topic' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'create_issue', display_name: 'Create Issue', description: 'Create a new issue in a GitHub repository' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'create_pull_request', display_name: 'Create Pull Request', description: 'Create a new pull request in a GitHub repository' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'get_file_contents', display_name: 'Get File Contents', description: 'Get the contents of a file or directory from a GitHub repository' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'push_files', display_name: 'Push Files', description: 'Push multiple files to a GitHub repository in a single commit' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'list_commits', display_name: 'List Commits', description: 'Get the list of commits for a branch in a repository' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'tool', feature_name: 'search_code', display_name: 'Search Code', description: 'Search for code across GitHub repositories' },
    { space_id: spaceId, server_id: 'github-server', feature_type: 'resource', feature_name: 'repo://owner/repo', display_name: 'Repository Contents', description: 'Access repository file tree and metadata' },

    // ── Filesystem Server ──────────────────────────────────────────────
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'tool', feature_name: 'read_file', display_name: 'Read File', description: 'Read the complete contents of a file from the filesystem' },
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'tool', feature_name: 'write_file', display_name: 'Write File', description: 'Create or overwrite a file with new content' },
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'tool', feature_name: 'list_directory', display_name: 'List Directory', description: 'List all files and subdirectories in a given path' },
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'tool', feature_name: 'search_files', display_name: 'Search Files', description: 'Recursively search for files matching a pattern' },
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'tool', feature_name: 'move_file', display_name: 'Move File', description: 'Move or rename a file or directory' },
    { space_id: spaceId, server_id: 'filesystem-server', feature_type: 'resource', feature_name: 'file:///project', display_name: 'Project Files', description: 'Access allowed project directory contents' },

    // ── PostgreSQL Server ──────────────────────────────────────────────
    { space_id: spaceId, server_id: 'postgres-server', feature_type: 'tool', feature_name: 'query', display_name: 'Run SQL Query', description: 'Execute a read-only SQL query against the database' },
    { space_id: spaceId, server_id: 'postgres-server', feature_type: 'tool', feature_name: 'list_tables', display_name: 'List Tables', description: 'List all tables in the connected database' },
    { space_id: spaceId, server_id: 'postgres-server', feature_type: 'tool', feature_name: 'describe_table', display_name: 'Describe Table', description: 'Get column names, types, and constraints for a table' },
    { space_id: spaceId, server_id: 'postgres-server', feature_type: 'resource', feature_name: 'postgres://schema', display_name: 'Database Schema', description: 'Access the full database schema as a resource' },

    // ── Slack Server ───────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'slack-server', feature_type: 'tool', feature_name: 'send_message', display_name: 'Send Message', description: 'Send a message to a Slack channel or user' },
    { space_id: spaceId, server_id: 'slack-server', feature_type: 'tool', feature_name: 'list_channels', display_name: 'List Channels', description: 'List all accessible Slack channels in the workspace' },
    { space_id: spaceId, server_id: 'slack-server', feature_type: 'tool', feature_name: 'search_messages', display_name: 'Search Messages', description: 'Search for messages across Slack channels' },
    { space_id: spaceId, server_id: 'slack-server', feature_type: 'prompt', feature_name: 'summarize_channel', display_name: 'Summarize Channel', description: 'Generate a summary of recent channel activity' },

    // ── Brave Search ───────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'brave-search', feature_type: 'tool', feature_name: 'web_search', display_name: 'Web Search', description: 'Search the web using Brave Search API' },
    { space_id: spaceId, server_id: 'brave-search', feature_type: 'tool', feature_name: 'local_search', display_name: 'Local Search', description: 'Search for local businesses and points of interest' },

    // ── Docker Server ──────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'docker-server', feature_type: 'tool', feature_name: 'list_containers', display_name: 'List Containers', description: 'List all Docker containers and their status' },
    { space_id: spaceId, server_id: 'docker-server', feature_type: 'tool', feature_name: 'run_container', display_name: 'Run Container', description: 'Create and start a new Docker container' },
    { space_id: spaceId, server_id: 'docker-server', feature_type: 'tool', feature_name: 'container_logs', display_name: 'Container Logs', description: 'Get logs from a running or stopped container' },
    { space_id: spaceId, server_id: 'docker-server', feature_type: 'tool', feature_name: 'list_images', display_name: 'List Images', description: 'List all Docker images on the host' },

    // ── Notion Server ──────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'notion-server', feature_type: 'tool', feature_name: 'search_pages', display_name: 'Search Pages', description: 'Search for pages and databases in your Notion workspace' },
    { space_id: spaceId, server_id: 'notion-server', feature_type: 'tool', feature_name: 'create_page', display_name: 'Create Page', description: 'Create a new page in a Notion database' },
    { space_id: spaceId, server_id: 'notion-server', feature_type: 'tool', feature_name: 'update_page', display_name: 'Update Page', description: 'Update properties and content of an existing page' },
    { space_id: spaceId, server_id: 'notion-server', feature_type: 'resource', feature_name: 'notion://workspace', display_name: 'Workspace Pages', description: 'Access your Notion workspace page hierarchy' },

    // ── AWS Server ─────────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'aws-server', feature_type: 'tool', feature_name: 'list_s3_buckets', display_name: 'List S3 Buckets', description: 'List all S3 buckets in the AWS account' },
    { space_id: spaceId, server_id: 'aws-server', feature_type: 'tool', feature_name: 'get_s3_object', display_name: 'Get S3 Object', description: 'Download an object from an S3 bucket' },
    { space_id: spaceId, server_id: 'aws-server', feature_type: 'tool', feature_name: 'describe_instances', display_name: 'Describe EC2 Instances', description: 'List and describe EC2 instances with their status' },
    { space_id: spaceId, server_id: 'aws-server', feature_type: 'tool', feature_name: 'invoke_lambda', display_name: 'Invoke Lambda', description: 'Invoke an AWS Lambda function with a given payload' },

    // ── Cloudflare Workers Server ──────────────────────────────────────
    { space_id: spaceId, server_id: 'cloudflare-workers-server', feature_type: 'tool', feature_name: 'list_workers', display_name: 'List Workers', description: 'List all Cloudflare Workers in the account' },
    { space_id: spaceId, server_id: 'cloudflare-workers-server', feature_type: 'tool', feature_name: 'get_worker_code', display_name: 'Get Worker Code', description: 'Retrieve the source code of a deployed Worker' },
    { space_id: spaceId, server_id: 'cloudflare-workers-server', feature_type: 'tool', feature_name: 'list_kv_namespaces', display_name: 'List KV Namespaces', description: 'List all Workers KV namespaces in the account' },

    // ── Azure Server ───────────────────────────────────────────────────
    { space_id: spaceId, server_id: 'azure-server', feature_type: 'tool', feature_name: 'list_resource_groups', display_name: 'List Resource Groups', description: 'List all Azure resource groups in the subscription' },
    { space_id: spaceId, server_id: 'azure-server', feature_type: 'tool', feature_name: 'list_vms', display_name: 'List Virtual Machines', description: 'List all virtual machines and their status' },
    { space_id: spaceId, server_id: 'azure-server', feature_type: 'tool', feature_name: 'query_cosmos_db', display_name: 'Query Cosmos DB', description: 'Execute a SQL query against an Azure Cosmos DB container' },
  ],
};
