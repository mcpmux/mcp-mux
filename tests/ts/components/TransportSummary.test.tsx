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

  it('shows the env the server gets and fills input defaults in', () => {
    expect(
      describeLaunch({
        type: 'stdio',
        command: 'npx',
        args: ['-y', 'server', '--region', '${input:REGION}', '--key', '${input:API_KEY}'],
        env: { NODE_OPTIONS: '--require ./hook.js', LOG: '${input:REGION}' },
        metadata: {
          inputs: [
            { id: 'REGION', label: 'Region', default: 'eu' },
            { id: 'API_KEY', label: 'Key', secret: true },
          ],
        },
      })
    ).toBe(
      "LOG=eu NODE_OPTIONS='--require ./hook.js' REGION=eu API_KEY='<your value>' " +
        "npx -y server --region eu --key '${input:API_KEY}'"
    );
  });

  it('keeps hidden characters from hiding or reordering arguments', () => {
    const newlines = describeLaunch({
      type: 'stdio',
      command: 'run',
      args: ['ok' + '\n'.repeat(20) + '--steal'],
      env: {},
      metadata: meta,
    });
    expect(newlines).not.toContain('\n');
    expect(newlines).toContain('\\u{a}');
    expect(newlines).toContain('--steal');

    const bidi = describeLaunch({
      type: 'http',
      url: 'https://safe.example/\u202Etxt.exe',
      headers: {},
      metadata: meta,
    });
    expect(bidi).not.toContain('\u202E');
    expect(bidi).toContain('\\u{202e}');
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
