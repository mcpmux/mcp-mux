/**
 * Shared server icon component that handles both URL-based and emoji icons.
 *
 * Server definitions may have an `icon` field that is either:
 * - An HTTP(S) URL to an image (e.g., GitHub avatar)
 * - An emoji string (e.g., "📦")
 * - null/undefined
 */

import { useState } from 'react';

interface ServerIconProps {
  icon: string | null | undefined;
  /** CSS classes for the img element when rendering a URL icon */
  className?: string;
  /** Fallback emoji when icon is missing or fails to load (default: '📦') */
  fallback?: string;
}

export function ServerIcon({ icon, className = 'w-9 h-9 object-contain', fallback = '📦' }: ServerIconProps) {
  const [failed, setFailed] = useState(false);

  if (!icon || failed) {
    return <span data-testid="server-icon-fallback">{fallback}</span>;
  }

  if (icon.startsWith('http')) {
    return (
      <img
        src={icon}
        alt=""
        className={className}
        data-testid="server-icon-img"
        onError={() => setFailed(true)}
      />
    );
  }

  return <span data-testid="server-icon-emoji">{icon}</span>;
}
