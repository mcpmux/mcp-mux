import { useTranslation } from 'react-i18next';
import { ChevronDown, Compass, FileJson, Plus } from 'lucide-react';
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@mcpmux/ui';

interface AddServerMenuProps {
  /** Opens the Discover page to browse the community server registry. */
  onDiscover: () => void;
  /** Opens the guided custom-server panel (Form / JSON). */
  onCustom: () => void;
  /** Opens the full-space JSON manifest editor. */
  onViewManifest: () => void;
}

/**
 * Dropdown for adding MCP servers: registry discover, guided custom setup, or full manifest.
 */
export function AddServerMenu({ onDiscover, onCustom, onViewManifest }: AddServerMenuProps) {
  const { t } = useTranslation('servers');

  return (
    <DropdownMenu>
      <DropdownMenuTrigger data-testid="add-server-menu-trigger">
        <Button variant="primary" size="md" type="button">
          <Plus className="h-4 w-4" />
          {t('addMenu.addServer')}
          <ChevronDown className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80 p-1.5" data-testid="add-server-menu">
        <DropdownMenuItem
          icon={Compass}
          label={t('addMenu.discover')}
          description={t('addMenu.discoverDesc')}
          onSelect={onDiscover}
          data-testid="add-server-option-discover"
        />
        <DropdownMenuItem
          icon={Plus}
          label={t('addMenu.custom')}
          description={t('addMenu.customDesc')}
          onSelect={onCustom}
          data-testid="add-server-option-custom"
        />
        <DropdownMenuItem
          icon={FileJson}
          label={t('addMenu.viewManifest')}
          description={t('addMenu.viewManifestDesc')}
          onSelect={onViewManifest}
          data-testid="add-server-option-view-manifest"
        />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
