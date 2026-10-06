/**
 * What the Home page says about the Starter: how many tools apps get, and —
 * past the size warning — how to slim it down (ask @mux, or create a
 * FeatureSet). It's guidance only; nothing here caps tools.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { mockGoCreate, mockNavigate } = vi.hoisted(() => ({
  mockGoCreate: vi.fn(),
  mockNavigate: vi.fn(),
}));

vi.mock('@/hooks/useCrossTabNav', () => ({
  useGoCreateFeatureSet: () => mockGoCreate,
}));
vi.mock('@/stores', () => ({
  useNavigateTo: () => mockNavigate,
}));

import { StarterToolsCard } from '@/components/StarterToolsNotice';
import type { StarterToolSummary } from '@/lib/api/featureSets';

function summary(overrides: Partial<StarterToolSummary> = {}): StarterToolSummary {
  return {
    feature_set_id: 'fs_default_s1',
    auto_include: true,
    tool_count: 42,
    server_count: 3,
    threshold: 80,
    over_threshold: false,
    ...overrides,
  };
}

describe('StarterToolsCard', () => {
  beforeEach(() => vi.clearAllMocks());

  it('tells a new user what their apps get from an automatic Starter', async () => {
    const user = userEvent.setup();
    render(<StarterToolsCard summary={summary()} spaceId="s1" />);

    expect(screen.getByTestId('starter-tools-card-title')).toHaveTextContent(
      'Your apps get 42 tools from 3 servers'
    );
    expect(screen.getByText(/new servers show up on their own/i)).toBeInTheDocument();
    expect(screen.queryByTestId('starter-tools-warning')).not.toBeInTheDocument();

    await user.click(screen.getByTestId('starter-tools-card'));
    expect(mockNavigate).toHaveBeenCalledWith('featuresets');
  });

  it('describes a manual Starter as the user’s own pick', () => {
    render(
      <StarterToolsCard
        summary={summary({ auto_include: false, tool_count: 1, server_count: 1 })}
        spaceId="s1"
      />
    );
    expect(screen.getByTestId('starter-tools-card-title')).toHaveTextContent(
      'Your apps get 1 tool from 1 server'
    );
    expect(screen.getByText(/tools you picked/i)).toBeInTheDocument();
  });

  it('renders nothing while there are no tools yet', () => {
    const { container } = render(
      <StarterToolsCard summary={summary({ tool_count: 0, server_count: 0 })} spaceId="s1" />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('warns past the threshold and offers @mux or a new FeatureSet', async () => {
    const user = userEvent.setup();
    render(
      <StarterToolsCard
        summary={summary({ tool_count: 112, server_count: 9, over_threshold: true })}
        spaceId="s1"
      />
    );

    const warning = screen.getByTestId('starter-tools-warning');
    expect(warning).toHaveTextContent('Your apps are getting 112 tools');
    expect(warning).toHaveTextContent('past 80 tools');
    expect(screen.getByTestId('starter-tools-warning-copy')).toBeInTheDocument();
    expect(screen.queryByTestId('starter-tools-card')).not.toBeInTheDocument();

    await user.click(screen.getByTestId('starter-tools-warning-create'));
    expect(mockGoCreate).toHaveBeenCalledWith('s1');
  });
});
