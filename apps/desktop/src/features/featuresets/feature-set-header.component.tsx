import { useTranslation } from 'react-i18next';
import { Package, Star, X } from 'lucide-react';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';
import type { FeatureSet } from '@/lib/api/featureSets';
import type { GeneralInfo } from './feature-set-general-info.hooks';

interface HeaderProps {
  featureSet: FeatureSet;
  isStarter: boolean;
  values: GeneralInfo;
  onFieldChange: (field: keyof GeneralInfo, value: string) => void;
  onClose: () => void;
}

const INLINE_INPUT_CLASS =
  'w-full bg-transparent rounded-md px-2 py-1 border border-transparent hover:border-[rgb(var(--border))] focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500 transition-colors';

/**
 * Panel header with inline name, description, and icon editing plus the type badge.
 * The FeatureSet id lives in the badge tooltip.
 */
export function FeatureSetHeader({
  featureSet,
  isStarter,
  values,
  onFieldChange,
  onClose,
}: HeaderProps) {
  const { t } = useTranslation(['featuresets', 'common']);
  const isCustom = featureSet.feature_set_type === 'custom';
  const FallbackIcon = isStarter ? Star : Package;
  let badgeClass =
    'bg-gray-50 dark:bg-gray-900/20 text-gray-700 dark:text-gray-400 border-gray-200 dark:border-gray-800';
  if (isStarter) {
    badgeClass =
      'bg-yellow-50 dark:bg-yellow-900/20 text-yellow-700 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800';
  } else if (isCustom) {
    badgeClass =
      'bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-400 border-purple-200 dark:border-purple-800';
  }

  return (
    <div className="flex-shrink-0 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
      <div className="flex items-start gap-3">
        <div className="flex flex-shrink-0 flex-col items-center gap-1">
          <EmojiPickerButton
            value={values.icon}
            onChange={(emoji) => onFieldChange('icon', emoji)}
            testId="featureset-panel-icon"
          />
          {!values.icon && <FallbackIcon className="h-4 w-4 text-[rgb(var(--muted))]" />}
        </div>
        <div className="min-w-0 flex-1 space-y-1">
          <input
            type="text"
            value={values.name}
            onChange={(event) => onFieldChange('name', event.target.value)}
            aria-label={t('panel.nameLabel')}
            className={`${INLINE_INPUT_CLASS} text-lg font-bold`}
            data-testid="featureset-panel-name"
          />
          <input
            type="text"
            value={values.description}
            onChange={(event) => onFieldChange('description', event.target.value)}
            placeholder={t('panel.descriptionPlaceholder')}
            aria-label={t('panel.descriptionLabel')}
            className={`${INLINE_INPUT_CLASS} text-sm text-[rgb(var(--muted))]`}
            data-testid="featureset-panel-description"
          />
          <span
            title={isStarter ? t('panel.starterTitle') : `ID: ${featureSet.id}`}
            className={`ml-2 inline-block rounded-full border px-1.5 py-0.5 text-[10px] font-medium ${badgeClass}`}
          >
            {isStarter ? t('panel.starter') : featureSet.feature_set_type.toUpperCase()}
          </span>
        </div>
        <button
          type="button"
          data-testid="featureset-panel-close"
          aria-label={t('panel.close')}
          onClick={onClose}
          className="flex-shrink-0 rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))]"
        >
          <X className="h-5 w-5" />
        </button>
      </div>
    </div>
  );
}
