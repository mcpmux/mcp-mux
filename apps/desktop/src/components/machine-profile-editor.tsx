/**
 * Shared name / icon / hostname editor for machine identity forms.
 */

import { Loader2 } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';

export interface MachineProfileEditorProps {
  nameDraft: string;
  iconDraft: string;
  hostnameDraft: string;
  onNameDraftChange: (value: string) => void;
  onIconDraftChange: (value: string) => void;
  onHostnameDraftChange: (value: string) => void;
  onHostnameFocus?: () => void;
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
  onHostnameFocus,
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
    <div className="space-y-3">
      <div className="flex items-end gap-3">
        <div className="flex-shrink-0">
          <label className="text-xs font-medium text-[rgb(var(--muted))]">{iconLabel}</label>
          <div className="mt-1">
            <EmojiPickerButton
              value={iconDraft}
              onChange={onIconDraftChange}
              disabled={isSaving}
              testId={`${prefix}-icon`}
            />
          </div>
        </div>
        <div className="min-w-0 flex-1">
          <label className="text-xs font-medium text-[rgb(var(--muted))]">{nameLabel}</label>
          <input
            type="text"
            value={nameDraft}
            onChange={(e) => onNameDraftChange(e.target.value)}
            disabled={isSaving}
            className="mt-1 h-10 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 text-sm"
            data-testid={`${prefix}-name`}
          />
        </div>
      </div>
      <div>
        <label className="text-xs font-medium text-[rgb(var(--muted))]">{hostnameLabel}</label>
        <input
          type="text"
          value={hostnameDraft}
          onChange={(e) => onHostnameDraftChange(e.target.value)}
          onFocus={onHostnameFocus}
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
  );
}
