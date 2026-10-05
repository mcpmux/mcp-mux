import { Plus } from 'lucide-react';
import { useGoCreateFeatureSet } from '@/hooks/useCrossTabNav';
import type { NavItem } from '@/stores';

/**
 * Inline "New feature set" link for the places a user picks feature sets
 * (mapping form, setup wizard, new-folder sheet). Jumps to FeatureSets on the
 * chosen Space with the create dialog open, so a missing set is one click
 * away instead of a trip through the sidebar.
 */
export function CreateFeatureSetLink({
  spaceId,
  returnTo = 'workspaces',
  label = 'New feature set',
  onNavigate,
  testId = 'create-feature-set-link',
}: {
  spaceId?: string | null;
  returnTo?: NavItem;
  label?: string;
  /** Runs before navigating — e.g. to close the sheet the link lives in. */
  onNavigate?: () => void;
  testId?: string;
}) {
  const goCreate = useGoCreateFeatureSet(returnTo);
  return (
    <button
      type="button"
      onClick={() => {
        onNavigate?.();
        goCreate(spaceId);
      }}
      className="inline-flex items-center gap-1 text-xs font-medium text-[rgb(var(--accent))] hover:underline"
      title="Create a feature set in this Space — you'll be taken to FeatureSets, with a link back here."
      data-testid={testId}
    >
      <Plus className="h-3 w-3" />
      {label}
    </button>
  );
}
