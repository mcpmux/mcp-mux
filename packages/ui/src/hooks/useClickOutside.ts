import { useEffect, type RefObject } from 'react';

/**
 * Invoke a callback when the user clicks outside all provided element refs.
 */
export function useClickOutside(
  refs: RefObject<HTMLElement | null>[],
  onClickOutside: () => void,
  enabled: boolean
): void {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    function handlePointerDown(event: MouseEvent) {
      const target = event.target as Node;
      const isInside = refs.some((ref) => ref.current?.contains(target));
      if (!isInside) {
        onClickOutside();
      }
    }

    document.addEventListener('mousedown', handlePointerDown);
    return () => document.removeEventListener('mousedown', handlePointerDown);
  }, [refs, onClickOutside, enabled]);
}
