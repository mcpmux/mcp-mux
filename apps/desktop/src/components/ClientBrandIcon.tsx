/**
 * Brand icon for an MCP client.
 *
 * Some official marks are theme-specific — opencode's logo is a dark mark meant
 * for light backgrounds and a light mark for dark backgrounds, so neither reads
 * on both themes. When both variants are provided we render both and toggle with
 * Tailwind's `dark:` variant; a single asset is shown as-is. Returns `null` when
 * no asset is given so the caller can render its own fallback glyph.
 */
export function ClientBrandIcon({
  light,
  dark,
  alt = '',
  className = '',
}: {
  light?: string;
  dark?: string;
  alt?: string;
  className?: string;
}) {
  if (!light && !dark) return null;
  if (light && dark) {
    return (
      <>
        <img src={light} alt={alt} className={`${className} block dark:hidden`} />
        <img src={dark} alt={alt} className={`${className} hidden dark:block`} />
      </>
    );
  }
  return <img src={(light ?? dark) as string} alt={alt} className={className} />;
}
