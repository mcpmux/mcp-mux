/**
 * Shared name / icon / hostname editor for machine identity forms.
 */

import { Loader2, Monitor } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import { ServerIcon } from '@/components/ServerIcon';

export interface MachineProfileEditorProps {
  nameDraft: string;
  iconDraft: string;
  hostnameDraft: string;
  onNameDraftChange: (value: string) => void;
  onIconDraftChange: (value: string) => void;
  onHostnameDraftChange: (value: string) => void;
  onSave: () => void;
  isSaving: boolean;
  saveDisabled: boolean;
  nameLabel: string;
  iconLabel: string;
  hostnameLabel: string;
  saveLabel: string;
  testIdPrefix?: string;
}

/**
 * Render machine profile fields with icon preview and save action.
 */
export function MachineProfileEditor({
  nameDraft,
  iconDraft,
  hostnameDraft,
  onNameDraftChange,
  onIconDraftChange,
  onHostnameDraftChange,
  onSave,
  isSaving,
  saveDisabled,
  nameLabel,
  iconLabel,
  hostnameLabel,
  saveLabel,
  testIdPrefix = 'machine-profile',
}: MachineProfileEditorProps) {
  const prefix = testIdPrefix;

  return (
    <div className="flex items-start gap-3">
      <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
        {iconDraft.trim() ? (
          <ServerIcon icon={iconDraft.trim()} className="h-7 w-7 object-contain" fallback="🖥️" />
        ) : (
          <Monitor className="h-5 w-5 text-[rgb(var(--muted))]" />
        )}
      </div>
      <div className="min-w-0 flex-1 space-y-3">
        <div>
          <label className="text-xs font-medium text-[rgb(var(--muted))]">{nameLabel}</label>
          <input
            type="text"
            value={nameDraft}
            onChange={(e) => onNameDraftChange(e.target.value)}
            disabled={isSaving}
            className="mt-1 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 text-sm"
            data-testid={`${prefix}-name`}
          />
        </div>
        <div>
          <label className="text-xs font-medium text-[rgb(var(--muted))]">{iconLabel}</label>
          <input
            type="text"
            value={iconDraft}
            onChange={(e) => onIconDraftChange(e.target.value)}
            placeholder="🖥️"
            disabled={isSaving}
            className="mt-1 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 text-sm"
            data-testid={`${prefix}-icon`}
          />
        </div>
        <div>
          <label className="text-xs font-medium text-[rgb(var(--muted))]">{hostnameLabel}</label>
          <input
            type="text"
            value={hostnameDraft}
            onChange={(e) => onHostnameDraftChange(e.target.value)}
            disabled={isSaving}
            className="mt-1 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 font-mono text-sm"
            data-testid={`${prefix}-hostname`}
          />
        </div>
        <Button
          variant="primary"
          size="sm"
          onClick={onSave}
          disabled={isSaving || saveDisabled}
          data-testid={`${prefix}-save`}
        >
          {isSaving ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
          {saveLabel}
        </Button>
      </div>
    </div>
  );
}
