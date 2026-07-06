/**
 * CreateFeatureSetLink — the "Create a new feature set" shortcut shown under the
 * feature-set picker on the Mapping surfaces (the inspector's BindingForm + the
 * setup wizard's tools step).
 *
 * A fresh Space is auto-seeded with only its Starter set, so this affordance is
 * how a user discovers where new sets are made. Clicking it must route to the
 * FeatureSets page for the *same* Space the mapping targets, with the create
 * dialog queued to open on arrival.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { CreateFeatureSetLink } from '@/features/workspaces/CreateFeatureSetLink';
import { useAppStore } from '@/stores';

describe('CreateFeatureSetLink', () => {
  beforeEach(() => {
    useAppStore.setState({
      activeNav: 'workspaces',
      viewSpaceId: null,
      pendingFeatureSetNew: false,
    });
  });

  it('routes to the FeatureSets page for the mapped Space with the create dialog queued', async () => {
    const user = userEvent.setup();
    render(<CreateFeatureSetLink spaceId="space-42" />);

    await user.click(screen.getByTestId('workspace-binding-create-fs'));

    const state = useAppStore.getState();
    expect(state.activeNav).toBe('featuresets');
    // View the Space this mapping targets so the new set can be selected here.
    expect(state.viewSpaceId).toBe('space-42');
    // The FeatureSets page consumes this flag to open its Create dialog.
    expect(state.pendingFeatureSetNew).toBe(true);
  });

  it('is disabled when no Space is chosen — there is nowhere to create the set', () => {
    render(<CreateFeatureSetLink spaceId="" />);
    expect(screen.getByTestId('workspace-binding-create-fs')).toHaveProperty('disabled', true);
  });
});
