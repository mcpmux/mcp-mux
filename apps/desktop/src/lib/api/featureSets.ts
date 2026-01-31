import { invoke } from '@tauri-apps/api/core';

/**
 * FeatureSet type determines how features are resolved.
 */
export type FeatureSetType = 'all' | 'default' | 'server-all' | 'custom';

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
 * Get builtin feature sets for a space.
 */
export async function getBuiltinFeatureSets(spaceId: string): Promise<FeatureSet[]> {
  return invoke('get_builtin_feature_sets', { spaceId });
}

/**
 * Ensure a server-all featureset exists for a server in a space.
 */
export async function ensureServerAllFeatureSet(
  spaceId: string,
  serverId: string,
  serverName: string
): Promise<FeatureSet> {
  return invoke('ensure_server_all_feature_set', { spaceId, serverId, serverName });
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
export async function updateFeatureSet(id: string, input: UpdateFeatureSetInput): Promise<FeatureSet> {
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
