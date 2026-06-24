/**
 * Feature Members API
 *
 * For managing individual features (tools/prompts/resources) in feature sets.
 */

/** @deprecated Prefer `@/lib/backend` — shim during facade migration. */
import { apiCall } from './transport';

export interface FeatureSetMember {
  id: string;
  feature_set_id: string;
  member_type: 'feature' | 'feature_set';
  member_id: string;
  mode: 'include' | 'exclude';
}

/** Add an individual feature to a feature set. */
export async function addFeatureToSet(
  featureSetId: string,
  featureId: string,
  mode: 'include' | 'exclude' = 'include'
): Promise<void> {
  return apiCall('add_feature_set_member', {
    featureSetId,
    input: {
      member_type: 'feature',
      member_id: featureId,
      mode,
    },
  });
}

/** Remove an individual feature from a feature set. */
export async function removeFeatureFromSet(
  featureSetId: string,
  featureId: string
): Promise<void> {
  const members = await getFeatureSetMembers(featureSetId);
  const member = members.find(
    (row) => row.member_type === 'feature' && row.member_id === featureId
  );
  if (!member) {
    return;
  }
  return apiCall('remove_feature_set_member', {
    featureSetId,
    memberId: member.id,
  });
}

/** Get all individual feature members of a feature set. */
export async function getFeatureSetMembers(
  featureSetId: string
): Promise<FeatureSetMember[]> {
  const set = await apiCall<{ members: FeatureSetMember[] }>('get_feature_set_with_members', {
    id: featureSetId,
  });
  return set.members.filter((member) => member.member_type === 'feature');
}
