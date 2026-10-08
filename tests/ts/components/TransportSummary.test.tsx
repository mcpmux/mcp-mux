import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TransportSummary } from '@/components/TransportSummary';
import { describeLaunch } from '@/lib/serverLaunch';

const meta = { inputs: [] };

describe('describeLaunch', () => {
  it('joins a command line, quoting what a shell would split', () => {
    expect(
      describeLaunch({
        type: 'stdio',
        command: 'docker',
        args: ['run', '-i', '--rm', 'ghcr.io/acme/mcp:1.2', 'a b', "it's"],
        env: {},
        metadata: meta,
      })
    ).toBe(`docker run -i --rm ghcr.io/acme/mcp:1.2 'a b' 'it'\\''s'`);
  });

  it('returns the URL of a remote server', () => {
    expect(
      describeLaunch({ type: 'http', url: 'https://x.example/mcp', headers: {}, metadata: meta })
    ).toBe('https://x.example/mcp');
  });
});

describe('TransportSummary', () => {
  it('renders registry text as text, not markup', () => {
    render(
      <TransportSummary
        transport={{
          type: 'stdio',
          command: '<img src=x onerror=alert(1)>',
          args: [],
          env: {},
          metadata: meta,
        }}
      />
    );
    const summary = screen.getByTestId('transport-summary');
    expect(summary.querySelector('img')).toBeNull();
    expect(summary).toHaveTextContent('<img src=x onerror=alert(1)>');
  });
});
