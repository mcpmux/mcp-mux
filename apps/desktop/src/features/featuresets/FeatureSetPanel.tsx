import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ToastContainer, useConfirm, useToast } from '@mcpmux/ui';
import type { AddMemberInput, FeatureSet } from '@/lib/api/featureSets';
import {
  isStarterFeatureSet,
  setFeatureSetAutoInclude,
  setFeatureSetMembers,
  updateFeatureSet,
} from '@/lib/api/featureSets';
import { useStarterToolSummary } from '@/hooks/useStarterToolSummary';
import { FeatureSetFooter } from './feature-set-footer.component';
import { FeatureSetFeaturesSection } from './feature-set-features-section.component';
import { FeatureSetHeader } from './feature-set-header.component';
import {
  FeatureSetAutoIncludeCard,
  FeatureSetToolCountWarning,
} from './feature-set-notices.component';
import { useFeatureSetCatalog } from './feature-set-catalog.hooks';
import { useFeatureSetGeneralInfo } from './feature-set-general-info.hooks';
import { useFeatureSetSelection } from './feature-set-selection.hooks';
import { getAvailableIds } from './feature-set-features.helpers';

interface FeatureSetPanelProps {
  featureSet: FeatureSet;
  spaceId: string;
  onClose: () => void;
  onDelete?: (id: string) => void;
  onUpdate?: () => void;
}

/**
 * Slide-out editor for one FeatureSet: inline name, description, and icon, an auto-mode card,
 * and a searchable member picker, all saved together from the footer. Owns its overlay and
 * Escape handling so it can ask before discarding unsaved edits.
 */
export function FeatureSetPanel({
  featureSet,
  spaceId,
  onClose,
  onDelete,
  onUpdate,
}: FeatureSetPanelProps) {
  const { t } = useTranslation(['featuresets', 'common']);
  const { features, installed, isLoading, loadError } = useFeatureSetCatalog(
    spaceId,
    featureSet.id
  );
  const selection = useFeatureSetSelection(featureSet, features);
  const general = useFeatureSetGeneralInfo(featureSet);
  const { summary: starterSummary } = useStarterToolSummary(spaceId);
  const { toasts, success, error: showError, dismiss } = useToast();
  const {
    confirm,
    ConfirmDialogElement,
    isOpen: isConfirmOpen,
    cancel: cancelConfirm,
  } = useConfirm();
  const [autoInclude, setAutoInclude] = useState(featureSet.auto_include);
  const [isTogglingAuto, setIsTogglingAuto] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const error = actionError ?? loadError;

  const isStarter = isStarterFeatureSet(featureSet);
  const isCustom = featureSet.feature_set_type === 'custom';
  const isDirty = selection.isEdited || general.hasChanges;
  const availableFeatures = features.filter((feature) => feature.is_available);
  const selectedAvailableCount = availableFeatures.filter((feature) =>
    selection.selectedIds.has(feature.id)
  ).length;
  const selectedToolCount = availableFeatures.filter(
    (feature) => feature.feature_type === 'tool' && selection.selectedIds.has(feature.id)
  ).length;
  const toolThreshold = starterSummary?.threshold;
  const isOverToolThreshold = toolThreshold !== undefined && selectedToolCount > toolThreshold;
  const canSave = !isSaving && isDirty;

  /**
   * Close, asking first when there are unsaved edits.
   */
  const requestClose = useCallback(async () => {
    if (isDirty) {
      const shouldDiscard = await confirm({
        title: t('confirm.discardTitle'),
        message: t('confirm.discardMessage'),
        confirmLabel: t('confirm.discardLabel'),
        cancelLabel: t('common:actions.cancel'),
        variant: 'danger',
      });
      if (!shouldDiscard) {
        return;
      }
    }
    onClose();
  }, [isDirty, confirm, onClose, t]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') {
        return;
      }
      if (isConfirmOpen) {
        cancelConfirm();
        return;
      }
      void requestClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [requestClose, isConfirmOpen, cancelConfirm]);

  /**
   * Switch auto mode. Turning it on replaces the selection, so it asks first; turning it
   * off keeps what the set granted as an editable list.
   */
  const handleAutoToggle = async (enabled: boolean) => {
    if (
      enabled &&
      !(await confirm({
        title: t('auto.confirmTitle'),
        message: t('auto.confirmMessage', { name: featureSet.name }),
        confirmLabel: t('auto.confirmLabel'),
        cancelLabel: t('common:actions.cancel'),
      }))
    ) {
      return;
    }
    setIsTogglingAuto(true);
    setActionError(null);
    try {
      await setFeatureSetAutoInclude(featureSet.id, enabled);
      setAutoInclude(enabled);
      if (enabled) {
        selection.replaceSelection(getAvailableIds(features));
      } else {
        selection.markSaved();
      }
      success(
        enabled ? t('auto.toastOnTitle') : t('auto.toastOffTitle'),
        enabled
          ? t('auto.toastOnBody', { name: featureSet.name })
          : t('auto.toastOffBody', { name: featureSet.name })
      );
      onUpdate?.();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setActionError(message);
      showError(t('auto.toastFailed'), message);
    } finally {
      setIsTogglingAuto(false);
    }
  };

  /**
   * Save general info and members together. Members keep unavailable features that were
   * already in the set, since bulk actions never touch them.
   */
  const handleSave = async () => {
    const trimmedName = general.values.name.trim();
    if (!trimmedName) {
      setActionError(t('panel.nameRequired'));
      return;
    }
    setIsSaving(true);
    setActionError(null);
    try {
      if (general.hasChanges) {
        const trimmedDescription = general.values.description.trim();
        const updated = await updateFeatureSet(featureSet.id, {
          name: trimmedName,
          description:
            trimmedDescription !== general.baseline.description
              ? trimmedDescription
              : undefined,
          icon: general.values.icon.trim() || undefined,
        });
        general.commit(updated);
      }
      if (selection.isEdited) {
        const members: AddMemberInput[] = Array.from(selection.selectedIds).map((id) => ({
          member_type: 'feature' as const,
          member_id: id,
          mode: 'include' as const,
          surfaced: selection.surfacedIds.has(id),
        }));
        await setFeatureSetMembers(featureSet.id, members);
        setAutoInclude(false);
        selection.markSaved();
      }
      success(
        t('toast.changesSaved'),
        t('toast.changesSavedBody', { name: trimmedName, count: selection.selectedIds.size })
      );
      onUpdate?.();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setActionError(message);
      showError(t('toast.saveChangesFailed'), message);
    } finally {
      setIsSaving(false);
    }
  };

  /**
   * Confirm, then delete this set.
   */
  const handleDelete = async () => {
    const shouldDelete = await confirm({
      title: t('confirm.deleteTitle'),
      message: t('confirm.deleteMessage', { name: featureSet.name }),
      confirmLabel: t('confirm.deleteLabel'),
      cancelLabel: t('common:actions.cancel'),
      variant: 'danger',
    });
    if (shouldDelete) {
      onDelete?.(featureSet.id);
    }
  };

  return (
    <>
      <div
        data-testid="featureset-panel-overlay"
        className="animate-in fade-in fixed inset-0 z-40 bg-black/20 backdrop-blur-[2px] duration-200"
        onClick={() => void requestClose()}
      />
      <div className="animate-in slide-in-from-right fixed bottom-0 right-0 top-0 z-50 flex w-full min-w-[600px] max-w-[45%] flex-col border-l border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl duration-300">
        <ToastContainer toasts={toasts} onClose={dismiss} />
        {ConfirmDialogElement}
        <FeatureSetHeader
          featureSet={featureSet}
          isStarter={isStarter}
          values={general.values}
          onFieldChange={general.setField}
          onClose={() => void requestClose()}
        />

        <div className="flex-1 overflow-y-auto">
          <div className="space-y-5 p-6">
            {error && (
              <div className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-600 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
                {error}
              </div>
            )}

            {(isStarter || autoInclude) && (
              <FeatureSetAutoIncludeCard
                isAutoInclude={autoInclude}
                isDisabled={isTogglingAuto || isSaving}
                onToggle={handleAutoToggle}
              />
            )}

            {isOverToolThreshold && (
              <FeatureSetToolCountWarning toolCount={selectedToolCount} threshold={toolThreshold} />
            )}

            <div>
              <div className="mb-2 flex items-center justify-between">
                <span className="text-base font-semibold">{t('panel.includedFeatures')}</span>
                <span className="text-xs font-bold text-[rgb(var(--muted))]">
                  {t('panel.selectedCount', {
                    selected: selectedAvailableCount,
                    total: availableFeatures.length,
                  })}
                </span>
              </div>
              {selectedAvailableCount > 0 && (
                <div className="mb-3 h-1.5 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
                  <div
                    className="h-full bg-gradient-to-r from-green-500 to-blue-500 transition-all duration-300"
                    style={{
                      width: `${(selectedAvailableCount / availableFeatures.length) * 100}%`,
                    }}
                  />
                </div>
              )}
              <FeatureSetFeaturesSection
                features={features}
                installed={installed}
                isLoading={isLoading}
                selectedIds={selection.selectedIds}
                surfacedIds={selection.surfacedIds}
                onToggleFeature={selection.toggleFeature}
                onToggleSurfaced={selection.toggleSurfaced}
                onSetIdsSelected={selection.setIdsSelected}
              />
            </div>
          </div>
        </div>

        <FeatureSetFooter
          canDelete={isCustom && !!onDelete}
          canSave={canSave}
          isSaving={isSaving}
          isLeavingAuto={autoInclude && selection.isEdited}
          onDelete={() => void handleDelete()}
          onSave={() => void handleSave()}
        />
      </div>
    </>
  );
}
