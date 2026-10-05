import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import jetbrainsIcon from '@/assets/client-icons/jetbrains.svg';
import androidStudioIcon from '@/assets/client-icons/android-studio.svg';
import opencodeIcon from '@/assets/client-icons/opencode.svg';
import opencodeIconDark from '@/assets/client-icons/opencode-dark.svg';
import { ClientBrandIcon } from '@/components/ClientBrandIcon';
import { resolveKnownClientKey } from '@/lib/clientIcons';

// Bundled icons for well-known AI clients.
const CLIENT_ICON_ASSETS: Record<string, string> = {
  cursor: cursorIcon,
  vscode: vscodeIcon,
  claude: claudeIcon,
  windsurf: windsurfIcon,
  jetbrains: jetbrainsIcon,
  'android-studio': androidStudioIcon,
};

/**
 * Icon for an inbound AI client — a bundled brand mark for well-known clients,
 * else the client's self-reported `logo_uri`, else a generic robot. Shared by
 * the Clients and Mapping tabs so a client looks the same everywhere.
 */
export function ClientIcon({
  logo_uri,
  client_name,
}: {
  logo_uri?: string | null;
  client_name: string;
}) {
  const knownKey = resolveKnownClientKey(client_name);
  // opencode ships theme-specific marks; render our bundled official logo
  // (overriding any outdated self-reported logo_uri).
  if (knownKey === 'opencode') {
    return (
      <ClientBrandIcon
        light={opencodeIcon}
        dark={opencodeIconDark}
        alt={client_name}
        className="h-full w-full rounded object-contain"
      />
    );
  }
  const iconUrl = (knownKey && CLIENT_ICON_ASSETS[knownKey]) || logo_uri;
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={client_name}
        className="h-full w-full rounded object-contain"
        onError={(e) => {
          e.currentTarget.style.display = 'none';
          e.currentTarget.parentElement!.append(document.createTextNode('🤖'));
        }}
      />
    );
  }
  return <span>🤖</span>;
}
