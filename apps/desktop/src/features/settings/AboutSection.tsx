import { useTranslation } from 'react-i18next';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { Info } from 'lucide-react';
import { BuildStampPanelContent } from './BuildStampPanel';
import { useBuildStamp } from './use-build-stamp.hook';

/**
 * Web-admin Settings card showing app version and build stamp metadata.
 */
export function AboutSection() {
  const { t } = useTranslation('settings');
  const stamp = useBuildStamp();

  return (
    <Card data-testid="about-section">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Info className="h-5 w-5" />
          {t('about.title')}
        </CardTitle>
        <CardDescription>{t('about.description')}</CardDescription>
      </CardHeader>
      <CardContent>
        <div>
          <label className="text-sm font-medium">{t('about.currentVersion')}</label>
          <p className="text-sm text-[rgb(var(--muted))] mt-1" data-testid="current-version">
            {stamp.loading
              ? t('about.loading')
              : `v${stamp.version || t('about.unknownVersion')}`}
          </p>
          <BuildStampPanelContent context="web-admin" stamp={stamp} />
        </div>
      </CardContent>
    </Card>
  );
}
