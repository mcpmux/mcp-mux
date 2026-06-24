import { useState, useRef, useEffect } from 'react';
import { Check, Copy, Braces } from 'lucide-react';
import { Card, CardHeader, CardTitle, CardDescription, CardContent, Button } from '@mcpmux/ui';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import jetbrainsIcon from '@/assets/client-icons/jetbrains.svg';
import androidStudioIcon from '@/assets/client-icons/android-studio.svg';
import opencodeIcon from '@/assets/client-icons/opencode.svg';
import opencodeIconDark from '@/assets/client-icons/opencode-dark.svg';
import { addToVscode, addToCursor } from '@/lib/api/clientInstall';
import { ClientBrandIcon } from './ClientBrandIcon';

type GridAction = 'deep_link' | 'copy_command' | 'copy_config';

interface GridEntry {
  id: string;
  name: string;
  label: string;
  icon?: string;
  /** Optional dark-theme variant; rendered via ClientBrandIcon when present. */
  iconDark?: string;
  action: GridAction;
  handler: (() => Promise<void>) | string;
  /**
   * Per-IDE, what does the user actually have to do after the button fires?
   * Each IDE's "make MCP server live" flow is different — VS Code auto-starts
   * while Cursor needs the server toggled on, for example. Keep this wording
   * specific; a generic "restart" message has already misled testers.
   */
  nextStep: string;
}

interface ConnectIDEsGridProps {
  gatewayUrl: string;
  gatewayRunning: boolean;
}

/**
 * Chromeless grid of IDE connect shortcuts. Used directly by the dashboard
 * ConnectionCard (which owns the surrounding chrome) and wrapped by
 * `ConnectIDEs` below for the Clients page standalone usage.
 */
export function ConnectIDEsGrid({ gatewayUrl, gatewayRunning }: ConnectIDEsGridProps) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  const mcpUrl = `${gatewayUrl}/mcp`;

  const entries: GridEntry[] = [
    {
      id: 'vscode',
      name: 'VS Code',
      label: 'VS Code',
      icon: vscodeIcon,
      action: 'deep_link',
      handler: () => addToVscode(gatewayUrl),
      nextStep:
        'Opens VS Code and drops mcpmux into mcp.json. VS Code starts the server ' +
        'automatically — if it doesn’t, open the Command Palette and run ' +
        '"MCP: Show Installed Servers", then click Start on mcpmux. The approval ' +
        'prompt lands on this page.',
    },
    {
      id: 'cursor',
      name: 'Cursor',
      label: 'Cursor',
      icon: cursorIcon,
      action: 'deep_link',
      handler: () => addToCursor(gatewayUrl),
      nextStep:
        'Opens Cursor and adds mcpmux to its config. Cursor does not auto-start ' +
        'new MCP servers — go to Settings → Features → MCP (or the MCP ' +
        'Tools panel) and toggle mcpmux on. The approval prompt lands on this page.',
    },
    {
      id: 'windsurf',
      name: 'Windsurf',
      label: 'Windsurf',
      icon: windsurfIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "serverUrl": "${mcpUrl}"\n}`,
      nextStep:
        'Copies a JSON snippet. In Windsurf, open Cascade → MCP settings, ' +
        'paste mcpmux under mcpServers, and hit "Refresh" (or reload Windsurf). ' +
        'Approve on this page when Windsurf reaches the gateway.',
    },
    {
      id: 'claude-code',
      name: 'Claude Code',
      label: 'Claude',
      icon: claudeIcon,
      action: 'copy_command',
      handler: `claude mcp add --transport http --scope user mcpmux ${mcpUrl}`,
      nextStep:
        'Copies a `claude mcp add` command. Run it in your shell — Claude Code ' +
        'loads mcpmux on the next `claude` invocation (existing sessions need ' +
        '/restart). Approve on this page when it connects.',
    },
    {
      id: 'opencode',
      name: 'opencode',
      label: 'opencode',
      icon: opencodeIcon,
      iconDark: opencodeIconDark,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "type": "remote",\n  "url": "${mcpUrl}"\n}`,
      nextStep:
        'Copies a JSON snippet. In opencode, paste it under "mcp" in opencode.json ' +
        '(project) or ~/.config/opencode/opencode.json (global), then restart ' +
        'opencode. Approve on this page when it connects.',
    },
    {
      id: 'jetbrains',
      name: 'JetBrains IDEs',
      label: 'JetBrains',
      icon: jetbrainsIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "url": "${mcpUrl}"\n}`,
      nextStep:
        'Copies a JSON snippet. Paste into the AI Assistant MCP config, then ' +
        'restart the IDE — JetBrains only reads MCP config on startup. Approve ' +
        'on this page.',
    },
    {
      id: 'android-studio',
      name: 'Android Studio',
      label: 'Android',
      icon: androidStudioIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "httpUrl": "${mcpUrl}"\n}`,
      nextStep:
        'Copies a JSON snippet. Paste into Android Studio’s AI Assistant MCP ' +
        'config, then restart the IDE. Approve on this page.',
    },
    {
      id: 'copy-config',
      name: 'JSON Config',
      label: 'JSON',
      action: 'copy_config',
      handler: `"mcpmux": {\n  "type": "http",\n  "url": "${mcpUrl}"\n}`,
      nextStep:
        'Copies a generic MCP JSON snippet. Paste into any MCP-compatible client ' +
        'and follow its reload instructions. Approve on this page when it connects.',
    },
  ];

  // Close popover on outside click
  useEffect(() => {
    if (!activeId) return;
    const handleClick = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setActiveId(null);
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [activeId]);

  const handleDeepLink = async (entry: GridEntry) => {
    if (typeof entry.handler === 'function') {
      await entry.handler();
    }
    setActiveId(null);
  };

  const handleCopy = async (entry: GridEntry) => {
    if (typeof entry.handler === 'string') {
      await navigator.clipboard.writeText(entry.handler);
      setCopiedId(entry.id);
      setTimeout(() => {
        setCopiedId(null);
        setActiveId(null);
      }, 1500);
    }
  };

  return (
    <div className="flex flex-wrap gap-3" data-testid="client-grid">
      {entries.map((entry) => {
        const isActive = activeId === entry.id;
        const isCopied = copiedId === entry.id;

        return (
          <div
            key={entry.id}
            className="relative flex flex-col items-center gap-1"
            ref={isActive ? popoverRef : undefined}
          >
            <button
              type="button"
              className={`flex items-center justify-center h-10 w-10 rounded-lg border transition-all
                ${
                  isActive
                    ? 'border-primary-500 bg-primary-500/10 ring-1 ring-primary-500/30'
                    : 'border-[rgb(var(--border))] bg-[var(--surface)] hover:border-primary-400 hover:bg-primary-500/5'
                }`}
              title={entry.name}
              onClick={() => setActiveId(isActive ? null : entry.id)}
              data-testid={`client-icon-${entry.id}`}
            >
              {entry.icon ? (
                <ClientBrandIcon
                  light={entry.icon}
                  dark={entry.iconDark}
                  alt={entry.name}
                  className="h-5 w-5 object-contain"
                />
              ) : (
                <Braces className="h-4 w-4 text-[rgb(var(--muted))]" />
              )}
            </button>
            <span className="text-[10px] text-[rgb(var(--muted))] leading-none">
              {entry.label}
            </span>

            {/* Popover — opens UPWARD. The grid usually sits at the
                bottom of a Card (Dashboard + Clients empty state), so
                opening downward put the action button below the scroll
                viewport on first paint, forcing users to scroll to find
                it. Anchor to the bottom of the trigger button instead. */}
            {isActive && (
              <div
                className="absolute bottom-full left-0 mb-2 z-10 w-64 rounded-lg border border-[rgb(var(--border))] bg-white dark:bg-zinc-900 shadow-lg p-3"
                data-testid="client-popover"
              >
                <p className="text-xs font-semibold mb-1 relative">{entry.name}</p>

                {/* Per-IDE instructions. Not a switch on action type —
                    each IDE's post-install step is meaningfully different
                    (VS Code auto-starts, Cursor needs explicit toggle,
                    JetBrains needs a full restart, etc.). */}
                <p className="text-[11px] leading-snug text-[rgb(var(--muted))] mb-2.5">
                  {entry.nextStep}
                </p>

                {entry.action === 'deep_link' ? (
                  <Button
                    variant="primary"
                    size="sm"
                    className="w-full h-7 text-xs relative"
                    disabled={!gatewayRunning}
                    onClick={() => handleDeepLink(entry)}
                  >
                    Add to {entry.name}
                  </Button>
                ) : isCopied ? (
                  <div className="flex items-center justify-center gap-1 text-xs text-green-600 h-7 relative">
                    <Check className="h-3 w-3" />
                    Copied — paste &amp; follow above
                  </div>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    className="w-full h-7 text-xs gap-1 relative"
                    onClick={() => handleCopy(entry)}
                    data-testid={entry.id === 'copy-config' ? 'copy-config-btn' : undefined}
                  >
                    <Copy className="h-3 w-3" />
                    {entry.action === 'copy_config' ? 'Copy config' : 'Copy command'}
                  </Button>
                )}

                {/* Arrow — points down from the popover to the trigger
                    icon below. */}
                <div className="absolute -bottom-1.5 left-4 h-3 w-3 rotate-45 border-r border-b border-[rgb(var(--border))] bg-white dark:bg-zinc-900" />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

interface ConnectIDEsProps {
  gatewayUrl: string;
  gatewayRunning: boolean;
}

/**
 * Standalone Card-wrapped IDE grid. Used by the Clients page where it lives
 * on its own. The dashboard uses the chromeless `ConnectIDEsGrid` inside the
 * canonical ConnectionCard instead.
 */
export function ConnectIDEs({ gatewayUrl, gatewayRunning }: ConnectIDEsProps) {
  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle>Connect Your IDEs</CardTitle>
            <CardDescription>
              <span className="font-medium">VS Code &amp; Cursor</span> are one-click; the rest copy
              a config you paste into their MCP settings. Either path ends with an approval
              prompt in this app.
            </CardDescription>
          </div>
          <div className="flex items-center gap-1.5 text-xs text-[rgb(var(--muted))]">
            <span
              className={`h-2 w-2 rounded-full ${gatewayRunning ? 'bg-green-500' : 'bg-orange-500'}`}
            />
            <code className="text-primary-500">{gatewayUrl}</code>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <ConnectIDEsGrid gatewayUrl={gatewayUrl} gatewayRunning={gatewayRunning} />
      </CardContent>
    </Card>
  );
}
