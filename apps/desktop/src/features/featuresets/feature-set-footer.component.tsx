import { useTranslation } from 'react-i18next';
import { Loader2, Save, Trash2 } from 'lucide-react';
import { Button } from '@mcpmux/ui';

interface FooterProps {
  canDelete: boolean;
  canSave: boolean;
  isSaving: boolean;
  onDelete: () => void;
  onSave: () => void;
}

/**
 * Panel footer: delete (custom sets only) and the single Save.
 */
export function FeatureSetFooter({ canDelete, canSave, isSaving, onDelete, onSave }: FooterProps) {
  const { t } = useTranslation(['featuresets', 'common']);

  return (
    <div className="flex flex-shrink-0 items-center gap-3 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
      {canDelete && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onDelete}
          className="mr-auto text-red-500 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20"
          data-testid="featureset-panel-delete"
        >
          <Trash2 className="mr-2 h-4 w-4" />
          {t('common:actions.delete')}
        </Button>
      )}

      <Button
        onClick={onSave}
        disabled={!canSave}
        className="w-full flex-1"
        data-testid="featureset-panel-save-changes"
      >
        {isSaving ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" /> {t('panel.saving')}
          </>
        ) : (
          <>
            <Save className="mr-2 h-4 w-4" /> {t('panel.saveChanges')}
          </>
        )}
      </Button>
    </div>
  );
}
