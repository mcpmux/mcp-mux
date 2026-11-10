import { useState, useEffect, useCallback } from 'react';
import { Card, CardHeader, CardTitle, CardDescription, CardContent, Button } from '@mcpmux/ui';
import {
  Key,
  Plus,
  Trash2,
  Edit2,
  Eye,
  EyeOff,
  Check,
  X,
  Lock,
  Layers,
  AlertCircle,
} from 'lucide-react';
import {
  listCredentialProfiles,
  getCredentialProfile,
  createCredentialProfile,
  updateCredentialProfile,
  deleteCredentialProfile,
  type CredentialProfileSummary,
  type CredentialProfile,
} from '@/lib/api/credentials';

interface Props {
  onNotify?: (msg: string, type: 'success' | 'error') => void;
}

export function CredentialProfilesSection({ onNotify }: Props) {
  const [profiles, setProfiles] = useState<CredentialProfileSummary[]>([]);
  const [loading, setLoading] = useState(true);

  // Modal state
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [editingProfile, setEditingProfile] = useState<CredentialProfile | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  // Form state
  const [formName, setFormName] = useState('');
  const [formServerId, setFormServerId] = useState('');
  const [formServerName, setFormServerName] = useState('');
  const [formKeyValues, setFormKeyValues] = useState<
    Array<{ key: string; value: string; show?: boolean }>
  >([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const fetchProfiles = useCallback(async () => {
    try {
      setLoading(true);
      const list = await listCredentialProfiles();
      setProfiles(list);
    } catch (err) {
      console.error('[CredentialProfiles] Failed to list:', err);
      onNotify?.(String(err), 'error');
    } finally {
      setLoading(false);
    }
  }, [onNotify]);

  useEffect(() => {
    fetchProfiles();
  }, [fetchProfiles]);

  const openCreateModal = () => {
    setFormName('');
    setFormServerId('');
    setFormServerName('');
    setFormKeyValues([{ key: '', value: '', show: false }]);
    setFormError(null);
    setIsCreateOpen(true);
  };

  const openEditModal = async (summary: CredentialProfileSummary) => {
    try {
      setFormError(null);
      const full = await getCredentialProfile(summary.id);
      setEditingProfile(full);
      setFormName(full.name);
      setFormServerId(full.server_id);
      setFormServerName(full.server_name || '');
      const kvs = Object.entries(full.values).map(([k, v]) => ({ key: k, value: v, show: false }));
      setFormKeyValues(kvs.length > 0 ? kvs : [{ key: '', value: '', show: false }]);
    } catch (err) {
      console.error('[CredentialProfiles] Failed to load profile:', err);
      onNotify?.(String(err), 'error');
    }
  };

  const handleSaveCreate = async () => {
    if (!formName.trim()) {
      setFormError('Please enter a profile name.');
      return;
    }
    if (!formServerId.trim()) {
      setFormError('Please enter a server definition ID.');
      return;
    }

    const values: Record<string, string> = {};
    for (const item of formKeyValues) {
      if (item.key.trim()) {
        values[item.key.trim()] = item.value;
      }
    }

    try {
      setIsSubmitting(true);
      await createCredentialProfile(
        formName.trim(),
        formServerId.trim(),
        formServerName.trim() || null,
        values
      );
      setIsCreateOpen(false);
      onNotify?.(`Created credential profile "${formName.trim()}"`, 'success');
      await fetchProfiles();
    } catch (err) {
      setFormError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleSaveEdit = async () => {
    if (!editingProfile) return;
    if (!formName.trim()) {
      setFormError('Please enter a profile name.');
      return;
    }

    const values: Record<string, string> = {};
    for (const item of formKeyValues) {
      if (item.key.trim()) {
        values[item.key.trim()] = item.value;
      }
    }

    try {
      setIsSubmitting(true);
      await updateCredentialProfile(editingProfile.id, formName.trim(), values);
      setEditingProfile(null);
      onNotify?.(
        `Updated credential profile "${formName.trim()}". Secrets propagated to linked Spaces.`,
        'success'
      );
      await fetchProfiles();
    } catch (err) {
      setFormError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async (id: string, name: string) => {
    try {
      await deleteCredentialProfile(id);
      setDeletingId(null);
      onNotify?.(`Deleted credential profile "${name}"`, 'success');
      await fetchProfiles();
    } catch (err) {
      onNotify?.(String(err), 'error');
    }
  };

  return (
    <Card data-testid="settings-credentials-section">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Key className="h-5 w-5 text-[rgb(var(--primary))]" />
            Credential Profiles &amp; Secrets
          </CardTitle>
          <Button
            size="sm"
            onClick={openCreateModal}
            className="flex items-center gap-1.5"
            data-testid="add-credential-profile-btn"
          >
            <Plus className="h-4 w-4" />
            Add Profile
          </Button>
        </div>
        <CardDescription>
          Centrally managed secrets and API keys reused across your Spaces. Credential profiles
          survive server uninstalls and propagate rotated keys in a single place.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="py-6 text-center text-sm text-[rgb(var(--muted))]">
            Loading credential profiles...
          </div>
        ) : profiles.length === 0 ? (
          <div className="rounded-lg border border-dashed border-[rgb(var(--border))] py-8 text-center">
            <Key className="mx-auto mb-2 h-8 w-8 text-[rgb(var(--muted))]" />
            <p className="text-sm font-medium text-[rgb(var(--foreground))]">
              No credential profiles saved yet
            </p>
            <p className="mx-auto mt-1 max-w-sm text-xs text-[rgb(var(--muted))]">
              When installing or configuring MCP servers in any Space, check &quot;Save as
              Credential Profile&quot;, or create one here to reuse across Spaces.
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {profiles.map((profile) => (
              <div
                key={profile.id}
                className="flex items-center justify-between rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-3.5 transition-colors hover:border-[rgb(var(--border-hover))]"
                data-testid={`credential-profile-row-${profile.id}`}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-[rgb(var(--foreground))]">
                      {profile.name}
                    </span>
                    <span className="rounded bg-[rgb(var(--muted))]/15 px-2 py-0.5 font-mono text-xs text-[rgb(var(--muted))]">
                      {profile.server_name || profile.server_id}
                    </span>
                    {profile.usage_count > 0 ? (
                      <span className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">
                        <Layers className="h-3 w-3" />
                        Used in {profile.usage_count}{' '}
                        {profile.usage_count === 1 ? 'server' : 'servers'}
                      </span>
                    ) : (
                      <span className="rounded bg-[rgb(var(--muted))]/10 px-2 py-0.5 text-xs text-[rgb(var(--muted))]">
                        Unlinked
                      </span>
                    )}
                  </div>
                  <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-[rgb(var(--muted))]">
                    <Lock className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
                    <span>Keys:</span>
                    {profile.input_keys.length > 0 ? (
                      profile.input_keys.map((k) => (
                        <span
                          key={k}
                          className="rounded bg-[rgb(var(--muted))]/20 px-1.5 py-0.5 font-mono text-[11px] text-[rgb(var(--foreground))]"
                        >
                          {k}
                        </span>
                      ))
                    ) : (
                      <span className="italic">No keys configured</span>
                    )}
                  </div>
                </div>

                <div className="ml-4 flex shrink-0 items-center gap-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => openEditModal(profile)}
                    title="Edit credential profile"
                    data-testid={`edit-profile-${profile.id}`}
                  >
                    <Edit2 className="h-4 w-4" />
                  </Button>
                  {deletingId === profile.id ? (
                    <div className="flex items-center gap-1">
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={() => handleDelete(profile.id, profile.name)}
                        data-testid={`confirm-delete-profile-${profile.id}`}
                      >
                        <Check className="h-3.5 w-3.5" />
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => setDeletingId(null)}>
                        <X className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  ) : (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setDeletingId(profile.id)}
                      className="text-[rgb(var(--error))] hover:bg-[rgb(var(--error))]/10"
                      title="Delete credential profile"
                      data-testid={`delete-profile-${profile.id}`}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>

      {/* Modal for Create or Edit */}
      {(isCreateOpen || editingProfile) && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
          data-testid="credential-profile-modal-overlay"
        >
          <div
            className="dropdown-menu animate-in fade-in scale-in flex max-h-[85vh] w-full max-w-lg flex-col overflow-hidden duration-150"
            data-testid="credential-profile-modal"
          >
            <div className="px-6 pb-3 pt-6">
              <h3 className="text-lg font-semibold text-[rgb(var(--foreground))]">
                {isCreateOpen ? 'Create Credential Profile' : `Edit "${editingProfile?.name}"`}
              </h3>
              <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                {isCreateOpen
                  ? 'Add a reusable credential profile stored in your OS keychain / vault.'
                  : 'Updating values here automatically rotates secrets in all Spaces using this profile.'}
              </p>
            </div>

            <div className="flex-1 space-y-4 overflow-y-auto px-6 py-3">
              {formError && (
                <div className="flex items-center gap-2 rounded-lg bg-[rgb(var(--error))]/10 p-3 text-xs text-[rgb(var(--error))]">
                  <AlertCircle className="h-4 w-4 shrink-0" />
                  <span>{formError}</span>
                </div>
              )}

              <div>
                <label className="mb-1 block text-sm font-medium text-[rgb(var(--foreground))]">
                  Profile Name <span className="text-[rgb(var(--error))]">*</span>
                </label>
                <input
                  type="text"
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder="e.g. Work GitHub, Personal Cloudflare"
                  className="input w-full"
                  data-testid="profile-form-name"
                />
              </div>

              {isCreateOpen && (
                <div>
                  <label className="mb-1 block text-sm font-medium text-[rgb(var(--foreground))]">
                    MCP Server ID <span className="text-[rgb(var(--error))]">*</span>
                  </label>
                  <input
                    type="text"
                    value={formServerId}
                    onChange={(e) => setFormServerId(e.target.value)}
                    placeholder="e.g. com.github.mcp, cloudflare"
                    className="input w-full font-mono text-sm"
                    data-testid="profile-form-server-id"
                  />
                  <p className="mt-1 text-[11px] text-[rgb(var(--muted))]">
                    The identifier of the MCP server this credential belongs to.
                  </p>
                </div>
              )}

              <div>
                <div className="mb-2 flex items-center justify-between">
                  <label className="text-sm font-medium text-[rgb(var(--foreground))]">
                    Secrets &amp; Configuration Values
                  </label>
                  <button
                    type="button"
                    onClick={() =>
                      setFormKeyValues([...formKeyValues, { key: '', value: '', show: false }])
                    }
                    className="text-xs text-[rgb(var(--primary))] hover:underline"
                    data-testid="profile-form-add-key"
                  >
                    + Add field
                  </button>
                </div>

                <div className="space-y-2">
                  {formKeyValues.map((row, idx) => (
                    <div key={idx} className="flex items-center gap-2">
                      <input
                        type="text"
                        value={row.key}
                        onChange={(e) => {
                          const updated = [...formKeyValues];
                          updated[idx].key = e.target.value;
                          setFormKeyValues(updated);
                        }}
                        placeholder="KEY (e.g. API_KEY)"
                        className="input w-1/3 font-mono text-xs"
                      />
                      <div className="relative flex-1">
                        <input
                          type={row.show ? 'text' : 'password'}
                          value={row.value}
                          onChange={(e) => {
                            const updated = [...formKeyValues];
                            updated[idx].value = e.target.value;
                            setFormKeyValues(updated);
                          }}
                          placeholder="Secret value"
                          className="input w-full pr-8 font-mono text-xs"
                        />
                        <button
                          type="button"
                          onClick={() => {
                            const updated = [...formKeyValues];
                            updated[idx].show = !updated[idx].show;
                            setFormKeyValues(updated);
                          }}
                          className="absolute right-2 top-2 text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]"
                        >
                          {row.show ? (
                            <EyeOff className="h-3.5 w-3.5" />
                          ) : (
                            <Eye className="h-3.5 w-3.5" />
                          )}
                        </button>
                      </div>
                      <button
                        type="button"
                        onClick={() => {
                          const updated = formKeyValues.filter((_, i) => i !== idx);
                          setFormKeyValues(
                            updated.length > 0 ? updated : [{ key: '', value: '', show: false }]
                          );
                        }}
                        className="px-1.5 py-1 text-sm text-[rgb(var(--muted))] transition-colors hover:text-[rgb(var(--error))]"
                        title="Remove key"
                      >
                        ✕
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            <div className="flex justify-end gap-2 border-t border-[rgb(var(--border))] px-6 py-4">
              <Button
                variant="ghost"
                onClick={() => {
                  setIsCreateOpen(false);
                  setEditingProfile(null);
                }}
                disabled={isSubmitting}
              >
                Cancel
              </Button>
              <Button
                onClick={isCreateOpen ? handleSaveCreate : handleSaveEdit}
                disabled={isSubmitting}
                data-testid="profile-form-save-btn"
              >
                {isSubmitting ? 'Saving...' : isCreateOpen ? 'Create Profile' : 'Save Changes'}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}
