import type { ReactElement, ReactNode } from 'react';
import { render, type RenderOptions, type RenderResult } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import i18n from '../../apps/desktop/src/i18n';

interface RenderWithI18nOptions extends Omit<RenderOptions, 'wrapper'> {
  wrapper?: ({ children }: { children: ReactNode }) => ReactElement;
}

/**
 * Render a component tree with the desktop app's initialized i18n instance.
 */
export function renderWithI18n(
  ui: ReactElement,
  options: RenderWithI18nOptions = {},
): RenderResult {
  const { wrapper: OuterWrapper, ...renderOptions } = options;

  function Wrapper({ children }: { children: ReactNode }) {
    const inner = <I18nextProvider i18n={i18n}>{children}</I18nextProvider>;
    if (OuterWrapper) {
      return <OuterWrapper>{inner}</OuterWrapper>;
    }
    return inner;
  }

  return render(ui, { wrapper: Wrapper, ...renderOptions });
}
