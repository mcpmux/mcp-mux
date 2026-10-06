/**
 * The `@mux` prompt surfaces (Mapping, FeatureSets, tool warning, status bar)
 * all teach one copyable prompt. Copying must put exactly that prompt on the
 * clipboard and confirm it.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { MuxPromptBanner, MuxPromptCode, MuxStatusChip } from '@/components/MuxPrompt';
import { MUX_OPTIMIZE_PROMPT } from '@/hooks/useCopyPrompt';

describe('MuxPrompt', () => {
  let writeText: ReturnType<typeof vi.fn>;

  /** user-event installs its own clipboard stub, so ours goes in after it. */
  function setupUser() {
    const user = userEvent.setup();
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    });
    return user;
  }

  it('shows the canonical prompt and copies it', async () => {
    const user = setupUser();
    render(<MuxPromptCode testId="copy" />);

    expect(screen.getByText(MUX_OPTIMIZE_PROMPT)).toBeInTheDocument();
    await user.click(screen.getByTestId('copy'));

    expect(writeText).toHaveBeenCalledWith(MUX_OPTIMIZE_PROMPT);
    expect(screen.getByTestId('copy')).toHaveAttribute('aria-label', 'Copied');
  });

  it('banner renders its title, context, and a copy button', async () => {
    const user = setupUser();
    render(
      <MuxPromptBanner title="Let your AI build these for you" testId="hint">
        Start a message with @mux.
      </MuxPromptBanner>
    );

    expect(screen.getByText('Let your AI build these for you')).toBeInTheDocument();
    expect(screen.getByText('Start a message with @mux.')).toBeInTheDocument();
    await user.click(screen.getByTestId('hint-copy'));
    expect(writeText).toHaveBeenCalledWith(MUX_OPTIMIZE_PROMPT);
  });

  it('status-bar chip copies the prompt in one click', async () => {
    const user = setupUser();
    render(<MuxStatusChip />);

    const chip = screen.getByTestId('statusbar-mux');
    expect(chip).toHaveTextContent('@mux');
    expect(chip.getAttribute('title')).toContain(MUX_OPTIMIZE_PROMPT);

    await user.click(chip);
    expect(writeText).toHaveBeenCalledWith(MUX_OPTIMIZE_PROMPT);
    expect(chip).toHaveTextContent('Prompt copied');
  });
});
