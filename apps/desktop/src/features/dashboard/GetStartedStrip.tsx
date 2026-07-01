import { Compass, Monitor, Server, ArrowRight } from 'lucide-react';
import { useNavigate } from '@/hooks/use-navigate.hook';
import type { NavItem } from '@/stores/types';

/**
 * Three-step onboarding shown until the Space has its first installed server.
 */
export function GetStartedStrip() {
  const navigate = useNavigate();
  const steps = [
    {
      n: 1,
      icon: Compass,
      title: 'Pick your first tools',
      desc: 'Browse the registry and install a server in one click.',
      cta: 'Open Discover',
      nav: 'registry' as NavItem,
    },
    {
      n: 2,
      icon: Server,
      title: 'Enable it',
      desc: 'Turn the server on so the gateway can serve its tools.',
      cta: 'Open Tools',
      nav: 'servers' as NavItem,
    },
    {
      n: 3,
      icon: Monitor,
      title: 'Connect an AI app',
      desc: 'Point Cursor, Claude, or VS Code at your gateway below.',
      cta: 'See Apps',
      nav: 'clients' as NavItem,
    },
  ];
  return (
    <div
      className="overflow-hidden rounded-xl border border-[rgb(var(--primary))]/25 bg-[rgb(var(--primary))]/5"
      data-testid="dashboard-get-started"
    >
      <div className="grid grid-cols-1 divide-y divide-[rgb(var(--primary))]/15 md:grid-cols-3 md:divide-x md:divide-y-0">
        {steps.map((s) => (
          <button
            key={s.n}
            type="button"
            onClick={() => navigate(s.nav)}
            className="group flex items-start gap-3 p-4 text-left transition-colors hover:bg-[rgb(var(--primary))]/10"
          >
            <span className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--primary))]/15 text-sm font-bold text-[rgb(var(--primary))]">
              {s.n}
            </span>
            <span className="min-w-0">
              <span className="flex items-center gap-1.5 text-sm font-semibold">
                <s.icon className="h-3.5 w-3.5 text-[rgb(var(--primary))]" />
                {s.title}
              </span>
              <span className="mt-0.5 block text-xs leading-relaxed text-[rgb(var(--muted))]">
                {s.desc}
              </span>
              <span className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-[rgb(var(--primary))]">
                {s.cta}
                <ArrowRight className="h-3 w-3 transition-transform group-hover:translate-x-0.5" />
              </span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
