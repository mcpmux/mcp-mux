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
  /** Opens the Space JSON editor to add a custom server definition. */
  onCustom: () => void;
}

/**
 * Dropdown for the two ways to add MCP servers: registry discover vs custom JSON.
 */
export function AddServerMenu({ onDiscover, onCustom }: AddServerMenuProps) {
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
          icon={FileJson}
          label={t('addMenu.custom')}
          description={t('addMenu.customDesc')}
          onSelect={onCustom}
          data-testid="add-server-option-custom"
        />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
