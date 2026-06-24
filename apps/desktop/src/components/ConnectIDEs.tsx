import { useState, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy, Braces } from 'lucide-react';
import { Card, CardHeader, CardTitle, CardDescription, CardContent, Button } from '@mcpmux/ui';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import jetbrainsIcon from '@/assets/client-icons/jetbrains.svg';
import androidStudioIcon from '@/assets/client-icons/android-studio.svg';
import { addToVscode, addToCursor } from '@/lib/api/clientInstall';
import { isTauri } from '@/lib/backend/shell';

type GridAction = 'deep_link' | 'copy_command' | 'copy_config';

type IdeId =
  | 'vscode'
  | 'cursor'
  | 'windsurf'
  | 'claude-code'
  | 'jetbrains'
  | 'android-studio'
  | 'copy-config';

interface GridEntry {
  id: IdeId;
  icon?: string;
  action: GridAction;
  handler: (() => Promise<void>) | string;
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
  const { t } = useTranslation('common');
  const [activeId, setActiveId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  const mcpUrl = `${gatewayUrl}/mcp`;

  const allEntries: GridEntry[] = [
    {
      id: 'vscode',
      icon: vscodeIcon,
      action: 'deep_link',
      handler: () => addToVscode(gatewayUrl),
    },
    {
      id: 'cursor',
      icon: cursorIcon,
      action: 'deep_link',
      handler: () => addToCursor(gatewayUrl),
    },
    {
      id: 'windsurf',
      icon: windsurfIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "serverUrl": "${mcpUrl}"\n}`,
    },
    {
      id: 'claude-code',
      icon: claudeIcon,
      action: 'copy_command',
      handler: `claude mcp add --transport http --scope user mcpmux ${mcpUrl}`,
    },
    {
      id: 'jetbrains',
      icon: jetbrainsIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "url": "${mcpUrl}"\n}`,
    },
    {
      id: 'android-studio',
      icon: androidStudioIcon,
      action: 'copy_config',
      handler: `"mcpmux": {\n  "httpUrl": "${mcpUrl}"\n}`,
    },
    {
      id: 'copy-config',
      action: 'copy_config',
      handler: `"mcpmux": {\n  "type": "http",\n  "url": "${mcpUrl}"\n}`,
    },
  ];
  const entries = allEntries.filter((entry) => isTauri() || entry.action !== 'deep_link');

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
        const name = t(`connectIdes.ides.${entry.id}.name`);
        const label = t(`connectIdes.ides.${entry.id}.label`);
        const nextStep = t(`connectIdes.ides.${entry.id}.nextStep`);

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
              title={name}
              onClick={() => setActiveId(isActive ? null : entry.id)}
              data-testid={`client-icon-${entry.id}`}
            >
              {entry.icon ? (
                <img src={entry.icon} alt={name} className="h-5 w-5 object-contain" />
              ) : (
                <Braces className="h-4 w-4 text-[rgb(var(--muted))]" />
              )}
            </button>
            <span className="text-[10px] text-[rgb(var(--muted))] leading-none">{label}</span>

            {isActive && (
              <div
                className="absolute bottom-full left-0 mb-2 z-10 w-64 rounded-lg border border-[rgb(var(--border))] bg-white dark:bg-zinc-900 shadow-lg p-3"
                data-testid="client-popover"
              >
                <p className="text-xs font-semibold mb-1 relative">{name}</p>
                <p className="text-[11px] leading-snug text-[rgb(var(--muted))] mb-2.5">{nextStep}</p>

                {entry.action === 'deep_link' ? (
                  <Button
                    variant="primary"
                    size="sm"
                    className="w-full h-7 text-xs relative"
                    disabled={!gatewayRunning}
                    onClick={() => handleDeepLink(entry)}
                  >
                    {t('connectIdes.addTo', { name })}
                  </Button>
                ) : isCopied ? (
                  <div
                    className="flex items-center justify-center gap-1 text-xs text-green-600 h-7 relative"
                    data-testid={entry.id === 'copy-config' ? 'copy-config-copied' : undefined}
                  >
                    <Check className="h-3 w-3" />
                    {t('connectIdes.copied')}
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
                    {entry.action === 'copy_config'
                      ? t('connectIdes.copyConfig')
                      : t('connectIdes.copyCommand')}
                  </Button>
                )}

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
  const { t } = useTranslation('common');

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle>{t('connectIdes.title')}</CardTitle>
            <CardDescription>
              <span className="font-medium">{t('connectIdes.descriptionPrefix')}</span>{' '}
              {t('connectIdes.description')}
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
