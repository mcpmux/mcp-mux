import { useState, useEffect } from 'react';
import {
  X,
  Loader2,
  Search,
  Server,
  Wrench,
  MessageSquare,
  FileText,
  Package,
  ChevronDown,
  ChevronRight,
  ToggleLeft,
  ToggleRight,
  Settings,
  Trash2,
  Check,
  Star,
  Shield,
  Save,
  Zap,
  AlertTriangle,
} from 'lucide-react';
import { Button, Switch, useToast, ToastContainer, useConfirm } from '@mcpmux/ui';
import type { FeatureSet, AddMemberInput } from '@/lib/api/featureSets';
import {
  isStarterFeatureSet,
  setFeatureSetAutoInclude,
  setFeatureSetMembers,
} from '@/lib/api/featureSets';
import { useStarterToolSummary } from '@/hooks/useStarterToolSummary';
import { MuxPromptCode } from '@/components/MuxPrompt';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { listServerFeatures } from '@/lib/api/serverFeatures';

interface FeatureSetPanelProps {
  featureSet: FeatureSet;
  spaceId: string;
  onClose: () => void;
  onDelete?: (id: string) => void;
  onUpdate?: () => void;
}

interface ServerGroup {
  serverId: string;
  features: ServerFeature[];
  isExpanded: boolean;
}

export function FeatureSetPanel({
  featureSet,
  spaceId,
  onClose,
  onDelete,
  onUpdate,
}: FeatureSetPanelProps) {
  const [allFeatures, setAllFeatures] = useState<ServerFeature[]>([]);
  const [selectedFeatureIds, setSelectedFeatureIds] = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedServers, setExpandedServers] = useState<Set<string>>(new Set());
  // Auto mode (every server's tools). Editing the selection while it's on
  // and saving switches the set to a manual selection.
  const [autoInclude, setAutoInclude] = useState(featureSet.auto_include);
  const [isTogglingAuto, setIsTogglingAuto] = useState(false);
  const [selectionEdited, setSelectionEdited] = useState(false);
  const { summary: starterSummary } = useStarterToolSummary(spaceId);
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();

  // Collapsible sections - only one expanded at a time, features by default
  const [expandedSections, setExpandedSections] = useState({
    settings: false,
    features: true,
  });

  // Both FS types are member-driven now.
  const isConfigurable = true;
  // The auto-seeded "Starter" FS has editable membership like a Custom one
  // (change which tools it includes, or empty it). What's locked is its
  // identity + lifecycle: it's the default fallback for unmapped folders, so
  // its name is fixed (the backend ignores name changes on builtin rows) and
  // it can't be deleted — the Delete action below is gated to Custom sets.
  const isStarter = isStarterFeatureSet(featureSet);
  const isCustom = featureSet.feature_set_type === 'custom';

  const getActualMemberCount = () => selectedFeatureIds.size;

  const isFeatureSelected = (featureId: string, _feature: ServerFeature) =>
    selectedFeatureIds.has(featureId);

  useEffect(() => {
    const loadFeatures = async () => {
      setIsLoading(true);
      try {
        const features = await listServerFeatures(spaceId);
        setAllFeatures(features);

        // Seed from the set's include-mode feature members — or, in auto
        // mode, everything (that's what the set grants).
        const currentIds = new Set<string>();
        if (featureSet.auto_include) {
          features.forEach((f) => currentIds.add(f.id));
        } else {
          featureSet.members?.forEach((m) => {
            if (m.member_type === 'feature' && m.mode === 'include') {
              currentIds.add(m.member_id);
            }
          });
        }

        setSelectedFeatureIds(currentIds);
        setAutoInclude(featureSet.auto_include);
        setSelectionEdited(false);

        // Start with all servers collapsed
        setExpandedServers(new Set());
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setIsLoading(false);
      }
    };

    loadFeatures();
  }, [spaceId, featureSet]);

  // Group features by server
  const serverGroups: ServerGroup[] = allFeatures.reduce((acc, feature) => {
    const group = acc.find((g) => g.serverId === feature.server_id);
    if (group) {
      group.features.push(feature);
    } else {
      acc.push({
        serverId: feature.server_id,
        features: [feature],
        isExpanded: expandedServers.has(feature.server_id),
      });
    }
    return acc;
  }, [] as ServerGroup[]);

  // Filter by search
  const filteredGroups = serverGroups
    .map((group) => ({
      ...group,
      features: group.features.filter(
        (f) =>
          f.feature_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
          f.display_name?.toLowerCase().includes(searchQuery.toLowerCase()) ||
          f.description?.toLowerCase().includes(searchQuery.toLowerCase())
      ),
    }))
    .filter((group) => group.features.length > 0);

  const toggleFeature = (featureId: string) => {
    if (!isConfigurable) return;
    setSelectionEdited(true);
    setSelectedFeatureIds((prev) => {
      const next = new Set(prev);
      if (next.has(featureId)) {
        next.delete(featureId);
      } else {
        next.add(featureId);
      }
      return next;
    });
  };

  const toggleServer = (serverId: string) => {
    setExpandedServers((prev) => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
      }
      return next;
    });
  };

  const toggleAllInServer = (serverId: string) => {
    if (!isConfigurable) return;
    const serverFeatures = allFeatures.filter((f) => f.server_id === serverId);
    const allSelected = serverFeatures.every((f) => selectedFeatureIds.has(f.id));
    setSelectionEdited(true);

    setSelectedFeatureIds((prev) => {
      const next = new Set(prev);
      serverFeatures.forEach((f) => {
        if (allSelected) {
          next.delete(f.id);
        } else {
          next.add(f.id);
        }
      });
      return next;
    });
  };

  const handleAutoToggle = async (enabled: boolean) => {
    if (
      enabled &&
      !(await confirm({
        title: "Include every server's tools?",
        message: `"${featureSet.name}" will grant every tool, prompt, and resource from every server in this Space — including servers you add later. Your current selection is replaced.`,
        confirmLabel: 'Include everything',
      }))
    ) {
      return;
    }
    setIsTogglingAuto(true);
    setError(null);
    try {
      await setFeatureSetAutoInclude(featureSet.id, enabled);
      setAutoInclude(enabled);
      setSelectionEdited(false);
      if (enabled) setSelectedFeatureIds(new Set(allFeatures.map((f) => f.id)));
      success(
        enabled ? 'Every tool, automatically' : 'You pick the tools now',
        enabled
          ? `"${featureSet.name}" now includes every server's tools.`
          : `"${featureSet.name}" keeps its current tools — change the selection below.`
      );
      onUpdate?.();
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to update', errorMsg);
    } finally {
      setIsTogglingAuto(false);
    }
  };

  // Tools (not prompts/resources) the current selection would serve — the
  // number AI apps feel, compared against the size warning.
  const selectedToolCount = allFeatures.filter(
    (f) => f.feature_type === 'tool' && f.is_available && selectedFeatureIds.has(f.id)
  ).length;
  const toolThreshold = starterSummary?.threshold;
  const overToolThreshold = toolThreshold !== undefined && selectedToolCount > toolThreshold;

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);
    try {
      // Update members
      const members: AddMemberInput[] = Array.from(selectedFeatureIds).map((id) => ({
        member_type: 'feature' as const,
        member_id: id,
        mode: 'include' as const,
      }));

      await setFeatureSetMembers(featureSet.id, members);
      setAutoInclude(false);
      setSelectionEdited(false);

      success(
        'Changes saved',
        `"${featureSet.name}" has been updated with ${members.length} feature${members.length !== 1 ? 's' : ''}`
      );
      onUpdate?.();
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to save changes', errorMsg);
    } finally {
      setIsSaving(false);
    }
  };

  const getFeatureIcon = (type: string) => {
    switch (type) {
      case 'tool':
        return <Wrench className="h-4 w-4 text-purple-500" />;
      case 'prompt':
        return <MessageSquare className="h-4 w-4 text-blue-500" />;
      case 'resource':
        return <FileText className="h-4 w-4 text-green-500" />;
      default:
        return <Package className="h-4 w-4 text-gray-500" />;
    }
  };

  const getTypeColor = (type: string) => {
    switch (type) {
      case 'tool':
        return 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300';
      case 'prompt':
        return 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300';
      case 'resource':
        return 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300';
      default:
        return 'bg-gray-100 dark:bg-gray-800 text-gray-700 dark:text-gray-300';
    }
  };

  const getFeatureSetIcon = () => {
    if (featureSet.icon) return <span className="text-xl">{featureSet.icon}</span>;
    switch (featureSet.feature_set_type) {
      case 'default':
        return <Star className="h-6 w-6 text-yellow-500" />;
      case 'custom':
      default:
        return <Package className="h-6 w-6 text-purple-500" />;
    }
  };

  const toggleSection = (section: keyof typeof expandedSections) => {
    setExpandedSections((prev) => {
      // Accordion behavior - close others when opening a section
      if (!prev[section]) {
        return { settings: false, features: false, [section]: true };
      }
      // Allow closing the current section
      return { ...prev, [section]: false };
    });
  };

  return (
    <div className="animate-in slide-in-from-right fixed bottom-0 right-0 top-0 z-50 flex w-full min-w-[600px] max-w-[45%] flex-col border-l border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl duration-300">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}
      {/* Panel Header */}
      <div className="flex-shrink-0 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <div className="mb-3 flex items-start justify-between">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))]">
              {getFeatureSetIcon()}
            </div>
            <div className="min-w-0 flex-1">
              <h2 className="flex items-center gap-2 truncate text-lg font-bold">
                {featureSet.name}
              </h2>
              <div className="mt-0.5 flex items-center gap-2">
                <span
                  title={
                    isStarter
                      ? "Auto-created with this Space. The default set for folders you haven't mapped — edit which tools it includes; its name is fixed and it can't be deleted."
                      : undefined
                  }
                  className={`rounded-full border px-1.5 py-0.5 text-[10px] font-medium ${
                    isStarter
                      ? 'border-yellow-200 bg-yellow-50 text-yellow-700 dark:border-yellow-800 dark:bg-yellow-900/20 dark:text-yellow-400'
                      : isCustom
                        ? 'border-purple-200 bg-purple-50 text-purple-700 dark:border-purple-800 dark:bg-purple-900/20 dark:text-purple-400'
                        : 'border-gray-200 bg-gray-50 text-gray-700 dark:border-gray-800 dark:bg-gray-900/20 dark:text-gray-400'
                  }`}
                >
                  {isStarter ? 'STARTER' : featureSet.feature_set_type.toUpperCase()}
                </span>
                <span className="truncate text-xs text-[rgb(var(--muted))]">
                  ID: {featureSet.id}
                </span>
              </div>
            </div>
          </div>
          <button
            data-testid="featureset-panel-close"
            onClick={onClose}
            className="flex-shrink-0 rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))]"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      {/* Scrollable Content */}
      <div className="flex-1 overflow-y-auto">
        <div className="space-y-5 p-6">
          {/* Error */}
          {error && (
            <div className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-600 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
              {error}
            </div>
          )}

          {/* Info Section (Read-only for non-custom/default) */}
          <div className="overflow-hidden rounded-xl border-2 border-[rgb(var(--border))] bg-[rgb(var(--background))]">
            <button
              onClick={() => toggleSection('settings')}
              className={`flex w-full items-center justify-between p-4 transition-all ${
                expandedSections.settings
                  ? 'from-primary-50 to-primary-100/50 dark:from-primary-900/10 dark:to-primary-800/10 bg-gradient-to-r'
                  : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              <div className="flex items-center gap-3">
                <div
                  className={`rounded-lg p-2 ${
                    expandedSections.settings
                      ? 'bg-gray-500 text-white'
                      : 'bg-gray-100 text-gray-600 dark:bg-gray-900/30 dark:text-gray-400'
                  }`}
                >
                  <Settings className="h-5 w-5" />
                </div>
                <span className="text-base font-semibold">General Information</span>
              </div>
              {expandedSections.settings ? (
                <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
              ) : (
                <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
              )}
            </button>

            {expandedSections.settings && (
              <div className="space-y-4 border-t-2 border-[rgb(var(--border))] bg-white p-4 dark:bg-[rgb(var(--background))]">
                <div>
                  <label className="mb-1.5 block text-xs font-medium text-[rgb(var(--muted))]">
                    Description
                  </label>
                  <p className="text-sm">{featureSet.description || 'No description provided.'}</p>
                </div>

                {isStarter && (
                  <div className="rounded-lg border border-yellow-200 bg-yellow-50 p-3 dark:border-yellow-800 dark:bg-yellow-900/10">
                    <div className="flex gap-2">
                      <Star className="mt-0.5 h-4 w-4 flex-shrink-0 text-yellow-500" />
                      <div className="text-xs text-yellow-800 dark:text-yellow-200">
                        <strong>Starter FeatureSet:</strong> auto-created with this Space and used
                        as the <em>default</em> for folders you haven&apos;t explicitly mapped (and
                        rootless sessions). Edit which tools it includes (or empty it) to change
                        what they get. Its name is fixed and it{' '}
                        <strong>can&apos;t be deleted</strong>, since the fallback always needs a
                        stable target.
                      </div>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Auto mode: every server's tools, including servers added later */}
          <div
            className={`rounded-xl border-2 p-4 ${
              autoInclude
                ? 'border-emerald-300 bg-emerald-50/60 dark:border-emerald-700/60 dark:bg-emerald-900/15'
                : 'border-[rgb(var(--border))] bg-[rgb(var(--background))]'
            }`}
            data-testid="featureset-auto-card"
          >
            <div className="flex items-start justify-between gap-4">
              <div className="flex min-w-0 items-start gap-3">
                <Zap
                  className={`mt-0.5 h-5 w-5 flex-shrink-0 ${
                    autoInclude
                      ? 'text-emerald-600 dark:text-emerald-400'
                      : 'text-[rgb(var(--muted))]'
                  }`}
                />
                <div>
                  <p className="text-sm font-semibold">Include every server&apos;s tools</p>
                  <p className="mt-0.5 text-xs leading-relaxed text-[rgb(var(--muted))]">
                    {autoInclude
                      ? 'On — servers you add later show up here on their own. Change the selection below and save to pick tools yourself.'
                      : 'Off — this set grants only the tools selected below. Turn on to include every server, now and later.'}
                  </p>
                </div>
              </div>
              <Switch
                checked={autoInclude}
                onCheckedChange={handleAutoToggle}
                disabled={isTogglingAuto || isSaving}
                data-testid="featureset-auto-switch"
              />
            </div>
          </div>

          {overToolThreshold && (
            <div
              className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 dark:border-amber-700/60 dark:bg-amber-900/20"
              data-testid="featureset-panel-tools-warning"
            >
              <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
              <div className="min-w-0 flex-1 text-xs leading-relaxed text-amber-800 dark:text-amber-200">
                <p className="text-sm font-semibold text-amber-900 dark:text-amber-100">
                  {selectedToolCount} tools selected — more than the {toolThreshold} AI apps handle
                  well
                </p>
                <p className="mt-0.5">
                  Everything still works, but apps get slower and less accurate. Untick what this
                  set doesn&apos;t need, or ask your AI app to build a focused set:
                </p>
                <div className="mt-2">
                  <MuxPromptCode testId="featureset-panel-tools-warning-copy" />
                </div>
              </div>
            </div>
          )}

          {/* Feature Selection Section */}
          <div className="overflow-hidden rounded-xl border-2 border-[rgb(var(--border))] bg-[rgb(var(--background))]">
            <button
              onClick={() => toggleSection('features')}
              className={`flex w-full items-center justify-between p-4 transition-all ${
                expandedSections.features
                  ? 'bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20'
                  : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              <div className="flex flex-1 items-center gap-3">
                <div
                  className={`rounded-lg p-2 ${
                    expandedSections.features
                      ? 'bg-blue-500 text-white'
                      : 'bg-blue-100 text-blue-600 dark:bg-blue-900/30 dark:text-blue-400'
                  }`}
                >
                  <Shield className="h-5 w-5" />
                </div>
                <div className="flex-1">
                  <div className="mb-1 flex items-center gap-2">
                    <span className="text-base font-semibold">Included Features</span>
                    {/* Show count badge only for configurable feature sets */}
                    {isConfigurable && (
                      <span
                        className={`rounded-full px-2.5 py-1 text-xs font-bold ${
                          getActualMemberCount() > 0
                            ? 'border border-green-300 bg-green-100 text-green-700 dark:border-green-700 dark:bg-green-900/30 dark:text-green-300'
                            : 'border border-gray-300 bg-gray-100 text-gray-600 dark:border-gray-700 dark:bg-gray-900/30 dark:text-gray-400'
                        }`}
                      >
                        {getActualMemberCount()} / {allFeatures.length} selected
                      </span>
                    )}
                  </div>
                  {/* Progress Bar */}
                  <div className="h-1.5 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
                    <div
                      className={`h-full transition-all duration-300 ${
                        getActualMemberCount() === 0
                          ? 'bg-gray-400 dark:bg-gray-600'
                          : 'bg-gradient-to-r from-green-500 to-blue-500'
                      }`}
                      style={{
                        width: `${allFeatures.length > 0 ? (getActualMemberCount() / allFeatures.length) * 100 : 0}%`,
                      }}
                    />
                  </div>
                </div>
              </div>
              {expandedSections.features ? (
                <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
              ) : (
                <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
              )}
            </button>

            {expandedSections.features && (
              <div className="flex h-[500px] flex-col border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                {/* Search Bar inside panel */}
                <div className="border-b border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3">
                  <div className="relative">
                    <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[rgb(var(--muted))]" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder="Search features..."
                      className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] py-2 pl-9 pr-3 text-sm focus:outline-none focus:ring-2"
                    />
                  </div>
                </div>

                <div className="flex-1 overflow-y-auto">
                  {isLoading ? (
                    <div className="flex h-full items-center justify-center">
                      <Loader2 className="text-primary-500 h-8 w-8 animate-spin" />
                    </div>
                  ) : filteredGroups.length === 0 ? (
                    <div className="flex h-full flex-col items-center justify-center p-4 text-center text-[rgb(var(--muted))]">
                      <Package className="mb-2 h-8 w-8 opacity-50" />
                      <p className="text-sm">No features found matching your search</p>
                    </div>
                  ) : (
                    <div className="divide-y divide-[rgb(var(--border))]">
                      {filteredGroups.map((group) => {
                        // For special sets, use isFeatureSelected logic
                        const selectedCount = group.features.filter((f) =>
                          isFeatureSelected(f.id, f)
                        ).length;
                        const allSelected = selectedCount === group.features.length;
                        const someSelected =
                          selectedCount > 0 && selectedCount < group.features.length;
                        const isExpanded = group.isExpanded;

                        return (
                          <div key={group.serverId} className="bg-[rgb(var(--surface))]">
                            <div
                              className="flex cursor-pointer items-center justify-between px-4 py-3 transition-colors hover:bg-[rgb(var(--surface-hover))]"
                              onClick={() => toggleServer(group.serverId)}
                              data-testid={`featureset-server-group-${group.serverId}`}
                            >
                              <div className="flex min-w-0 flex-1 items-center gap-3">
                                {isExpanded ? (
                                  <ChevronDown className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
                                ) : (
                                  <ChevronRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
                                )}
                                <Server className="h-4 w-4 flex-shrink-0 text-blue-500" />
                                <div className="min-w-0 flex-1">
                                  <div className="mb-1 flex items-center gap-2">
                                    <span className="truncate text-sm font-medium">
                                      {group.serverId}
                                    </span>
                                    {/* Show count badge only for configurable feature sets */}
                                    {isConfigurable && (
                                      <span
                                        className={`flex-shrink-0 rounded-full px-2 py-0.5 text-xs font-bold ${
                                          selectedCount === 0
                                            ? 'bg-gray-100 text-gray-600 dark:bg-gray-900/30 dark:text-gray-400'
                                            : allSelected
                                              ? 'border border-green-300 bg-green-100 text-green-700 dark:border-green-700 dark:bg-green-900/30 dark:text-green-300'
                                              : 'border border-amber-300 bg-amber-100 text-amber-700 dark:border-amber-700 dark:bg-amber-900/30 dark:text-amber-300'
                                        }`}
                                      >
                                        {selectedCount}/{group.features.length}
                                      </span>
                                    )}
                                  </div>
                                  {/* Progress Bar for Server */}
                                  <div className="h-1 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
                                    <div
                                      className={`h-full transition-all duration-300 ${
                                        selectedCount === 0
                                          ? 'bg-gray-400 dark:bg-gray-600'
                                          : allSelected
                                            ? 'bg-green-500'
                                            : 'bg-gradient-to-r from-amber-500 to-green-500'
                                      }`}
                                      style={{
                                        width: `${(selectedCount / group.features.length) * 100}%`,
                                      }}
                                    />
                                  </div>
                                </div>
                              </div>

                              {isConfigurable && (
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    toggleAllInServer(group.serverId);
                                  }}
                                  className={`flex-shrink-0 rounded-md p-1.5 transition-colors hover:bg-[rgb(var(--background))]`}
                                  title={allSelected ? 'Disable All' : 'Enable All'}
                                >
                                  {allSelected ? (
                                    <ToggleRight className="text-primary-500 h-5 w-5" />
                                  ) : someSelected ? (
                                    <ToggleLeft className="h-5 w-5 text-amber-500" />
                                  ) : (
                                    <ToggleLeft className="h-5 w-5 text-[rgb(var(--muted))]" />
                                  )}
                                </button>
                              )}
                            </div>

                            {isExpanded && (
                              <div className="border-t border-[rgb(var(--border))] bg-[rgb(var(--background))]">
                                {group.features.map((feature) => {
                                  const isSelected = isFeatureSelected(feature.id, feature);

                                  return (
                                    <button
                                      key={feature.id}
                                      onClick={() => toggleFeature(feature.id)}
                                      disabled={!isConfigurable}
                                      className={`flex w-full items-center gap-3 border-b border-[rgb(var(--border))] px-4 py-2.5 pl-12 text-left transition-colors last:border-b-0 ${isConfigurable ? 'hover:bg-[rgb(var(--surface-hover))]' : 'cursor-default'} ${isSelected ? 'bg-primary-50 dark:bg-primary-900/10' : ''}`}
                                    >
                                      <div
                                        className={`flex h-4 w-4 flex-shrink-0 items-center justify-center rounded border transition-colors ${
                                          isSelected
                                            ? 'bg-primary-500 border-primary-500'
                                            : 'border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--surface))]'
                                        }`}
                                      >
                                        {isSelected && <Check className="h-3 w-3 text-white" />}
                                      </div>

                                      {getFeatureIcon(feature.feature_type)}

                                      <div className="min-w-0 flex-1">
                                        <div className="flex items-center gap-2">
                                          <span className="truncate text-sm font-medium">
                                            {feature.display_name || feature.feature_name}
                                          </span>
                                          <span
                                            className={`rounded px-1.5 py-0.5 text-[10px] ${getTypeColor(feature.feature_type)}`}
                                          >
                                            {feature.feature_type}
                                          </span>
                                        </div>
                                        {feature.description && (
                                          <p className="mt-0.5 line-clamp-1 text-xs text-[rgb(var(--muted))]">
                                            {feature.description}
                                          </p>
                                        )}
                                      </div>
                                    </button>
                                  );
                                })}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Footer Actions */}
      <div className="flex flex-shrink-0 items-center gap-3 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        {isCustom && onDelete && (
          <Button
            variant="ghost"
            size="sm"
            onClick={async () => {
              if (
                await confirm({
                  title: 'Delete feature set',
                  message: `Delete "${featureSet.name}"? This cannot be undone.`,
                  confirmLabel: 'Delete',
                  variant: 'danger',
                })
              ) {
                onDelete(featureSet.id);
              }
            }}
            className="mr-auto text-red-500 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20"
          >
            <Trash2 className="mr-2 h-4 w-4" />
            Delete
          </Button>
        )}

        {autoInclude && selectionEdited && (
          <span
            className="text-xs text-[rgb(var(--muted))]"
            data-testid="featureset-save-leaves-auto"
          >
            Saving switches this set to your own selection.
          </span>
        )}
        {isConfigurable && (
          <Button
            onClick={handleSave}
            disabled={isSaving || (autoInclude && !selectionEdited)}
            className="w-full flex-1"
            data-testid="featureset-save"
          >
            {isSaving ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" /> Saving...
              </>
            ) : (
              <>
                <Save className="mr-2 h-4 w-4" /> Save Changes
              </>
            )}
          </Button>
        )}
      </div>
    </div>
  );
}
