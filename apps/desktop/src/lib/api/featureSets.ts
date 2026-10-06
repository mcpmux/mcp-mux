import { invoke } from '@tauri-apps/api/core';

/**
 * FeatureSet type.
 *
 * - `default`: auto-created per Space. Fallback when no WorkspaceBinding matches.
 * - `custom`: user-defined.
 */
/**
 * `starter` is the auto-seeded FS that comes with each Space. It's the
 * default fallback the resolver routes unmapped folders / rootless sessions
 * to, so it's builtin: its members are editable (change which tools it
 * includes, or empty it), but it can't be renamed or deleted. The legacy
 * `'default'` value is accepted on read because migration 013 rewrites stored
 * rows lazily and a stale fetch could still surface it; new writes use
 * `'starter'`.
 */
export type FeatureSetType = 'starter' | 'default' | 'custom';

/**
 * Is this FeatureSet the auto-seeded "Starter" for its Space? Returns
 * `true` for both the new `'starter'` value and the legacy `'default'`
 * — migration 013 rewrites rows in-place but a stale read could still
 * surface the old value. Use this everywhere instead of comparing the
 * type literal directly so the transition window is invisible to UI.
 */
export function isStarterFeatureSet(fs: { feature_set_type: FeatureSetType }): boolean {
  return fs.feature_set_type === 'starter' || fs.feature_set_type === 'default';
}

/**
 * Member type in a feature set.
 */
export type MemberType = 'feature' | 'feature_set';

/**
 * Mode for including/excluding members.
 */
export type MemberMode = 'include' | 'exclude';

/**
 * A member of a feature set.
 */
export interface FeatureSetMember {
  id: string;
  feature_set_id: string;
  member_type: MemberType;
  member_id: string;
  mode: MemberMode;
}

/**
 * A FeatureSet defines which tools are available to clients.
 */
export interface FeatureSet {
  id: string;
  name: string;
  description: string | null;
  icon: string | null;
  space_id: string | null;
  feature_set_type: FeatureSetType;
  server_id: string | null;
  is_builtin: boolean;
  is_deleted: boolean;
  /**
   * Auto mode: the set grants every tool, prompt, and resource from every
   * server in its Space, so newly installed servers show up with no setup.
   * An auto set has no explicit `members`; saving a member list switches it
   * to a manual selection.
   */
  auto_include: boolean;
  members: FeatureSetMember[];
}

/**
 * Input for creating a feature set.
 */
export interface CreateFeatureSetInput {
  name: string;
  space_id: string;
  description?: string;
  icon?: string;
}

/**
 * Input for updating a feature set.
 */
export interface UpdateFeatureSetInput {
  name?: string;
  description?: string;
  icon?: string;
}

/**
 * Input for adding a member to a feature set.
 */
export interface AddMemberInput {
  member_type: MemberType;
  member_id: string;
  mode?: MemberMode;
}

/**
 * List all feature sets.
 */
export async function listFeatureSets(): Promise<FeatureSet[]> {
  return invoke('list_feature_sets');
}

/**
 * List feature sets for a specific space.
 */
export async function listFeatureSetsBySpace(spaceId: string): Promise<FeatureSet[]> {
  return invoke('list_feature_sets_by_space', { spaceId });
}

/**
 * Get a feature set by ID.
 */
export async function getFeatureSet(id: string): Promise<FeatureSet | null> {
  return invoke('get_feature_set', { id });
}

/**
 * Create a new feature set.
 */
export async function createFeatureSet(input: CreateFeatureSetInput): Promise<FeatureSet> {
  return invoke('create_feature_set', { input });
}

/**
 * Delete a feature set.
 */
export async function deleteFeatureSet(id: string): Promise<void> {
  return invoke('delete_feature_set', { id });
}

/**
 * Get a feature set with its members.
 */
export async function getFeatureSetWithMembers(id: string): Promise<FeatureSet | null> {
  return invoke('get_feature_set_with_members', { id });
}

/**
 * Update a feature set.
 */
export async function updateFeatureSet(
  id: string,
  input: UpdateFeatureSetInput
): Promise<FeatureSet> {
  return invoke('update_feature_set', { id, input });
}

/**
 * Add a member to a feature set.
 */
export async function addFeatureSetMember(
  featureSetId: string,
  input: AddMemberInput
): Promise<FeatureSet> {
  return invoke('add_feature_set_member', { featureSetId, input });
}

/**
 * Remove a member from a feature set.
 */
export async function removeFeatureSetMember(
  featureSetId: string,
  memberId: string
): Promise<FeatureSet> {
  return invoke('remove_feature_set_member', { featureSetId, memberId });
}

/**
 * Set all members for a feature set (replaces existing).
 */
export async function setFeatureSetMembers(
  featureSetId: string,
  members: AddMemberInput[]
): Promise<FeatureSet> {
  return invoke('set_feature_set_members', { featureSetId, members });
}

/**
 * Switch a FeatureSet into or out of auto mode (every server's tools).
 * Turning it off keeps what the set granted as an explicit list to edit.
 */
export async function setFeatureSetAutoInclude(
  featureSetId: string,
  enabled: boolean
): Promise<FeatureSet> {
  return invoke('set_feature_set_auto_include', { featureSetId, enabled });
}

/**
 * What the Space's Starter gives connected apps: how many tools, from how
 * many servers, and whether that's past the size where McpMux suggests a
 * focused FeatureSet (`threshold`, a warning — nothing is ever cut off).
 */
export interface StarterToolSummary {
  feature_set_id: string;
  auto_include: boolean;
  tool_count: number;
  server_count: number;
  threshold: number;
  over_threshold: boolean;
}

export async function getStarterToolSummary(spaceId: string): Promise<StarterToolSummary | null> {
  return invoke('get_starter_tool_summary', { spaceId });
}

/**
 * Settings switch: do Starters include every server's tools automatically?
 * On by default. Turning it off switches every Space's Starter to a manual
 * selection (keeping its current tools) and makes new Spaces start manual.
 */
export async function getStarterAutoIncludeDefault(): Promise<boolean> {
  return invoke('get_starter_auto_include_default');
}

export async function setStarterAutoIncludeDefault(enabled: boolean): Promise<void> {
  return invoke('set_starter_auto_include_default', { enabled });
}
