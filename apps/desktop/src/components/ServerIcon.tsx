/**
 * Shared server icon component that handles both URL-based and emoji icons.
 *
 * Server definitions may have an `icon` field that is either:
 * - An HTTP(S) URL to an image (e.g., GitHub avatar)
 * - An emoji string (e.g., "📦")
 * - null/undefined
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { resolveWorkspaceIconDisplaySrc } from '@/lib/api/workspaceAppearances';

interface ServerIconProps {
  icon: string | null | undefined;
  /** CSS classes for the img element when rendering a URL icon */
  className?: string;
  /** Fallback emoji when icon is missing or fails to load (default: '📦') */
  fallback?: string;
}

/**
 * Renders a server icon from a remote URL, local file reference, emoji, or fallback.
 */
export function ServerIcon({ icon, className = 'w-9 h-9 object-contain', fallback = '📦' }: ServerIconProps) {
  const isLocalRef = icon?.startsWith('local:') ?? false;
  const isRemoteUrl = icon?.startsWith('http') ?? false;
  const [failedIcon, setFailedIcon] = useState<string | null>(null);
  const [localResolved, setLocalResolved] = useState<{ icon: string; src: string | null } | null>(
    null
  );
  const blobUrlRef = useRef<string | null>(null);
  const hasFailed = icon != null && failedIcon === icon;
  const localSrc =
    localResolved != null && localResolved.icon === icon ? localResolved.src : null;
  const resolvedSrc = isRemoteUrl && icon ? icon : isLocalRef ? localSrc : null;

  useEffect(() => {
    if (!icon || !isLocalRef) {
      return;
    }

    const localIcon = icon;
    let cancelled = false;
    void resolveWorkspaceIconDisplaySrc(localIcon)
      .then((src) => {
        if (cancelled) {
          if (src?.startsWith('blob:')) {
            URL.revokeObjectURL(src);
          }
          return;
        }
        if (blobUrlRef.current) {
          URL.revokeObjectURL(blobUrlRef.current);
          blobUrlRef.current = null;
        }
        if (src?.startsWith('blob:')) {
          blobUrlRef.current = src;
        }
        setLocalResolved({ icon: localIcon, src });
      })
      .catch(() => {
        if (cancelled) {
          return;
        }
        setFailedIcon(localIcon);
      });

    return () => {
      cancelled = true;
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
        blobUrlRef.current = null;
      }
    };
  }, [icon, isLocalRef]);

  const shouldRenderImage = useMemo(
    () => isRemoteUrl || isLocalRef,
    [isLocalRef, isRemoteUrl]
  );

  if (!icon || hasFailed) {
    return <span data-testid="server-icon-fallback">{fallback}</span>;
  }

  if (shouldRenderImage) {
    if (!resolvedSrc) {
      return <span data-testid="server-icon-fallback">{fallback}</span>;
    }
    return (
      <img
        src={resolvedSrc}
        alt=""
        className={className}
        data-testid="server-icon-img"
        onError={() => setFailedIcon(icon)}
      />
    );
  }

  return <span data-testid="server-icon-emoji">{icon}</span>;
}
