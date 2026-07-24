/**
 * CloneAccountModal — wizard for adding another account of an installed MCP server.
 */

import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Copy, Loader2, X } from 'lucide-react';
import type { ServerViewModel } from '@/types/registry';
import {
  CLONE_SUFFIX_SUGGESTIONS,
  cloneServer,
  deriveCloneAlias,
  deriveCloneServerId,
  isCloneIdAvailable,
  suggestCloneSuffix,
  type ClonedInstalledServer,
} from '@/lib/api/serverClone';

export interface CloneAccountModalProps {
  open: boolean;
  spaceId: string;
  sourceServer: ServerViewModel;
  onClose: () => void;
  /** Called after a successful clone with the new install row. */
  onCloned: (cloned: ClonedInstalledServer) => void;
}

/**
 * Modal for creating a suffixed clone of an installed server in the same space.
 */
export function CloneAccountModal({
  open,
  spaceId,
  sourceServer,
  onClose,
  onCloned,
}: CloneAccountModalProps) {
  const { t } = useTranslation(['servers', 'common']);
  const [suffix, setSuffix] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [isChecking, setIsChecking] = useState(false);
  const [isAvailable, setIsAvailable] = useState<boolean | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [isLoadingSuggestion, setIsLoadingSuggestion] = useState(false);

  const trimmedSuffix = suffix.trim();
  const trimmedDisplayName = displayName.trim();
  const displayNamePlaceholder = trimmedSuffix
    ? t('cloneModal.displayNamePlaceholder', { name: sourceServer.name, suffix: trimmedSuffix })
    : t('cloneModal.displayNamePlaceholderDefault', { name: sourceServer.name });

  const previewId = deriveCloneServerId(sourceServer.id, suffix);
  const previewAlias = deriveCloneAlias(suffix);
  const hasSuffix = suffix.trim().length > 0;
  const hasCollision = hasSuffix && isAvailable === false;
  const sourceHeaderKeys =
    sourceServer.transport.type === 'http'
      ? Object.keys(sourceServer.extra_headers ?? {}).filter((key) => key.trim())
      : [];

  useEffect(() => {
    if (!open) {
      return;
    }

    let cancelled = false;

    const loadSuggestion = async () => {
      setIsLoadingSuggestion(true);
      setSubmitError(null);
      try {
        const suggested = await suggestCloneSuffix(spaceId, sourceServer.id);
        if (!cancelled) {
          setSuffix(suggested);
        }
      } catch (e) {
        if (!cancelled) {
          setSuffix(CLONE_SUFFIX_SUGGESTIONS[0]);
          setSubmitError(String(e));
        }
      } finally {
        if (!cancelled) {
          setIsLoadingSuggestion(false);
        }
      }
    };

    loadSuggestion();

    return () => {
      cancelled = true;
    };
  }, [open, spaceId, sourceServer.id]);

  useEffect(() => {
    if (!open || !hasSuffix) {
      setIsAvailable(null);
      setIsChecking(false);
      return;
    }

    let cancelled = false;
    setIsChecking(true);

    const timer = setTimeout(async () => {
      try {
        const available = await isCloneIdAvailable(spaceId, sourceServer.id, suffix);
        if (!cancelled) {
          setIsAvailable(available);
        }
      } catch {
        if (!cancelled) {
          setIsAvailable(null);
        }
      } finally {
        if (!cancelled) {
          setIsChecking(false);
        }
      }
    }, 300);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, spaceId, sourceServer.id, suffix, hasSuffix]);

  /**
   * Submit the clone request.
   */
  const handleSubmit = useCallback(async () => {
    if (!hasSuffix || hasCollision || isChecking) {
      return;
    }

    setIsSubmitting(true);
    setSubmitError(null);

    try {
      const cloned = await cloneServer(
        spaceId,
        sourceServer.id,
        suffix,
        undefined,
        trimmedDisplayName.length > 0 ? trimmedDisplayName : undefined
      );
      onCloned(cloned);
      onClose();
    } catch (e) {
      setSubmitError(String(e));
    } finally {
      setIsSubmitting(false);
    }
  }, [
    hasSuffix,
    hasCollision,
    isChecking,
    spaceId,
    sourceServer.id,
    suffix,
    trimmedDisplayName,
    onCloned,
    onClose,
  ]);

  if (!open) {
    return null;
  }

  const canSubmit =
    hasSuffix && !hasCollision && !isChecking && !isSubmitting && !isLoadingSuggestion;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      data-testid="clone-account-modal-overlay"
    >
      <div
        className="dropdown-menu animate-in fade-in scale-in w-full max-w-md p-6 duration-150"
        data-testid="clone-account-modal"
      >
        <div className="mb-4 flex items-start justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="rounded-lg bg-[rgb(var(--primary))]/10 p-2">
              <Copy className="h-5 w-5 text-[rgb(var(--primary))]" />
            </div>
            <div>
              <h3 className="text-lg font-semibold text-[rgb(var(--foreground))]">
                {t('cloneModal.title')}
              </h3>
              <p className="text-sm text-[rgb(var(--muted))]">
                {t('cloneModal.subtitle', { name: sourceServer.name })}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
            aria-label={t('cloneModal.closeAria')}
            data-testid="clone-account-close-btn"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="space-y-4">
          <div>
            <label
              htmlFor="clone-display-name"
              className="mb-1 block text-sm font-medium text-[rgb(var(--foreground))]"
            >
              {t('cloneModal.displayName')}
            </label>
            <p className="mb-2 text-xs text-[rgb(var(--muted))]">{t('cloneModal.displayNameDesc')}</p>
            <input
              id="clone-display-name"
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder={displayNamePlaceholder}
              className="input w-full"
              disabled={isSubmitting}
              data-testid="clone-display-name-input"
            />
          </div>

          <div>
            <label
              htmlFor="clone-suffix"
              className="mb-1 block text-sm font-medium text-[rgb(var(--foreground))]"
            >
              {t('cloneModal.accountLabel')}
            </label>
            <p className="mb-2 text-xs text-[rgb(var(--muted))]">{t('cloneModal.accountLabelDesc')}</p>
            <input
              id="clone-suffix"
              type="text"
              value={suffix}
              onChange={(e) => setSuffix(e.target.value)}
              placeholder={t('cloneModal.accountLabelPlaceholder')}
              className={`input w-full ${hasCollision ? 'border-[rgb(var(--error))]' : ''}`}
              disabled={isLoadingSuggestion || isSubmitting}
              data-testid="clone-suffix-input"
            />
            {hasCollision && (
              <p
                className="mt-1 text-xs text-[rgb(var(--error))]"
                data-testid="clone-collision-error"
              >
                {t('cloneModal.collisionError')}
              </p>
            )}
          </div>

          <div>
            <p className="mb-2 text-xs font-medium text-[rgb(var(--muted))]">{t('cloneModal.suggestions')}</p>
            <div className="flex flex-wrap gap-2">
              {CLONE_SUFFIX_SUGGESTIONS.map((suggestion) => (
                <button
                  key={suggestion}
                  type="button"
                  onClick={() => setSuffix(suggestion)}
                  className={`rounded-md border px-2.5 py-1 text-xs transition-colors ${
                    suffix === suggestion
                      ? 'border-[rgb(var(--primary))] bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))]'
                      : 'border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                  data-testid={`clone-suffix-suggestion-${suggestion}`}
                >
                  {suggestion}
                </button>
              ))}
            </div>
          </div>

          {hasSuffix && (
            <div className="space-y-2 rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-dim))] p-3">
              <div className="flex items-center justify-between gap-2 text-sm">
                <span className="text-[rgb(var(--muted))]">{t('cloneModal.serverId')}</span>
                <code className="font-mono text-xs text-[rgb(var(--foreground))]">
                  {previewId || '—'}
                </code>
              </div>
              <div className="flex items-center justify-between gap-2 text-sm">
                <span className="text-[rgb(var(--muted))]">{t('cloneModal.toolPrefix')}</span>
                <code className="font-mono text-xs text-[rgb(var(--foreground))]">
                  {previewAlias ? `${previewAlias}_*` : '—'}
                </code>
              </div>
              {isChecking && (
                <div className="flex items-center gap-2 text-xs text-[rgb(var(--muted))]">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  {t('cloneModal.checkingAvailability')}
                </div>
              )}
            </div>
          )}

          {sourceServer.transport.type === 'http' && (
            <div className="rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-dim))] p-3">
              <p className="text-sm font-medium text-[rgb(var(--foreground))]">
                {t('cloneModal.headersPreviewTitle')}
              </p>
              <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                {t('cloneModal.headersPreviewDesc')}
              </p>
              {sourceHeaderKeys.length > 0 ? (
                <ul
                  className="mt-2 space-y-1 font-mono text-xs text-[rgb(var(--foreground))]"
                  data-testid="clone-headers-preview"
                >
                  {sourceHeaderKeys.map((headerKey) => (
                    <li key={headerKey}>{headerKey}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-2 text-xs text-[rgb(var(--muted))]">
                  {t('cloneModal.headersPreviewEmpty')}
                </p>
              )}
            </div>
          )}

          <p className="text-xs text-[rgb(var(--muted))]">{t('cloneModal.footerNote')}</p>

          {submitError && (
            <p className="text-sm text-[rgb(var(--error))]" data-testid="clone-submit-error">
              {submitError}
            </p>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <button
              onClick={onClose}
              className="rounded-lg border border-[rgb(var(--border))] px-4 py-2 text-sm text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
              disabled={isSubmitting}
              data-testid="clone-cancel-btn"
            >
              {t('common:actions.cancel')}
            </button>
            <button
              onClick={handleSubmit}
              disabled={!canSubmit}
              className="flex items-center gap-2 rounded-lg bg-[rgb(var(--primary))] px-4 py-2 text-sm text-[rgb(var(--primary-foreground))] transition-colors hover:bg-[rgb(var(--primary-hover))] disabled:opacity-50"
              data-testid="clone-submit-btn"
            >
              {isSubmitting && <Loader2 className="h-4 w-4 animate-spin" />}
              {isSubmitting ? t('cloneModal.creating') : t('cloneModal.createAccount')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
