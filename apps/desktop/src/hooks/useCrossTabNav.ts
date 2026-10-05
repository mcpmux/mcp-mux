/**
 * Cross-tab jumps between Clients → Mapping → FeatureSets.
 *
 * Each tab owns one link in the chain (keys → client → mapping → feature
 * sets), so a user configuring one of them needs a direct way to the next
 * instead of hunting through the sidebar. These hooks set a one-shot store
 * intent and switch tabs; the destination page consumes the intent on mount.
 */

import { useCallback } from 'react';
import {
  useNavigateTo,
  useSetPendingClientId,
  useSetPendingFeatureSetCreate,
  useSetPendingMapping,
  useSetViewSpace,
  useViewSpaceId,
  type NavItem,
} from '@/stores';

/**
 * Open FeatureSets on `spaceId` with the create dialog up, offering a way back
 * to `returnTo` once the set exists. FeatureSets is scoped to the viewed Space,
 * so we switch it first — otherwise the new set would land in the wrong Space.
 */
export function useGoCreateFeatureSet(returnTo: NavItem) {
  const navigateTo = useNavigateTo();
  const viewSpaceId = useViewSpaceId();
  const setViewSpace = useSetViewSpace();
  const setPending = useSetPendingFeatureSetCreate();
  return useCallback(
    (spaceId?: string | null) => {
      const switching = !!spaceId && spaceId !== viewSpaceId;
      if (switching) setViewSpace(spaceId);
      setPending({ returnTo, restoreSpaceId: switching ? viewSpaceId : null });
      navigateTo('featuresets');
    },
    [navigateTo, viewSpaceId, setViewSpace, setPending, returnTo]
  );
}

/**
 * Open the Mapping tab on a client's mapping — or its create flow, prefilled
 * with the client, when it has none yet.
 */
export function useGoConfigureClientMapping() {
  const navigateTo = useNavigateTo();
  const setPendingMapping = useSetPendingMapping();
  return useCallback(
    (clientId: string) => {
      setPendingMapping({ key: clientId, bindingType: 'id' });
      navigateTo('workspaces');
    },
    [navigateTo, setPendingMapping]
  );
}

/** Open the Clients tab with a client's side panel showing. */
export function useGoToClient() {
  const navigateTo = useNavigateTo();
  const setPendingClientId = useSetPendingClientId();
  return useCallback(
    (clientId: string) => {
      setPendingClientId(clientId);
      navigateTo('clients');
    },
    [navigateTo, setPendingClientId]
  );
}
