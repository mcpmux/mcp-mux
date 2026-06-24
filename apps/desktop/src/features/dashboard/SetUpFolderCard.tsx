import { ArrowRight, FolderPlus } from 'lucide-react';
import { useNavigateTo, useSetPendingWorkspaceNew } from '@/stores';

/**
 * Per-folder setup entry point. ConnectionCard connects an app to the
 * gateway globally; this routes into the Workspaces walkthrough to map a
 * specific project and write its per-folder config.
 */
export function SetUpFolderCard() {
  const navigateTo = useNavigateTo();
  const openWizard = useSetPendingWorkspaceNew();
  return (
    <button
      type="button"
      onClick={() => {
        openWizard(true);
        navigateTo('workspaces');
      }}
      data-testid="dashboard-setup-folder"
      className="group flex w-full items-center gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:border-[rgb(var(--border))] hover:shadow-md"
    >
      <span className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--primary))]/12 text-[rgb(var(--primary))]">
        <FolderPlus className="h-5 w-5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-semibold">Set up a folder</span>
        <span className="block text-xs text-[rgb(var(--muted))]">
          Map a project to its tools and connect your apps to it — even ones that don&apos;t report
          the folder.
        </span>
      </span>
      <ArrowRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] transition-transform group-hover:translate-x-0.5" />
    </button>
  );
}
