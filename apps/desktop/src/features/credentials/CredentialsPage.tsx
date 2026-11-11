import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  KeyRound,
  Plus,
  Trash2,
  Edit2,
  Eye,
  EyeOff,
  Copy,
  Check,
  Search,
  Lock,
  Layers,
  ShieldCheck,
  Loader2,
  RefreshCw,
} from 'lucide-react';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Button,
  useToast,
  ToastContainer,
  useConfirm,
} from '@mcpmux/ui';
import {
  listCredentialProfiles,
  getCredentialProfile,
  updateCredentialProfile,
  deleteCredentialProfile,
  type CredentialProfileSummary,
  type CredentialProfile,
} from '@/lib/api/credentials';
import { CreateProfileModal } from './CreateProfileModal';
import { EditProfileModal } from './EditProfileModal';

export function CredentialsPage() {
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();

  const [profiles, setProfiles] = useState<CredentialProfileSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');

  // Selected profile state
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [activeProfile, setActiveProfile] = useState<CredentialProfile | null>(null);
  const [loadingActiveProfile, setLoadingActiveProfile] = useState(false);

  // Active profile edited keys & values
  const [activeKeyValues, setActiveKeyValues] = useState<
    Array<{ key: string; value: string; show?: boolean }>
  >([]);
  const [newKeyName, setNewKeyName] = useState('');
  const [newKeyValue, setNewKeyValue] = useState('');
  const [newKeyShow, setNewKeyShow] = useState(false);
  const [isSavingKeys, setIsSavingKeys] = useState(false);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // Modals
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [editingProfileTarget, setEditingProfileTarget] = useState<CredentialProfile | null>(null);

  const fetchProfiles = useCallback(
    async (selectIdAfter?: string) => {
      try {
        setLoading(true);
        const list = await listCredentialProfiles();
        setProfiles(list);

        if (selectIdAfter) {
          setSelectedProfileId(selectIdAfter);
        } else if (
          list.length > 0 &&
          (!selectedProfileId || !list.some((p) => p.id === selectedProfileId))
        ) {
          setSelectedProfileId(list[0].id);
        } else if (list.length === 0) {
          setSelectedProfileId(null);
          setActiveProfile(null);
        }
      } catch (err) {
        showError('Failed to load profiles', String(err));
      } finally {
        setLoading(false);
      }
    },
    [selectedProfileId, showError]
  );

  useEffect(() => {
    fetchProfiles();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Load active profile details whenever selectedProfileId changes
  useEffect(() => {
    if (!selectedProfileId) {
      setActiveProfile(null);
      setActiveKeyValues([]);
      return;
    }

    let isMounted = true;
    setLoadingActiveProfile(true);

    getCredentialProfile(selectedProfileId)
      .then((full) => {
        if (!isMounted) return;
        setActiveProfile(full);
        const kvs = Object.entries(full.values).map(([k, v]) => ({
          key: k,
          value: v,
          show: false,
        }));
        setActiveKeyValues(kvs);
      })
      .catch((err) => {
        if (!isMounted) return;
        showError('Failed to load profile details', String(err));
      })
      .finally(() => {
        if (isMounted) setLoadingActiveProfile(false);
      });

    return () => {
      isMounted = false;
    };
  }, [selectedProfileId, showError]);

  // Filter profiles based on search query
  const filteredProfiles = useMemo(() => {
    if (!searchQuery.trim()) return profiles;
    const q = searchQuery.toLowerCase();
    return profiles.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.server_id.toLowerCase().includes(q) ||
        (p.server_name && p.server_name.toLowerCase().includes(q)) ||
        p.input_keys.some((k) => k.toLowerCase().includes(q))
    );
  }, [profiles, searchQuery]);

  const handleCopyValue = (key: string, value: string) => {
    navigator.clipboard.writeText(value);
    setCopiedKey(key);
    success('Copied to clipboard', `Secret value for "${key}" copied.`);
    setTimeout(() => {
      setCopiedKey((curr) => (curr === key ? null : curr));
    }, 2000);
  };

  const handleAddKeyRow = () => {
    if (!newKeyName.trim()) return;
    const key = newKeyName.trim();
    if (activeKeyValues.some((item) => item.key === key)) {
      showError('Duplicate Key', `The key "${key}" is already present in this profile.`);
      return;
    }
    setActiveKeyValues((prev) => [...prev, { key, value: newKeyValue, show: newKeyShow }]);
    setNewKeyName('');
    setNewKeyValue('');
    setNewKeyShow(false);
  };

  const handleRemoveKey = (keyToRemove: string) => {
    setActiveKeyValues((prev) => prev.filter((item) => item.key !== keyToRemove));
  };

  const handleSaveActiveKeys = async () => {
    if (!activeProfile) return;
    const values: Record<string, string> = {};
    for (const item of activeKeyValues) {
      if (item.key.trim()) {
        values[item.key.trim()] = item.value;
      }
    }

    try {
      setIsSavingKeys(true);
      const updated = await updateCredentialProfile(activeProfile.id, activeProfile.name, values);
      setActiveProfile(updated);
      success(
        'Secrets Updated',
        `Credential profile "${updated.name}" updated. Secrets propagated to linked servers.`
      );
      await fetchProfiles(updated.id);
    } catch (err) {
      showError('Failed to save secrets', String(err));
    } finally {
      setIsSavingKeys(false);
    }
  };

  const handleDeleteProfile = async () => {
    if (!activeProfile) return;
    const confirmed = await confirm({
      title: `Delete profile "${activeProfile.name}"?`,
      message: `Are you sure you want to delete this credential profile? Servers configured with this profile will no longer have access to these secrets.`,
      confirmLabel: 'Delete Profile',
      variant: 'danger',
    });
    if (!confirmed) return;

    try {
      await deleteCredentialProfile(activeProfile.id);
      success('Profile Deleted', `Profile "${activeProfile.name}" has been deleted.`);
      await fetchProfiles();
    } catch (err) {
      showError('Failed to delete profile', String(err));
    }
  };

  // Determine if active profile's keys have unsaved changes
  const hasUnsavedChanges = useMemo(() => {
    if (!activeProfile) return false;
    const originalKeys = Object.keys(activeProfile.values);
    if (originalKeys.length !== activeKeyValues.length) return true;
    for (const item of activeKeyValues) {
      if (activeProfile.values[item.key] !== item.value) return true;
    }
    return false;
  }, [activeProfile, activeKeyValues]);

  return (
    <>
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}

      <div className="flex h-full flex-col overflow-hidden" data-testid="credentials-page">
        {/* Top Header */}
        <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-6 sm:p-8">
          <div className="mx-auto flex max-w-[2000px] flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h1 className="flex items-center gap-3 text-2xl font-bold sm:text-3xl">
                <KeyRound className="h-7 w-7 text-[rgb(var(--primary))]" />
                Secrets &amp; Credentials
              </h1>
              <p className="mt-1 text-sm text-[rgb(var(--muted))]">
                Centrally managed secrets and API keys reused across your Spaces. Credential
                profiles survive server uninstalls and propagate rotated keys in a single place.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => fetchProfiles(selectedProfileId ?? undefined)}
                disabled={loading}
                title="Refresh profiles"
              >
                <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={() => setIsCreateOpen(true)}
                className="flex items-center gap-1.5"
                data-testid="create-credential-profile-btn"
              >
                <Plus className="h-4 w-4" />
                New Profile
              </Button>
            </div>
          </div>
        </header>

        {/* Master-Detail Layout */}
        <div className="flex flex-1 overflow-hidden">
          {/* Left Sub-Sidebar: Profiles List */}
          <aside className="flex w-80 flex-shrink-0 flex-col border-r border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-subtle))]">
            {/* Search filter */}
            <div className="border-b border-[rgb(var(--border-subtle))] p-3">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[rgb(var(--muted))]" />
                <input
                  type="text"
                  placeholder="Search profiles..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] py-1.5 pl-8 pr-3 text-xs text-[rgb(var(--foreground))] placeholder:text-[rgb(var(--muted))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                  data-testid="credentials-search-input"
                />
              </div>
            </div>

            {/* Profiles items */}
            <div className="flex-1 space-y-2 overflow-y-auto p-3">
              {loading && profiles.length === 0 ? (
                <div className="py-8 text-center text-xs text-[rgb(var(--muted))]">
                  <Loader2 className="mx-auto mb-2 h-5 w-5 animate-spin" />
                  Loading profiles...
                </div>
              ) : filteredProfiles.length === 0 ? (
                <div className="px-2 py-8 text-center text-xs text-[rgb(var(--muted))]">
                  {searchQuery
                    ? 'No profiles match your search.'
                    : 'No credential profiles saved yet.'}
                </div>
              ) : (
                filteredProfiles.map((p) => {
                  const isSelected = p.id === selectedProfileId;
                  return (
                    <button
                      key={p.id}
                      onClick={() => setSelectedProfileId(p.id)}
                      className={`w-full rounded-xl border p-3 text-left transition-all ${
                        isSelected
                          ? 'border-[rgb(var(--primary))] bg-[rgb(var(--primary))]/10 shadow-sm'
                          : 'border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:border-[rgb(var(--border-hover))] hover:bg-[rgb(var(--surface-hover))]'
                      }`}
                      data-testid={`profile-sidebar-item-${p.id}`}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate text-sm font-semibold text-[rgb(var(--foreground))]">
                          {p.name}
                        </span>
                        {p.usage_count > 0 ? (
                          <span className="inline-flex flex-shrink-0 items-center gap-1 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-600 dark:text-emerald-400">
                            <Layers className="h-2.5 w-2.5" />
                            {p.usage_count}
                          </span>
                        ) : null}
                      </div>

                      <div className="mt-1 flex items-center justify-between text-xs text-[rgb(var(--muted))]">
                        <span className="truncate font-mono text-[11px]">
                          {p.server_name || p.server_id}
                        </span>
                        <span className="flex-shrink-0 text-[10px]">
                          {p.input_keys.length} {p.input_keys.length === 1 ? 'secret' : 'secrets'}
                        </span>
                      </div>
                    </button>
                  );
                })
              )}
            </div>

            {/* Bottom count badge */}
            <div className="flex items-center justify-between border-t border-[rgb(var(--border-subtle))] p-3 text-[11px] text-[rgb(var(--muted))]">
              <span>{profiles.length} total profiles</span>
              <button
                onClick={() => setIsCreateOpen(true)}
                className="flex items-center gap-1 font-medium text-[rgb(var(--primary))] hover:underline"
              >
                <Plus className="h-3 w-3" />
                Add
              </button>
            </div>
          </aside>

          {/* Right Detail View */}
          <main className="flex-1 overflow-y-auto bg-[rgb(var(--background))] p-6 sm:p-8">
            <div className="mx-auto max-w-4xl space-y-6">
              {loadingActiveProfile ? (
                <div className="py-16 text-center text-sm text-[rgb(var(--muted))]">
                  <Loader2 className="mx-auto mb-2 h-6 w-6 animate-spin text-[rgb(var(--primary))]" />
                  Loading profile details...
                </div>
              ) : activeProfile ? (
                <>
                  {/* Profile Header Card */}
                  <Card className="border-[rgb(var(--border))]">
                    <CardHeader className="pb-4">
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                        <div className="flex items-center gap-3">
                          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))]">
                            <KeyRound className="h-5 w-5" />
                          </div>
                          <div>
                            <CardTitle className="text-xl">{activeProfile.name}</CardTitle>
                            <div className="mt-1 flex flex-wrap items-center gap-2">
                              <span className="rounded border border-[rgb(var(--border))] bg-[rgb(var(--surface-subtle))] px-2 py-0.5 font-mono text-xs text-[rgb(var(--foreground))]">
                                {activeProfile.server_name || activeProfile.server_id}
                              </span>
                              {activeProfile.server_name && activeProfile.server_id ? (
                                <span className="font-mono text-xs text-[rgb(var(--muted))]">
                                  ({activeProfile.server_id})
                                </span>
                              ) : null}
                              <span className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">
                                <ShieldCheck className="h-3.5 w-3.5" />
                                Vault Protected
                              </span>
                            </div>
                          </div>
                        </div>

                        <div className="flex items-center gap-2 self-start sm:self-auto">
                          <Button
                            variant="secondary"
                            size="sm"
                            onClick={() => setEditingProfileTarget(activeProfile)}
                            className="flex items-center gap-1.5"
                            data-testid="edit-profile-name-btn"
                          >
                            <Edit2 className="h-3.5 w-3.5" />
                            Rename
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={handleDeleteProfile}
                            className="flex items-center gap-1.5 text-red-600 hover:bg-red-500/10 hover:text-red-700"
                            data-testid="delete-profile-btn"
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                            Delete
                          </Button>
                        </div>
                      </div>
                    </CardHeader>
                  </Card>

                  {/* Secrets & Variables Card */}
                  <Card className="border-[rgb(var(--border))]">
                    <CardHeader>
                      <div className="flex items-center justify-between">
                        <div>
                          <CardTitle className="flex items-center gap-2 text-lg">
                            <Lock className="h-4 w-4 text-emerald-600 dark:text-emerald-400" />
                            Stored Secrets &amp; Environment Variables
                          </CardTitle>
                          <CardDescription className="mt-1">
                            Values are encrypted securely. Click the eye icon to reveal, or copy
                            directly to your clipboard.
                          </CardDescription>
                        </div>
                        {hasUnsavedChanges ? (
                          <Button
                            variant="primary"
                            size="sm"
                            onClick={handleSaveActiveKeys}
                            disabled={isSavingKeys}
                            data-testid="save-profile-keys-btn"
                          >
                            {isSavingKeys ? (
                              <>
                                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                                Saving...
                              </>
                            ) : (
                              'Save Changes'
                            )}
                          </Button>
                        ) : null}
                      </div>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      {activeKeyValues.length === 0 ? (
                        <div className="rounded-xl border border-dashed border-[rgb(var(--border))] py-8 text-center text-xs text-[rgb(var(--muted))]">
                          No secret keys in this profile yet. Add your first key below.
                        </div>
                      ) : (
                        <div className="space-y-2.5">
                          {activeKeyValues.map((item, idx) => (
                            <div
                              key={item.key}
                              className="flex items-center justify-between gap-3 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3"
                              data-testid={`secret-key-row-${item.key}`}
                            >
                              <div className="w-1/3 min-w-0">
                                <span className="block truncate font-mono text-xs font-semibold text-[rgb(var(--foreground))]">
                                  {item.key}
                                </span>
                              </div>

                              <div className="flex flex-1 items-center gap-2">
                                <div className="relative flex-1">
                                  <input
                                    type={item.show ? 'text' : 'password'}
                                    value={item.value}
                                    onChange={(e) => {
                                      const newVal = e.target.value;
                                      setActiveKeyValues((prev) =>
                                        prev.map((k, i) =>
                                          i === idx ? { ...k, value: newVal } : k
                                        )
                                      );
                                    }}
                                    className="w-full rounded-md border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-2.5 py-1.5 pr-8 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-1 focus:ring-[rgb(var(--primary))]"
                                  />
                                  <button
                                    type="button"
                                    onClick={() => {
                                      setActiveKeyValues((prev) =>
                                        prev.map((k, i) =>
                                          i === idx ? { ...k, show: !k.show } : k
                                        )
                                      );
                                    }}
                                    className="absolute right-2 top-1/2 -translate-y-1/2 text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]"
                                    title={item.show ? 'Hide value' : 'Show value'}
                                  >
                                    {item.show ? (
                                      <EyeOff className="h-3.5 w-3.5" />
                                    ) : (
                                      <Eye className="h-3.5 w-3.5" />
                                    )}
                                  </button>
                                </div>

                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => handleCopyValue(item.key, item.value)}
                                  className="h-8 w-8 p-0"
                                  title="Copy secret"
                                >
                                  {copiedKey === item.key ? (
                                    <Check className="h-3.5 w-3.5 text-emerald-500" />
                                  ) : (
                                    <Copy className="h-3.5 w-3.5" />
                                  )}
                                </Button>

                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => handleRemoveKey(item.key)}
                                  className="h-8 w-8 p-0 text-[rgb(var(--muted))] hover:text-red-500"
                                  title="Delete variable"
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </div>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* Add new key row */}
                      <div className="space-y-3 rounded-xl border border-dashed border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-subtle))] p-3.5">
                        <span className="block text-xs font-semibold text-[rgb(var(--foreground))]">
                          Add New Secret Key
                        </span>
                        <div className="flex flex-col items-center gap-2 sm:flex-row">
                          <input
                            type="text"
                            placeholder="KEY_NAME (e.g. API_KEY)"
                            value={newKeyName}
                            onChange={(e) => setNewKeyName(e.target.value)}
                            className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40 sm:w-1/3"
                          />
                          <div className="relative w-full sm:flex-1">
                            <input
                              type={newKeyShow ? 'text' : 'password'}
                              placeholder="Secret value"
                              value={newKeyValue}
                              onChange={(e) => setNewKeyValue(e.target.value)}
                              className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 pr-8 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                            />
                            <button
                              type="button"
                              onClick={() => setNewKeyShow(!newKeyShow)}
                              className="absolute right-2 top-1/2 -translate-y-1/2 text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]"
                            >
                              {newKeyShow ? (
                                <EyeOff className="h-3.5 w-3.5" />
                              ) : (
                                <Eye className="h-3.5 w-3.5" />
                              )}
                            </button>
                          </div>
                          <Button
                            variant="secondary"
                            size="sm"
                            onClick={handleAddKeyRow}
                            disabled={!newKeyName.trim()}
                            className="w-full sm:w-auto"
                          >
                            <Plus className="mr-1.5 h-3.5 w-3.5" />
                            Add Key
                          </Button>
                        </div>
                      </div>
                    </CardContent>
                  </Card>

                  {/* Usage & Propagation Card */}
                  <Card className="border-[rgb(var(--border))]">
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Layers className="h-4 w-4 text-[rgb(var(--primary))]" />
                        Propagation &amp; Linked Spaces
                      </CardTitle>
                      <CardDescription className="text-xs">
                        This credential profile is referenced by{' '}
                        <strong className="text-[rgb(var(--foreground))]">
                          {profiles.find((p) => p.id === activeProfile.id)?.usage_count ?? 0}
                        </strong>{' '}
                        installed MCP server(s). Any updates or key rotations saved here are
                        immediately reflected when these servers run, with no manual reinstall
                        needed.
                      </CardDescription>
                    </CardHeader>
                  </Card>
                </>
              ) : (
                /* Empty state when no profile selected */
                <div className="py-20 text-center">
                  <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))]">
                    <KeyRound className="h-8 w-8" />
                  </div>
                  <h3 className="text-lg font-bold text-[rgb(var(--foreground))]">
                    Manage Credential Profiles
                  </h3>
                  <p className="mx-auto mt-2 max-w-md text-xs text-[rgb(var(--muted))]">
                    Select a profile from the left sidebar to view, copy, or edit its secrets. Or
                    create a new profile to share tokens across servers in multiple Spaces.
                  </p>
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => setIsCreateOpen(true)}
                    className="mt-6 inline-flex items-center gap-2"
                  >
                    <Plus className="h-4 w-4" />
                    Create Profile
                  </Button>
                </div>
              )}
            </div>
          </main>
        </div>
      </div>

      {/* Modals */}
      <CreateProfileModal
        open={isCreateOpen}
        onClose={() => setIsCreateOpen(false)}
        onCreated={(newProfile) => {
          fetchProfiles(newProfile.id);
        }}
      />

      <EditProfileModal
        profile={editingProfileTarget}
        open={Boolean(editingProfileTarget)}
        onClose={() => setEditingProfileTarget(null)}
        onUpdated={(updated) => {
          setActiveProfile(updated);
          fetchProfiles(updated.id);
          success('Profile Renamed', `Profile renamed to "${updated.name}".`);
        }}
      />
    </>
  );
}
