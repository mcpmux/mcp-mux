import { useEffect, useState } from 'react';
import Editor, { type EditorProps } from '@monaco-editor/react';
import { loadLocalMonaco } from '@/lib/monaco';

/** Monaco editor backed by the bundled Monaco (never the CDN default). */
export function LocalEditor(props: EditorProps) {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let live = true;
    loadLocalMonaco()
      .then(() => live && setReady(true))
      .catch((err) => console.error('[Editor] Failed to load Monaco:', err));
    return () => {
      live = false;
    };
  }, []);

  if (!ready) return <>{props.loading ?? null}</>;
  return <Editor {...props} />;
}
