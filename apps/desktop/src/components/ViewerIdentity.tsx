/**
 * Viewer identity — name prompt modal and status bar indicator.
 */

import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import { Button, Card, CardContent } from '@mcpmux/ui';
import { MachineProfileEditor } from '@/components/machine-profile-editor';
import { ServerIcon } from '@/components/ServerIcon';
import { useViewerIdentity } from '@/hooks/use-viewer-identity.hook';
import { NAV_SETTINGS } from '@/lib/navigation';
import { useNavigateTo } from '@/stores';

/**
 * Blocking modal for first-time viewer device naming.
 */
export function ViewerIdentityModal() {
  const { t } = useTranslation(['common', 'settings']);
  const navigateTo = useNavigateTo();
  const {
    name,
    hints,
    showPrompt,
    isLoading,
    isSaving,
    nameDraft,
    iconDraft,
    hostnameDraft,
    setNameDraft,
    setIconDraft,
    setHostnameDraft,
    error,
    setError,
    saveProfile,
    canSaveProfile,
    closePrompt,
  } = useViewerIdentity();

  if (isLoading || !showPrompt) {
    return null;
  }

  const handleSave = async () => {
    const ok = await saveProfile();
    if (!ok && !error) {
      setError(t('common:viewerIdentity.nameRequired'));
    }
  };

  const handleOpenSettings = () => {
    if (name) {
      closePrompt();
    }
    navigateTo(NAV_SETTINGS.key);
  };

  const errorMessage =
    error === 'saveFailed'
      ? t('common:viewerIdentity.saveFailed')
      : error === 'name' || error === 'icon' || error === 'hostname'
        ? t(`common:viewerIdentity.${error}Required`)
        : error;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <Card className="animate-in fade-in zoom-in relative mx-4 w-full max-w-md shadow-xl duration-200">
        <button
          type="button"
          onClick={closePrompt}
          aria-label={t('common:viewerIdentity.close')}
          disabled={!name || isSaving}
          className="absolute right-3 top-3 rounded-md p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))] disabled:invisible"
        >
          <X className="h-4 w-4" />
        </button>
        <CardContent className="flex flex-col gap-4 px-6 pb-6 pt-8">
          <div className="space-y-1.5 text-center">
            <h2 className="text-lg font-semibold text-[rgb(var(--foreground))]">
              {t('common:viewerIdentity.promptTitle')}
            </h2>
            <p className="text-sm text-[rgb(var(--muted))]">{t('common:viewerIdentity.promptDesc')}</p>
            {hints ? <p className="text-xs text-[rgb(var(--muted))]">{hints}</p> : null}
          </div>

          <MachineProfileEditor
            nameDraft={nameDraft}
            iconDraft={iconDraft}
            hostnameDraft={hostnameDraft}
            onNameDraftChange={setNameDraft}
            onIconDraftChange={setIconDraft}
            onHostnameDraftChange={setHostnameDraft}
            onSave={() => void handleSave()}
            isSaving={isSaving}
            saveDisabled={!canSaveProfile}
            nameLabel={t('settings:machineIdentity.nameLabel')}
            iconLabel={t('settings:machineIdentity.iconLabel')}
            hostnameLabel={t('settings:machineIdentity.hostnameLabel')}
            saveLabel={isSaving ? t('common:viewerIdentity.saving') : t('common:viewerIdentity.save')}
            testIdPrefix="viewer-identity"
          />

          {errorMessage ? <p className="text-sm text-red-500">{errorMessage}</p> : null}

          <p className="text-center text-xs text-[rgb(var(--muted))]">
            {t('common:viewerIdentity.settingsHint')}{' '}
            <button
              type="button"
              onClick={handleOpenSettings}
              disabled={isSaving}
              className="text-[rgb(var(--primary))] underline-offset-2 hover:underline disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t('common:viewerIdentity.settingsLink')}
            </button>
          </p>
        </CardContent>
      </Card>
    </div>
  );
}

/**
 * Status bar indicator for the current viewer device.
 */
export function ViewerIdentityStatusItem() {
  const { t } = useTranslation('common');
  const { name, icon, isLoading, openPrompt } = useViewerIdentity();

  if (isLoading) {
    return null;
  }

  return (
    <button
      type="button"
      onClick={() => openPrompt()}
      className="flex items-center gap-1.5 transition-colors hover:text-[rgb(var(--foreground))]"
      data-testid="statusbar-viewer"
      title={t('viewerIdentity.statusTitle')}
    >
      <span className="inline-flex h-4 w-4 shrink-0 items-center justify-center text-sm leading-none">
        <ServerIcon icon={icon || null} className="h-4 w-4 object-contain" fallback="🖥️" />
      </span>
      {name
        ? t('viewerIdentity.statusLabel', { name })
        : t('viewerIdentity.statusUnset')}
    </button>
  );
}
