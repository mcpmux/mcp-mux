/**
 * Monaco JSON editor with measured height for flex/Tauri WebView layouts.
 * Avoids height="100%" collapse and re-layouts on container resize.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import Editor, { type Monaco } from '@monaco-editor/react';
import { Loader2 } from 'lucide-react';
import type { editor } from 'monaco-editor';

const MIN_EDITOR_HEIGHT = 200;

const BASE_JSON_OPTIONS: editor.IStandaloneEditorConstructionOptions = {
  minimap: { enabled: false },
  fontSize: 14,
  fontFamily: "'Fira Code', 'Consolas', monospace",
  lineNumbers: 'on',
  scrollBeyondLastLine: false,
  automaticLayout: false,
  tabSize: 2,
  wordWrap: 'on',
  folding: true,
  bracketPairColorization: { enabled: true },
  guides: {
    bracketPairs: true,
    indentation: true,
  },
  padding: { top: 12, bottom: 12 },
};

interface MonacoJsonEditorProps {
  value: string;
  onChange?: (value: string | undefined) => void;
  readOnly?: boolean;
  beforeMount?: (monaco: Monaco) => void;
  onValidate?: (markers: editor.IMarker[]) => void;
  onMount?: (mountedEditor: editor.IStandaloneCodeEditor, monaco: Monaco) => void;
  onMountFailed?: () => void;
  testId?: string;
  extraOptions?: editor.IStandaloneEditorConstructionOptions;
}

/**
 * JSON editor backed by Monaco with ResizeObserver-driven layout.
 */
export function MonacoJsonEditor({
  value,
  onChange,
  readOnly = false,
  beforeMount,
  onValidate,
  onMount,
  onMountFailed,
  testId,
  extraOptions,
}: MonacoJsonEditorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const [height, setHeight] = useState(MIN_EDITOR_HEIGHT);

  const layoutEditor = useCallback(() => {
    const container = containerRef.current;
    const mountedEditor = editorRef.current;
    if (!container || !mountedEditor) {
      return;
    }

    const nextHeight = Math.max(container.clientHeight, MIN_EDITOR_HEIGHT);
    setHeight(nextHeight);
    mountedEditor.layout({ width: container.clientWidth, height: nextHeight });
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const observer = new ResizeObserver(() => {
      layoutEditor();
    });
    observer.observe(container);

    const initialHeight = Math.max(container.clientHeight, MIN_EDITOR_HEIGHT);
    setHeight(initialHeight);

    return () => observer.disconnect();
  }, [layoutEditor]);

  /**
   * Mount handler — stores editor ref, lays out, and notifies parent.
   */
  const handleMount = (mountedEditor: editor.IStandaloneCodeEditor, monaco: Monaco) => {
    editorRef.current = mountedEditor;

    const container = containerRef.current;
    if (!container || container.clientHeight === 0) {
      onMountFailed?.();
      return;
    }

    layoutEditor();
    onMount?.(mountedEditor, monaco);

    if (!readOnly) {
      mountedEditor.focus();
    }
  };

  const options: editor.IStandaloneEditorConstructionOptions = {
    ...BASE_JSON_OPTIONS,
    ...extraOptions,
    readOnly,
    domReadOnly: readOnly,
    formatOnPaste: readOnly ? undefined : true,
    formatOnType: readOnly ? undefined : true,
  };

  return (
    <div ref={containerRef} className="h-full w-full" data-testid={testId}>
      <Editor
        height={height}
        defaultLanguage="json"
        value={value}
        theme="vs-dark"
        onChange={onChange}
        beforeMount={beforeMount}
        onMount={handleMount}
        onValidate={onValidate}
        options={options}
        loading={
          <div className="flex h-full items-center justify-center bg-[#1e1e1e]">
            <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
          </div>
        }
      />
    </div>
  );
}
