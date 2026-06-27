/**
 * Button that opens an emoji picker popover.
 * Uses emoji-picker-element (web component) — no external data fetch required.
 * The popover renders in a portal with fixed positioning so it is never clipped
 * by a scrolling drawer/panel and always stays inside the viewport.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Smile } from 'lucide-react';
import type { EmojiClickEvent } from 'emoji-picker-element/shared';
import 'emoji-picker-element';

const PICKER_WIDTH = 344;
const PICKER_HEIGHT = 398;
const GAP = 4;
const VIEWPORT_MARGIN = 8;

interface Props {
  value: string;
  onChange: (emoji: string) => void;
  disabled?: boolean;
  testId?: string;
}

/**
 * Compute a fixed popover position anchored to the trigger, clamped to the viewport.
 */
function computePosition(rect: DOMRect): { top: number; left: number } {
  const left = Math.min(
    Math.max(VIEWPORT_MARGIN, rect.left + rect.width / 2 - PICKER_WIDTH / 2),
    window.innerWidth - PICKER_WIDTH - VIEWPORT_MARGIN,
  );
  const spaceBelow = window.innerHeight - rect.bottom;
  const top =
    spaceBelow < PICKER_HEIGHT + GAP + VIEWPORT_MARGIN && rect.top > PICKER_HEIGHT
      ? rect.top - PICKER_HEIGHT - GAP
      : rect.bottom + GAP;
  return { top, left };
}

/**
 * Compact emoji picker trigger button with a portal-rendered popover.
 */
export function EmojiPickerButton({ value, onChange, disabled, testId }: Props) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open || !buttonRef.current) return;
    setPos(computePosition(buttonRef.current.getBoundingClientRect()));
  }, [open]);

  useEffect(() => {
    if (!open || !pos) return;
    const picker = popoverRef.current?.querySelector('emoji-picker');
    if (!picker) return;

    const handlePick = (e: Event) => {
      const detail = (e as CustomEvent<EmojiClickEvent['detail']>).detail;
      if (detail.unicode) {
        onChange(detail.unicode);
        setOpen(false);
      }
    };

    picker.addEventListener('emoji-click', handlePick);
    return () => picker.removeEventListener('emoji-click', handlePick);
  }, [open, pos, onChange]);

  useEffect(() => {
    if (!open) return;
    const handleOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (
        !buttonRef.current?.contains(target) &&
        !popoverRef.current?.contains(target)
      ) {
        setOpen(false);
      }
    };
    const handleScrollOrResize = () => setOpen(false);
    document.addEventListener('mousedown', handleOutside);
    window.addEventListener('resize', handleScrollOrResize);
    window.addEventListener('scroll', handleScrollOrResize, true);
    return () => {
      document.removeEventListener('mousedown', handleOutside);
      window.removeEventListener('resize', handleScrollOrResize);
      window.removeEventListener('scroll', handleScrollOrResize, true);
    };
  }, [open]);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        disabled={disabled}
        data-testid={testId}
        onClick={() => setOpen((prev) => !prev)}
        className={[
          'flex h-10 w-10 items-center justify-center rounded-lg border text-lg transition-colors',
          'border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]',
          disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer',
        ].join(' ')}
        aria-label="Pick an emoji"
        aria-expanded={open}
      >
        {value.trim() ? value.trim() : <Smile className="h-5 w-5 text-[rgb(var(--muted))]" />}
      </button>

      {open &&
        pos &&
        createPortal(
          <div
            ref={popoverRef}
            className="fixed z-[1000] shadow-xl rounded-lg overflow-hidden"
            style={{ top: pos.top, left: pos.left, width: PICKER_WIDTH }}
          >
            {/* ponytail: emoji-picker-element renders its own shadow DOM; sizing via style only */}
            {/* @ts-expect-error -- emoji-picker is a custom web component not in React's intrinsic elements */}
            <emoji-picker style={{ width: '100%' }} />
          </div>,
          document.body,
        )}
    </>
  );
}
