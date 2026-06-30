/**
 * Machine catalog UUID display, copy actions, and paste-to-link UI.
 */

import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy, Loader2 } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  buildMcpMachineHeaderSnippet,
  copyTextToClipboard,
  isMachineUuid,
} from '@/lib/machine-id.helpers';

export interface MachineIdSectionProps {
  machineId: string | null;
  linkMachineIdDraft?: string;
  onLinkMachineIdDraftChange?: (value: string) => void;
  onLink?: (id: string) => Promise<boolean>;
  isLinking?: boolean;
  linkError?: string | null;
  testIdPrefix?: string;
  compact?: boolean;
}

type CopiedKind = 'uuid' | 'header' | null;

const COPY_FEEDBACK_MS = 1500;

/**
 * Render machine ID with copy actions when linked, or paste-to-link when unlinked.
 */
export function MachineIdSection({
  machineId,
  linkMachineIdDraft = '',
  onLinkMachineIdDraftChange,
  onLink,
  isLinking = false,
  linkError = null,
  testIdPrefix = 'machine-id',
  compact = false,
}: MachineIdSectionProps) {
  const { t } = useTranslation('common');
  const [copiedKind, setCopiedKind] = useState<CopiedKind>(null);
  const [localLinkError, setLocalLinkError] = useState<string | null>(null);

  const normalizedId = machineId?.trim() || null;
  const prefix = testIdPrefix;

  /**
   * Copy text and show brief inline feedback on the triggering button.
   */
  const handleCopy = useCallback(async (kind: CopiedKind, text: string) => {
    try {
      await copyTextToClipboard(text);
      setCopiedKind(kind);
      setTimeout(() => setCopiedKind(null), COPY_FEEDBACK_MS);
    } catch {
      /* clipboard may be unavailable */
    }
  }, []);

  /**
   * Validate draft and delegate linking to the parent hook.
   */
  const handleLink = useCallback(async () => {
    const draft = linkMachineIdDraft.trim();
    if (!isMachineUuid(draft)) {
      setLocalLinkError('invalidId');
      return;
    }
    setLocalLinkError(null);
    if (!onLink) {
      return;
    }
    const ok = await onLink(draft);
    if (!ok && !linkError) {
      setLocalLinkError('linkFailed');
    }
  }, [linkError, linkMachineIdDraft, onLink]);

  const resolvedLinkError =
    linkError === 'invalidId'
      ? t('viewerIdentity.invalidId')
      : linkError === 'linkNotFound'
        ? t('viewerIdentity.linkNotFound')
        : linkError === 'linkFailed'
          ? t('viewerIdentity.linkFailed')
          : localLinkError === 'invalidId'
            ? t('viewerIdentity.invalidId')
            : localLinkError === 'linkFailed'
              ? t('viewerIdentity.linkFailed')
              : null;

  return (
    <div
      className={[
        'space-y-2 rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]',
        compact ? 'p-3' : 'p-4',
      ].join(' ')}
      data-testid={`${prefix}-section`}
    >
      <p className="text-xs font-medium text-[rgb(var(--muted))]">{t('viewerIdentity.idLabel')}</p>

      {normalizedId ? (
        <>
          <code
            className="block break-all rounded-md border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-xs text-[rgb(var(--foreground))]"
            data-testid={`${prefix}-value`}
          >
            {normalizedId}
          </code>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              size="sm"
              disabled={isLinking}
              onClick={() => void handleCopy('uuid', normalizedId)}
              data-testid={`${prefix}-copy-uuid`}
            >
              {copiedKind === 'uuid' ? (
                <Check className="mr-2 h-4 w-4" />
              ) : (
                <Copy className="mr-2 h-4 w-4" />
              )}
              {copiedKind === 'uuid' ? t('viewerIdentity.copied') : t('viewerIdentity.copyUuid')}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={isLinking}
              onClick={() =>
                void handleCopy('header', buildMcpMachineHeaderSnippet(normalizedId))
              }
              data-testid={`${prefix}-copy-header`}
            >
              {copiedKind === 'header' ? (
                <Check className="mr-2 h-4 w-4" />
              ) : (
                <Copy className="mr-2 h-4 w-4" />
              )}
              {copiedKind === 'header' ? t('viewerIdentity.copied') : t('viewerIdentity.copyHeader')}
            </Button>
          </div>
        </>
      ) : (
        <>
          <p className="text-xs text-[rgb(var(--muted))]" data-testid={`${prefix}-pending-hint`}>
            {t('viewerIdentity.idPendingHint')}
          </p>
          {onLink ? (
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <input
                type="text"
                value={linkMachineIdDraft}
                onChange={(e) => {
                  setLocalLinkError(null);
                  onLinkMachineIdDraftChange?.(e.target.value);
                }}
                placeholder={t('viewerIdentity.linkIdPlaceholder')}
                disabled={isLinking}
                className="min-w-0 flex-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-xs"
                data-testid={`${prefix}-link-input`}
              />
              <Button
                variant="secondary"
                size="sm"
                disabled={isLinking || !linkMachineIdDraft.trim()}
                onClick={() => void handleLink()}
                data-testid={`${prefix}-link-btn`}
              >
                {isLinking ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                {t('viewerIdentity.linkButton')}
              </Button>
            </div>
          ) : null}
        </>
      )}

      {resolvedLinkError ? (
        <p className="text-xs text-red-500" data-testid={`${prefix}-link-error`}>
          {resolvedLinkError}
        </p>
      ) : null}
    </div>
  );
}
