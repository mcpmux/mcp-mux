/**
 * Serve Monaco from the app bundle instead of a CDN.
 *
 * `@monaco-editor/react` downloads Monaco from jsDelivr by default, which would
 * run third-party code in the app window. Handing the loader the bundled
 * instance (and bundled workers) keeps every script local, which the
 * Content-Security-Policy in `tauri.conf.json` also requires. Monaco is large,
 * so it is loaded on first use rather than at startup.
 */
import { loader } from '@monaco-editor/react';

let ready: Promise<void> | null = null;

export function loadLocalMonaco(): Promise<void> {
  ready ??= (async () => {
    // The core editor plus the JSON language only: the full `monaco-editor`
    // entry would also bundle the CSS/HTML/TypeScript language workers (~9 MB)
    // that the app never uses.
    const [core, json, { default: EditorWorker }, { default: JsonWorker }] = await Promise.all([
      import('monaco-editor/esm/vs/editor/edcore.main'),
      import('monaco-editor/esm/vs/language/json/monaco.contribution'),
      import('monaco-editor/esm/vs/editor/editor.worker?worker'),
      import('monaco-editor/esm/vs/language/json/json.worker?worker'),
    ]);
    // Same wiring as monaco-editor's own entry point, for JSON only.
    const monaco = core as typeof import('monaco-editor');
    (monaco.languages as unknown as Record<string, unknown>).json = json;
    self.MonacoEnvironment = {
      getWorker(_workerId: string, label: string) {
        return label === 'json' ? new JsonWorker() : new EditorWorker();
      },
    };
    loader.config({ monaco });
  })();
  return ready;
}
