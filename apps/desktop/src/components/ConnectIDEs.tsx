import { useState, useRef, useEffect } from 'react';
import { Check, Copy, Braces } from 'lucide-react';
import { Card, CardHeader, CardTitle, CardDescription, CardContent, Button } from '@mcpmux/ui';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import { addToVscode, addToCursor } from '@/lib/api/clientInstall';

type GridAction = 'deep_link' | 'copy_command' | 'copy_config';

interface GridEntry {
  id: string;
  name: string;
  label: string;
  icon?: string;
  action: GridAction;
  handler: (() => Promise<void>) | string;
}

interface ConnectIDEsProps {
  gatewayUrl: string;
  gatewayRunning: boolean;
}

export function ConnectIDEs({ gatewayUrl, gatewayRunning }: ConnectIDEsProps) {
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
    },
    {
      id: 'cursor',
      name: 'Cursor',
      label: 'Cursor',
      icon: cursorIcon,
      action: 'deep_link',
      handler: () => addToCursor(gatewayUrl),
    },
    {
      id: 'claude-code',
      name: 'Claude Code',
      label: 'Claude',
      icon: claudeIcon,
      action: 'copy_command',
      handler: `claude mcp add --transport http --scope user mcpmux ${mcpUrl}`,
    },
    {
      id: 'copy-config',
      name: 'JSON Config',
      label: 'JSON',
      action: 'copy_config',
      handler: `"mcpmux": {\n  "type": "http",\n  "url": "${mcpUrl}"\n}`,
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
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle>Connect Your IDEs</CardTitle>
            <CardDescription>
              Add McpMux to your AI clients. Auth happens on first connect.
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
        <div className="flex flex-wrap gap-3" data-testid="client-grid">
          {entries.map((entry) => {
            const isActive = activeId === entry.id;
            const isCopied = copiedId === entry.id;

            return (
              <div key={entry.id} className="relative flex flex-col items-center gap-1" ref={isActive ? popoverRef : undefined}>
                <button
                  type="button"
                  className={`flex items-center justify-center h-10 w-10 rounded-lg border transition-all
                    ${isActive
                      ? 'border-primary-500 bg-primary-500/10 ring-1 ring-primary-500/30'
                      : 'border-[rgb(var(--border))] bg-[var(--surface)] hover:border-primary-400 hover:bg-primary-500/5'
                    }`}
                  title={entry.name}
                  onClick={() => setActiveId(isActive ? null : entry.id)}
                  data-testid={`client-icon-${entry.id}`}
                >
                  {entry.icon ? (
                    <img
                      src={entry.icon}
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

                {/* Popover */}
                {isActive && (
                  <div
                    className="absolute top-full left-0 mt-2 z-10 w-44 rounded-lg border border-[rgb(var(--border))] bg-white dark:bg-zinc-900 shadow-lg p-2.5"
                    data-testid="client-popover"
                  >
                    {/* Arrow */}
                    <div className="absolute -top-1.5 left-4 h-3 w-3 rotate-45 border-l border-t border-[rgb(var(--border))] bg-white dark:bg-zinc-900" />

                    <p className="text-xs font-medium mb-2 text-center relative">
                      {entry.name}
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
                        Copied!
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
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </CardContent>
    </Card>
  );
}
