import { Plus } from 'lucide-react';
import { useNavigateTo, useSetPendingFeatureSetNew, useSetViewSpace } from '@/stores';

/**
 * Inline "create a new feature set" shortcut shown under the feature-set picker
 * on the Mapping surfaces (the inspector's BindingForm + the setup wizard's
 * tools step).
 *
 * A fresh Space is auto-seeded with only its Starter set, so a user who wants
 * to hand a folder a *different* toolset has no obvious way to discover where
 * sets are made — the picker just shows "Starter" with no affordance to add
 * more. This routes them straight to the FeatureSets page for the Space they're
 * mapping, with its Create dialog already open, then lets them come back and
 * pick the new set. Disabled until a Space is chosen (there's nowhere to create
 * the set otherwise).
 */
export function CreateFeatureSetLink({ spaceId }: { spaceId: string }) {
  const navigateTo = useNavigateTo();
  const setViewSpace = useSetViewSpace();
  const openCreate = useSetPendingFeatureSetNew();
  return (
    <button
      type="button"
      disabled={!spaceId}
      onClick={() => {
        // View the same Space this mapping targets so the new set lands where
        // it can actually be selected here, then open the create dialog.
        if (spaceId) setViewSpace(spaceId);
        openCreate(true);
        navigateTo('featuresets');
      }}
      className="text-primary-600 dark:text-primary-400 hover:text-primary-700 dark:hover:text-primary-300 inline-flex items-center gap-1 rounded text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-50"
      data-testid="workspace-binding-create-fs"
    >
      <Plus className="h-3.5 w-3.5" />
      Create a new feature set
    </button>
  );
}
