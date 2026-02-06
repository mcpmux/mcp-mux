/**
 * Registry types for MCP server browsing and installation.
 */

/** Input definition from registry */
export interface InputDefinition {
  id: string;
  label: string;
  description?: string;
  /** Input type - determines how the field is rendered */
  type?: 'text' | 'password' | 'boolean' | 'number' | 'url';
  required?: boolean;
  secret?: boolean;
  placeholder?: string;
  /** URL to obtain credentials/values */
  obtain_url?: string;
  /** Instructions for obtaining credentials/values */
  obtain_instructions?: string;
  /** Nested obtain info (alternative format from API) */
  obtain?: {
    url?: string;
    instructions?: string;
    button_label?: string;
  };
}

/** Transport metadata */
export interface TransportMetadata {
  inputs: InputDefinition[];
}

/** Transport configuration */
export type TransportConfig =
  | { type: 'stdio'; command: string; args: string[]; env: Record<string, string>; metadata: TransportMetadata }
  | { type: 'http'; url: string; headers: Record<string, string>; metadata: TransportMetadata };

/** Server source */
export type ServerSource =
  | { type: 'UserSpace'; space_id: string; file_path: string }
  | { type: 'Bundled' }
  | { type: 'Registry'; url: string; name: string };

/** Publisher info */
export interface PublisherInfo {
  name: string;
  domain: string | null;
  verified: boolean;
  official: boolean;
}

/** Server definition from discovery */
export interface ServerDefinition {
  id: string;
  name: string;
  description: string | null;
  alias: string | null;
  auth: AuthConfig | null;
  icon: string | null;
  transport: TransportConfig;
  categories: string[];
  publisher: PublisherInfo | null;
  source: ServerSource;
  // Schema v2.1 additions
  badges?: Badge[];
  hosting_type?: HostingType;
  license?: string;
  license_url?: string;
  installation?: Installation;
  capabilities?: Capabilities;
  sponsored?: Sponsored;
  media?: Media;
  changelog_url?: string;
}

/** Auth configuration - matches backend snake_case serialization */
export type AuthConfig =
  | { type: 'none' }
  | { type: 'api_key'; instructions: string | null }
  | { type: 'optional_api_key'; instructions: string | null }
  | { type: 'oauth' };

/** Installation source - tracks how the server was installed */
export type InstallationSource =
  | { type: 'registry' }
  | { type: 'user_config'; file_path: string }
  | { type: 'manual_entry' };

/** Installed server state from database */
export interface InstalledServerState {
  id: string;
  space_id: string;
  server_id: string;
  server_name: string | null; // Cached from definition at install time
  cached_definition: string | null; // JSON-serialized ServerDefinition
  input_values: Record<string, string>;
  enabled: boolean;
  env_overrides: Record<string, string>;
  args_append: string[];
  extra_headers: Record<string, string>;
  oauth_connected: boolean;
  source: InstallationSource; // How this server was installed
  created_at: string;
  updated_at: string;
}

/** Server view model (merged definition + state) */
export interface ServerViewModel extends ServerDefinition {
  is_installed: boolean;
  enabled: boolean;
  oauth_connected: boolean;
  input_values: Record<string, string>;
  connection_status: 'disconnected' | 'connecting' | 'connected' | 'oauth_required' | 'error';
  missing_required_inputs: boolean;
  last_error: string | null;
  created_at?: string;
  /** Installation source - only present for installed servers */
  installation_source?: InstallationSource;
}

/** Registry category */
export interface RegistryCategory {
  id: string;
  name: string;
  icon: string | null;
}

// ============================================
// UI Configuration Types (API-driven)
// ============================================

/** UI configuration from bundle */
export interface UiConfig {
  filters: FilterDefinition[];
  sort_options: SortOption[];
  default_sort: string;
  items_per_page: number;
}

/** Filter definition */
export interface FilterDefinition {
  id: string;
  label: string;
  type: 'single' | 'multi';
  options: FilterOption[];
}

/** Filter option */
export interface FilterOption {
  id: string;
  label: string;
  icon?: string;
  match?: FilterMatch;
}

/** Filter match rule */
export interface FilterMatch {
  field: string;
  operator: 'eq' | 'in' | 'contains';
  value: unknown;
}

/** Sort option */
export interface SortOption {
  id: string;
  label: string;
  rules: SortRule[];
}

/** Sort rule */
export interface SortRule {
  field: string;
  direction: 'asc' | 'desc';
  nulls?: 'first' | 'last';
}

/** Home configuration */
export interface HomeConfig {
  featured_server_ids: string[];
}

// ============================================
// Schema v2.1 Additions
// ============================================

/** Visual badge indicators */
export type Badge = 'official' | 'verified' | 'featured' | 'sponsored' | 'popular';

/** Server hosting type */
export type HostingType = 'local' | 'remote' | 'hybrid';

/** Installation metadata */
export interface Installation {
  difficulty?: 'easy' | 'moderate' | 'advanced';
  prerequisites?: string[];
  estimated_time?: string;
}

/** MCP capabilities with read-only support */
export interface Capabilities {
  tools?: boolean;
  resources?: boolean;
  prompts?: boolean;
  read_only_mode?: boolean;
}

/** Sponsorship information */
export interface Sponsored {
  enabled?: boolean;
  sponsor_name?: string;
  sponsor_url?: string;
  sponsor_logo?: string;
  campaign_id?: string;
}

/** Rich media content */
export interface Media {
  screenshots?: string[];
  demo_video?: string;
  banner?: string;
}