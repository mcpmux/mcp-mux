import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { ContributeMenu, RequestServerCTA } from '../../../apps/desktop/src/components/Contribute';
import { renderWithI18n } from '../render-with-i18n.helpers';

vi.mock('@/lib/contribute', () => ({
  CONTRIBUTE: {
    requestServer: () => 'https://example.com/request',
    contributeServer: 'https://example.com/contribute',
    bug: 'https://example.com/bug',
    featureRequest: 'https://example.com/feature',
    repo: 'https://example.com/repo',
  },
  openExternal: vi.fn(),
}));

describe('RequestServerCTA', () => {
  it('renders default copy without a search term', () => {
    renderWithI18n(<RequestServerCTA />);

    expect(screen.getByText("Don't see what you need?")).toBeInTheDocument();
    expect(
      screen.getByText(
        'Request a new server in the community registry, or add one yourself via a pull request.',
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId('request-server-btn')).toHaveTextContent('Request');
    expect(screen.getByTestId('contribute-server-btn')).toHaveTextContent('Contribute');
  });

  it('renders search-specific copy when a search term is provided', () => {
    renderWithI18n(<RequestServerCTA searchTerm="notion" />);

    expect(
      screen.getByText(
        'We couldn\'t find "notion". Request it from the community registry or open a PR yourself.',
      ),
    ).toBeInTheDocument();
  });
});

describe('ContributeMenu', () => {
  it('renders the contribute trigger label', () => {
    renderWithI18n(<ContributeMenu />);

    expect(screen.getByTestId('contribute-menu-trigger')).toHaveTextContent('Contribute');
  });
});
