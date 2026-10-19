// Bundle the code editor with the app so LaTeX Studio works fully offline.
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';
import { loader } from '@monaco-editor/react';

self.MonacoEnvironment = { getWorker: () => new editorWorker() };
loader.config({ monaco });

let themesDefined = false;
/** Editor themes that match the app: "Ink" (dark navy) and "Parchment" (light). */
export function defineDomeThemes(monacoInstance) {
  if (themesDefined) return;
  themesDefined = true;
  monacoInstance.editor.defineTheme('dome-dark', {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'comment', foreground: '6f8296', fontStyle: 'italic' },
      { token: 'keyword', foreground: 'f0d28f' },
      { token: 'number', foreground: '9fc6e8' },
      { token: 'string', foreground: '9fd4b0' },
      { token: 'attribute.name', foreground: '9fc6e8' },
      { token: 'delimiter.bracket', foreground: 'c9b98a' },
    ],
    colors: {
      'editor.background': '#0a1827',
      'editor.foreground': '#e9dcb9',
      'editorLineNumber.foreground': '#4a6076',
      'editorLineNumber.activeForeground': '#f0d28f',
      'editor.lineHighlightBackground': '#10243a',
      'editor.selectionBackground': '#27476a99',
      'editorCursor.foreground': '#f0d28f',
      'editorGutter.background': '#0a1827',
      'editorWidget.background': '#0e2033',
      'editorSuggestWidget.background': '#0e2033',
    },
  });
  monacoInstance.editor.defineTheme('dome-light', {
    base: 'vs',
    inherit: true,
    rules: [
      { token: 'comment', foreground: '8a8670', fontStyle: 'italic' },
      { token: 'keyword', foreground: '0f2740', fontStyle: 'bold' },
      { token: 'number', foreground: '8a4b1f' },
      { token: 'string', foreground: '2e7355' },
      { token: 'attribute.name', foreground: '1f5d8f' },
      { token: 'delimiter.bracket', foreground: '7a6a3a' },
    ],
    colors: {
      'editor.background': '#fbf4df',
      'editor.foreground': '#0b1a2b',
      'editorLineNumber.foreground': '#b3a679',
      'editorLineNumber.activeForeground': '#0f2740',
      'editor.lineHighlightBackground': '#f3e8cb',
      'editor.selectionBackground': '#d9c88d99',
      'editorCursor.foreground': '#0f2740',
      'editorGutter.background': '#fbf4df',
    },
  });
}
