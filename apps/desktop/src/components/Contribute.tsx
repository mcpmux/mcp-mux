import { useEffect, useRef, useState } from 'react';
import { Bug, Github, Heart, Lightbulb, Package, SendHorizontal } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import { CONTRIBUTE, openExternal } from '@/lib/contribute';

/**
 * Inline "Didn't find your server?" CTA used on empty search states in the
 * Registry page and the Add-Custom-Server flow.
 *
 * Ships two buttons side-by-side: **Request** (opens a pre-labelled GitHub
 * issue in mcp-servers with the search term in the title) and
 * **Contribute** (opens the mcp-servers CONTRIBUTING guide).
 */
export function RequestServerCTA({
  searchTerm,
  className,
}: {
  searchTerm?: string;
  className?: string;
}) {
  return (
    <div
      className={`rounded-xl border border-primary-200/60 dark:border-primary-800/40 bg-gradient-to-br from-primary-50/50 to-transparent dark:from-primary-900/10 p-4 flex flex-col sm:flex-row items-start sm:items-center gap-3 ${className ?? ''}`}
      data-testid="request-server-cta"
    >
      <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary-500/10 text-primary-600 dark:text-primary-300 flex-shrink-0">
        <Package className="h-4 w-4" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium">Don&apos;t see what you need?</p>
        <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
          {searchTerm
            ? `We couldn't find "${searchTerm}". Request it from the community registry or open a PR yourself.`
            : 'Request a new server in the community registry, or add one yourself via a pull request.'}
        </p>
      </div>
      <div className="flex items-center gap-2 flex-shrink-0">
        <Button
          variant="primary"
          size="sm"
          onClick={() => openExternal(CONTRIBUTE.requestServer(searchTerm))}
          data-testid="request-server-btn"
        >
          <SendHorizontal className="h-3 w-3 mr-1.5" />
          Request
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => openExternal(CONTRIBUTE.contributeServer)}
          data-testid="contribute-server-btn"
        >
          <Github className="h-3 w-3 mr-1.5" />
          Contribute
        </Button>
      </div>
    </div>
  );
}

/**
 * A persistent "Contribute / Report" dropdown menu — the single global
 * affordance for: open GitHub repo, report a bug, request a feature, open
 * the server registry. Place wherever you want a friendly "help make mcpmux
 * better" call-to-action.
 */
export function ContributeMenu({
  variant = 'ghost',
  size = 'sm',
}: {
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md';
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const items = [
    {
      label: 'Request a new server',
      caption: 'Ask the community to add an MCP server to the registry',
      icon: Package,
      href: CONTRIBUTE.requestServer(),
    },
    {
      label: 'Report a bug',
      caption: 'Something broken in the desktop app or gateway',
      icon: Bug,
      href: CONTRIBUTE.bug,
    },
    {
      label: 'Suggest a feature',
      caption: 'An idea for mcpmux itself',
      icon: Lightbulb,
      href: CONTRIBUTE.featureRequest,
    },
    {
      label: 'Open on GitHub',
      caption: 'Browse source, issues, pull requests',
      icon: Github,
      href: CONTRIBUTE.repo,
    },
  ];

  return (
    <div className="relative inline-block" ref={ref}>
      <Button
        variant={variant}
        size={size}
        onClick={() => setOpen((v) => !v)}
        data-testid="contribute-menu-trigger"
      >
        <Heart className="h-4 w-4 mr-1.5" />
        Contribute
      </Button>
      {open && (
        <div
          className="absolute right-0 mt-2 z-20 w-72 rounded-xl border border-[rgb(var(--border))] bg-white dark:bg-zinc-900 shadow-xl p-1"
          data-testid="contribute-menu"
        >
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              onClick={() => {
                setOpen(false);
                openExternal(item.href);
              }}
              className="w-full text-left flex items-start gap-3 px-3 py-2.5 rounded-lg hover:bg-[rgb(var(--surface))] transition-colors"
            >
              <item.icon className="h-4 w-4 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">{item.label}</p>
                <p className="text-[11px] text-[rgb(var(--muted))] leading-snug">
                  {item.caption}
                </p>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
