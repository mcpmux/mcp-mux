import { useTranslation } from 'react-i18next';
import { Check, FileText, MessageSquare, Monitor, Wrench } from 'lucide-react';
import type { FeatureType, ServerFeature } from '@/lib/api/serverFeatures';
import { getFeatureLabel } from './feature-set-features.helpers';

interface FeatureRowProps {
  feature: ServerFeature;
  isSelected: boolean;
  isSurfaced: boolean;
  onToggle: (id: string) => void;
  onToggleSurfaced: (id: string) => void;
}

const TYPE_ICONS: Record<FeatureType, typeof Wrench> = {
  tool: Wrench,
  prompt: MessageSquare,
  resource: FileText,
};

const TYPE_ICON_COLORS: Record<FeatureType, string> = {
  tool: 'text-purple-500',
  prompt: 'text-blue-500',
  resource: 'text-green-500',
};

const TYPE_PILL_COLORS: Record<FeatureType, string> = {
  tool: 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300',
  prompt: 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300',
  resource: 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300',
};

const SURFACE_TITLE_KEYS = {
  tool: 'panel.surfaceToolTitle',
  prompt: 'panel.surfacePromptTitle',
  resource: 'panel.surfaceResourceTitle',
} as const satisfies Record<FeatureType, string>;

/**
 * One tool, prompt, or resource: checkbox, title, raw MCP name, and Surface toggle.
 */
export function FeatureSetFeatureRow({
  feature,
  isSelected,
  isSurfaced,
  onToggle,
  onToggleSurfaced,
}: FeatureRowProps) {
  const { t } = useTranslation('featuresets');
  const TypeIcon = TYPE_ICONS[feature.feature_type];
  const label = getFeatureLabel(feature);
  const hasDistinctName = label !== feature.feature_name;

  return (
    <div
      className={`flex items-center gap-2 border-b border-[rgb(var(--border))] transition-colors last:border-b-0 ${
        isSelected ? 'bg-primary-50 dark:bg-primary-900/10' : ''
      } ${feature.is_available ? '' : 'opacity-60'}`}
    >
      <button
        type="button"
        onClick={() => onToggle(feature.id)}
        className="flex min-w-0 flex-1 items-center gap-3 px-4 py-2.5 pl-12 text-left transition-colors hover:bg-[rgb(var(--surface-hover))]"
      >
        <div
          className={`flex h-4 w-4 flex-shrink-0 items-center justify-center rounded border transition-colors ${
            isSelected
              ? 'bg-primary-500 border-primary-500'
              : 'border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--surface))]'
          }`}
        >
          {isSelected && <Check className="h-3 w-3 text-white" />}
        </div>

        <TypeIcon className={`h-4 w-4 flex-shrink-0 ${TYPE_ICON_COLORS[feature.feature_type]}`} />

        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-sm font-medium">{label}</span>
            {hasDistinctName && (
              <span className="truncate font-mono text-xs text-[rgb(var(--muted))]">
                {feature.feature_name}
              </span>
            )}
            <span
              className={`flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] ${TYPE_PILL_COLORS[feature.feature_type]}`}
            >
              {feature.feature_type}
            </span>
            {!feature.is_available && (
              <span className="flex-shrink-0 rounded bg-gray-100 px-1.5 py-0.5 text-[10px] text-gray-600 dark:bg-gray-800 dark:text-gray-400">
                {t('panel.unavailable')}
              </span>
            )}
          </div>
          {feature.description && (
            <p className="mt-0.5 line-clamp-1 text-xs text-[rgb(var(--muted))]">
              {feature.description}
            </p>
          )}
        </div>
      </button>

      {isSelected && (
        <button
          type="button"
          onClick={() => onToggleSurfaced(feature.id)}
          className={`mr-3 flex flex-shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium transition-colors ${
            isSurfaced
              ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300'
              : 'text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))]'
          }`}
          title={t(SURFACE_TITLE_KEYS[feature.feature_type])}
          data-testid={`surface-toggle-${feature.id}`}
        >
          <Monitor className="h-3.5 w-3.5" />
          {t('panel.surface')}
        </button>
      )}
    </div>
  );
}
