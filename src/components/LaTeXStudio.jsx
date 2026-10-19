import Editor from '@monaco-editor/react';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ArrowLeft, BookOpen, CheckCircle2, ChevronRight, Code2, Download,
  FileCode2, FileImage, FilePlus2, FileText, Image as ImageIcon,
  LoaderCircle, PanelLeftOpen, Play, Plus, Save, Trash2, UploadCloud, X, XCircle,
} from 'lucide-react';
import { api, downloadFile, fileUrl, safeName, upload } from '../lib/backend';
import { registerLatexCompletions, setLatexCompletionContext } from '../lib/latex-completions';
import { defineDomeThemes } from '../lib/monaco-setup'; // also wires the bundled editor
import { useConfirm } from './Confirm';
import PdfPreview from './PdfPreview';

const MonacoEditor = Editor;

const TEXT_EXTENSIONS = new Set(['.tex', '.bib', '.sty', '.cls', '.bst', '.def', '.cfg', '.tikz', '.pgf', '.csv', '.dat', '.txt']);
const FILE_ACCEPT = '.tex,.bib,.sty,.cls,.bst,.def,.cfg,.tikz,.pgf,.csv,.dat,.txt,.png,.jpg,.jpeg,.webp,.svg,.eps,.pdf';

function extensionOf(path = '') {
  const index = path.lastIndexOf('.');
  return index >= 0 ? path.slice(index).toLowerCase() : '';
}

function formatFileSize(bytes = 0) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function iconForFile(file) {
  if (file?.kind === 'image') return FileImage;
  if (file?.extension === '.tex' || file?.extension === '.bib' || file?.kind === 'text') return FileCode2;
  return FileText;
}

function fileQuery(path) {
  return `?path=${encodeURIComponent(path)}`;
}

async function makePdfLatexCompatibleAsset(file) {
  const extension = extensionOf(file.name);
  // pdfLaTeX cannot consume SVG/WebP directly. Convert these browser-renderable
  // formats to PNG during upload so the inserted includegraphics command works.
  if (!['.svg', '.webp'].includes(extension)) return file;
  const objectUrl = URL.createObjectURL(file);
  try {
    const image = new Image();
    image.decoding = 'async';
    image.src = objectUrl;
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = () => reject(new Error(`Could not read ${file.name} for PNG conversion.`));
    });
    const width = image.naturalWidth || 1600;
    const height = image.naturalHeight || 900;
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Your browser could not prepare this image for LaTeX.');
    context.drawImage(image, 0, 0, width, height);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
    if (!blob) throw new Error(`Could not convert ${file.name} to PNG.`);
    const newName = file.name.replace(/\.(svg|webp)$/i, '.png');
    return new File([blob], newName, { type: 'image/png', lastModified: Date.now() });
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
}

function registerResearchLanguages(monaco) {
  defineDomeThemes(monaco);
  registerLatexCompletions(monaco);
  const known = new Set(monaco.languages.getLanguages().map((language) => language.id));
  if (!known.has('latex')) monaco.languages.register({ id: 'latex' });
  monaco.languages.setMonarchTokensProvider('latex', {
    tokenizer: { root: [
      [/%.*$/, 'comment'],
      [/\\[a-zA-Z@]+/, 'keyword'],
      [/\\./, 'keyword'],
      [/[{}]/, 'delimiter.bracket'],
      [/\[[^\]]*\]/, 'delimiter'],
      [/\b\d+(?:\.\d+)?\b/, 'number'],
    ] },
  });
  if (!known.has('bibtex')) monaco.languages.register({ id: 'bibtex' });
  monaco.languages.setMonarchTokensProvider('bibtex', {
    tokenizer: { root: [
      [/%.*$/, 'comment'],
      [/@[a-zA-Z]+/, 'keyword'],
      [/\b[a-zA-Z_][\w-]*(?=\s*=)/, 'attribute.name'],
      [/"[^"\n]*"/, 'string'],
      [/[{}]/, 'delimiter.bracket'],
      [/\b\d{4}\b/, 'number'],
    ] },
  });
}

function stripLatexComment(line) {
  let slashes = 0;
  for (let index = 0; index < line.length; index++) {
    const char = line[index];
    if (char === '%' && slashes % 2 === 0) return line.slice(0, index);
    slashes = char === '\\' ? slashes + 1 : 0;
  }
  return line;
}

function getLiveLatexDiagnostics(source, monaco) {
  const markers = [];
  const braceStack = [];
  const environmentStack = [];
  const error = monaco.MarkerSeverity.Error;
  const warning = monaco.MarkerSeverity.Warning;
  source.split(/\r?\n/).forEach((raw, index) => {
    const line = stripLatexComment(raw);
    let escaped = false;
    for (let column = 0; column < line.length; column++) {
      const char = line[column];
      if (escaped) { escaped = false; continue; }
      if (char === '\\') { escaped = true; continue; }
      if (char === '{') braceStack.push({ line: index + 1, column: column + 1 });
      else if (char === '}') {
        if (braceStack.length) braceStack.pop();
        else markers.push({ startLineNumber: index + 1, startColumn: column + 1, endLineNumber: index + 1, endColumn: column + 2, message: 'Unmatched closing brace `}`.', severity: error });
      }
    }
    const envPattern = /\\(begin|end)\s*\{([^}]+)\}/g;
    let match;
    while ((match = envPattern.exec(line)) !== null) {
      const type = match[1];
      const name = match[2].trim();
      const column = match.index + 1;
      if (type === 'begin') environmentStack.push({ name, line: index + 1, column });
      else if (!environmentStack.length) {
        markers.push({ startLineNumber: index + 1, startColumn: column, endLineNumber: index + 1, endColumn: column + match[0].length, message: `Environment \\end{${name}} has no matching \\begin{${name}}.`, severity: error });
      } else {
        const last = environmentStack[environmentStack.length - 1];
        if (last.name !== name) markers.push({ startLineNumber: index + 1, startColumn: column, endLineNumber: index + 1, endColumn: column + match[0].length, message: `Expected \\end{${last.name}} before \\end{${name}}.`, severity: error });
        else environmentStack.pop();
      }
    }
  });
  braceStack.forEach((brace) => markers.push({ startLineNumber: brace.line, startColumn: brace.column, endLineNumber: brace.line, endColumn: brace.column + 1, message: 'Opening brace `{` is not closed.', severity: warning }));
  environmentStack.forEach((env) => markers.push({ startLineNumber: env.line, startColumn: env.column, endLineNumber: env.line, endColumn: env.column + 1, message: `Environment \\begin{${env.name}} is not closed.`, severity: warning }));
  return markers;
}

function getBuildLogDiagnostics(log, activePath, monaco) {
  if (!log) return [];
  const target = activePath.replace(/\\/g, '/').toLowerCase();
  const baseName = target.split('/').pop();
  const markers = [];
  let pendingError = '';
  for (const line of log.split(/\r?\n/)) {
    const location = line.match(/(?:^|[ (])([\w./\\-]+\.tex):(\d+):\s*(.+)$/i);
    if (location) {
      const file = location[1].replace(/\\/g, '/').toLowerCase();
      if (file.endsWith(target) || file.split('/').pop() === baseName) {
        markers.push({ startLineNumber: Math.max(1, Number(location[2])), startColumn: 1, endLineNumber: Math.max(1, Number(location[2])), endColumn: 2, message: location[3].trim(), severity: monaco.MarkerSeverity.Error, source: 'pdflatex' });
      }
    }
    if (line.trim().startsWith('! ')) pendingError = line.trim().slice(2);
    const lineNumber = line.match(/^\s*l\.(\d+)\s*(.*)$/);
    if (lineNumber && pendingError && (target === 'main.tex' || target.endsWith('/main.tex'))) {
      const n = Math.max(1, Number(lineNumber[1]));
      markers.push({ startLineNumber: n, startColumn: 1, endLineNumber: n, endColumn: 2, message: pendingError + (lineNumber[2] ? ` — ${lineNumber[2].trim()}` : ''), severity: monaco.MarkerSeverity.Error, source: 'pdflatex' });
      pendingError = '';
    }
  }
  return markers;
}

export default function LaTeXStudio({ projectId, projectName = 'Research project', documents = [], onDocumentsChanged, onBack }) {
  const confirm = useConfirm();
  const [selectedId, setSelectedId] = useState(null);
  const [documentData, setDocumentData] = useState(null);
  const [files, setFiles] = useState([]);
  const [activePath, setActivePath] = useState('main.tex');
  const [activeFile, setActiveFile] = useState(null);
  const [source, setSource] = useState('');
  const [dirty, setDirty] = useState(false);
  const [loadingDoc, setLoadingDoc] = useState(false);
  const [saving, setSaving] = useState(false);
  const [compiling, setCompiling] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [compileResult, setCompileResult] = useState(null);
  const [previewVersion, setPreviewVersion] = useState(0);
  const [assetVersion, setAssetVersion] = useState(0);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [deletingId, setDeletingId] = useState(null);
  const [deletingPath, setDeletingPath] = useState('');
  const deletedDocumentIdsRef = useRef(new Set());
  const [showLog, setShowLog] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [showNewDoc, setShowNewDoc] = useState(false);
  const [newFilePath, setNewFilePath] = useState('references.bib');
  const [showNewFile, setShowNewFile] = useState(false);
  const [assetFolder, setAssetFolder] = useState('');
  const fileInputRef = useRef(null);
  const editorRef = useRef(null);
  const monacoRef = useRef(null);
  const decorationIdsRef = useRef([]);
  const [editorTheme, setEditorTheme] = useState('dome-dark');

  // Keep the code editor in step with the app's light/dark theme.
  useEffect(() => {
    const root = document.documentElement;
    const sync = () => setEditorTheme(root.dataset.theme === 'light' ? 'dome-light' : 'dome-dark');
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(root, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);


  // Feed the offline autocomplete (\cite, \includegraphics, \ref, \input) with this workspace's files.
  const completionVersionRef = useRef(0);
  const activePathRef = useRef(activePath);
  activePathRef.current = activePath;
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  useEffect(() => {
    if (!selectedId) { setLatexCompletionContext(null); return undefined; }
    completionVersionRef.current += 1;
    setLatexCompletionContext({
      documentId: selectedId,
      files,
      version: completionVersionRef.current,
      readFile: async (path) => (await api(`/latex-documents/${selectedIdRef.current}/file${fileQuery(path)}`)).content || '',
      getLiveSource: () => ({ path: activePathRef.current, text: editorRef.current?.getValue() ?? '' }),
    });
    return undefined;
  }, [selectedId, files, saving, assetVersion]);
  useEffect(() => () => setLatexCompletionContext(null), []);

  const selectedListDoc = documents.find((doc) => String(doc.id) === String(selectedId));
  const activeExtension = extensionOf(activePath);
  const activeIsText = TEXT_EXTENSIONS.has(activeExtension);
  const activeIsImage = activeFile?.kind === 'image';
  const activeIsPdfAsset = activeFile?.kind === 'pdf';
  const hasCompiledPdf = Boolean(compileResult?.success || documentData?.compiled);

  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    const model = editor?.getModel();
    if (!editor || !monaco || !model || !activeIsText) return;
    const sourceExt = ['.tex', '.tikz', '.pgf', '.sty', '.cls', '.def'].includes(activeExtension);
    const liveMarkers = sourceExt ? getLiveLatexDiagnostics(source, monaco) : [];
    const buildMarkers = !dirty && compileResult?.success === false ? getBuildLogDiagnostics(compileResult.log || '', activePath, monaco) : [];
    const markers = [...liveMarkers, ...buildMarkers];
    monaco.editor.setModelMarkers(model, 'research-vault-latex', markers);
    const decorations = markers.map((marker) => ({
      range: new monaco.Range(marker.startLineNumber, 1, marker.startLineNumber, 1),
      options: { isWholeLine: true, className: marker.severity === monaco.MarkerSeverity.Error ? 'latex-diagnostic-line' : 'latex-diagnostic-warning-line', glyphMarginClassName: 'latex-diagnostic-glyph', hoverMessage: { value: marker.message } },
    }));
    decorationIdsRef.current = editor.deltaDecorations(decorationIdsRef.current, decorations);
  }, [source, activePath, activeExtension, activeIsText, dirty, compileResult]);

  function handleEditorMount(editor, monaco) {
    editorRef.current = editor;
    monacoRef.current = monaco;
    registerResearchLanguages(monaco);
    editor.updateOptions({ glyphMargin: true });
  }


  const refreshFiles = useCallback(async (id = selectedId) => {
    if (!id) return [];
    const result = await api(`/latex-documents/${id}/files`);
    const nextFiles = result.files || [];
    setFiles(nextFiles);
    return nextFiles;
  }, [selectedId]);

  const readWorkspaceFile = useCallback(async (id, path, fileList) => {
    const list = fileList || files;
    const meta = list.find((file) => file.path === path) || null;
    setActivePath(path);
    setActiveFile(meta);
    setDirty(false);
    if (meta?.kind === 'text' || TEXT_EXTENSIONS.has(extensionOf(path))) {
      const response = await api(`/latex-documents/${id}/file${fileQuery(path)}`);
      setSource(response.content || '');
    } else {
      setSource('');
    }
  }, [files]);

  const loadWorkspace = useCallback(async (id, preferredPath = 'main.tex') => {
    setLoadingDoc(true);
    setError('');
    try {
      const [doc, fileResult] = await Promise.all([
        api(`/latex-documents/${id}`),
        api(`/latex-documents/${id}/files`),
      ]);
      const nextFiles = fileResult.files || [];
      setDocumentData(doc);
      setFiles(nextFiles);
      const firstText = nextFiles.find((file) => file.path === 'main.tex')
        || nextFiles.find((file) => file.kind === 'text')
        || nextFiles[0]
        || null;
      const nextPath = nextFiles.some((file) => file.path === preferredPath)
        ? preferredPath
        : (firstText?.path || 'main.tex');
      const nextMeta = nextFiles.find((file) => file.path === nextPath) || null;
      setActivePath(nextPath);
      setActiveFile(nextMeta);
      setDirty(false);
      if (nextMeta?.kind === 'text' || TEXT_EXTENSIONS.has(extensionOf(nextPath))) {
        const sourceFile = await api(`/latex-documents/${id}/file${fileQuery(nextPath)}`);
        setSource(sourceFile.content || '');
      } else {
        setSource('');
      }
      setCompileResult(doc.compiled ? { success: true, compiled: true, log: doc.last_compile_log || '' } : null);
      setShowLog(false);
      setPreviewVersion((version) => version + 1);
    } catch (e) {
      setError(e.message || 'Could not load this LaTeX workspace.');
    } finally {
      setLoadingDoc(false);
    }
  }, []);

  useEffect(() => {
    const availableDocuments = documents.filter((doc) => !deletedDocumentIdsRef.current.has(String(doc.id)));
    if (!selectedId && availableDocuments.length) {
      setSelectedId(availableDocuments[0].id);
    } else if (selectedId && !availableDocuments.some((doc) => String(doc.id) === String(selectedId))) {
      setSelectedId(availableDocuments[0]?.id || null);
    }
  }, [documents, selectedId]);

  useEffect(() => {
    if (selectedId) loadWorkspace(selectedId, 'main.tex');
    else {
      setDocumentData(null);
      setFiles([]);
      setActivePath('main.tex');
      setActiveFile(null);
      setSource('');
      setCompileResult(null);
    }
  }, [selectedId, loadWorkspace]);

  async function createDocument(event) {
    event.preventDefault();
    if (!newTitle.trim()) return;
    setError('');
    try {
      const data = await api(`/projects/${projectId}/latex-documents`, {
        method: 'POST',
        body: JSON.stringify({ title: newTitle.trim(), source: '' }),
      });
      setNewTitle('');
      setShowNewDoc(false);
      if (onDocumentsChanged) await onDocumentsChanged();
      setSelectedId(data.id);
    } catch (e) {
      setError(e.message);
    }
  }

  async function saveCurrentFile({ quiet = false } = {}) {
    if (!selectedId || !activeIsText) return true;
    setSaving(true);
    if (!quiet) setError('');
    try {
      await api(`/latex-documents/${selectedId}/file${fileQuery(activePath)}`, {
        method: 'PUT',
        body: JSON.stringify({ content: source }),
      });
      setDirty(false);
      const nextFiles = await refreshFiles(selectedId);
      setActiveFile(nextFiles.find((file) => file.path === activePath) || activeFile);
      setAssetVersion((version) => version + 1);
      return true;
    } catch (e) {
      if (!quiet) setError(e.message || 'Could not save this file.');
      return false;
    } finally {
      setSaving(false);
    }
  }

  async function switchToFile(path) {
    if (path === activePath || !selectedId) return;
    if (dirty) {
      const saved = await saveCurrentFile({ quiet: true });
      if (!saved) {
        setError('The current file could not be saved, so it was not switched.');
        return;
      }
    }
    try {
      setError('');
      await readWorkspaceFile(selectedId, path, files);
    } catch (e) {
      setError(e.message || 'Could not open this file.');
    }
  }

  async function switchWorkspace(doc) {
    if (String(doc.id) === String(selectedId)) return;
    if (dirty) {
      const saved = await saveCurrentFile({ quiet: true });
      if (!saved) {
        setError('The current file could not be saved, so the workspace was not switched.');
        return;
      }
    }
    setSidebarOpen(false);
    setSelectedId(doc.id);
  }

  async function createProjectFile(event) {
    event.preventDefault();
    if (!selectedId) return;
    const relativePath = newFilePath.trim().replace(/\\/g, '/');
    const extension = extensionOf(relativePath);
    if (!relativePath || relativePath.startsWith('/') || relativePath.split('/').some((part) => !part || part === '.' || part === '..')) {
      setError('Enter a relative path such as references.bib or sections/methods.tex.');
      return;
    }
    if (!TEXT_EXTENSIONS.has(extension)) {
      setError('For editable files, use .tex, .bib, .tikz, .pgf, .csv, .dat, .txt, .sty, .cls, .bst, .def, or .cfg. Use Upload asset for images and PDFs.');
      return;
    }
    if (files.some((file) => file.path.toLowerCase() === relativePath.toLowerCase())) {
      setError(`The file ${relativePath} already exists. Open it from the file list instead.`);
      return;
    }
    try {
      setError('');
      await api(`/latex-documents/${selectedId}/file${fileQuery(relativePath)}`, {
        method: 'PUT',
        body: JSON.stringify({ content: '' }),
      });
      const nextFiles = await refreshFiles(selectedId);
      setShowNewFile(false);
      setNewFilePath('references.bib');
      await readWorkspaceFile(selectedId, relativePath, nextFiles);
    } catch (e) {
      setError(e.message || 'Could not create the file.');
    }
  }

  async function deleteWorkspaceDocument(doc) {
    if (!doc?.id) return;
    const currentlyOpen = String(doc.id) === String(selectedId);
    const unsavedWarning = currentlyOpen && dirty ? ' The current file has unsaved changes, which will be discarded.' : '';
    if (!(await confirm(`Delete the LaTeX workspace “${doc.title}” and all its source files, assets, and compiled PDFs? This cannot be undone.${unsavedWarning}`, { title: 'Delete workspace?', confirmLabel: 'Delete workspace' }))) return;
    setDeletingId(doc.id); setError(''); setNotice('');
    try {
      await api(`/latex-documents/${doc.id}`, { method: 'DELETE' });
      deletedDocumentIdsRef.current.add(String(doc.id));
      const refreshed = onDocumentsChanged ? await onDocumentsChanged() : null;
      const remaining = (Array.isArray(refreshed) ? refreshed : documents).filter((item) => !deletedDocumentIdsRef.current.has(String(item.id)));
      if (currentlyOpen) setSelectedId(remaining[0]?.id ?? null);
      setNotice(`Deleted “${doc.title}”.`);
    } catch (e) {
      setError(e.message || 'Could not delete this LaTeX workspace.');
    } finally {
      setDeletingId(null);
    }
  }

  async function deleteWorkspaceFile(file) {
    if (!selectedId || !file?.path || file.path.toLowerCase() === 'main.tex') return;
    const unsavedWarning = dirty && activePath === file.path ? ' Unsaved edits to this file will be discarded.' : '';
    if (!(await confirm(`Delete “${file.path}” from this LaTeX workspace?${unsavedWarning}`, { title: 'Delete file?', confirmLabel: 'Delete file' }))) return;
    setDeletingPath(file.path); setError(''); setNotice('');
    try {
      await api(`/latex-documents/${selectedId}/file${fileQuery(file.path)}`, { method: 'DELETE' });
      const nextFiles = await refreshFiles(selectedId);
      if (activePath === file.path) {
        await readWorkspaceFile(selectedId, 'main.tex', nextFiles);
      }
      setNotice(`Deleted ${file.path}.`);
    } catch (e) {
      setError(e.message || 'Could not delete this project file.');
    } finally {
      setDeletingPath('');
    }
  }

  async function importLegacyBibWorkspace(legacyDoc) {
    if (!selectedId || String(legacyDoc.id) === String(selectedId)) return;
    const relativePath = (legacyDoc.title || 'references.bib').replace(/\\/g, '/').split('/').pop();
    if (extensionOf(relativePath) !== '.bib') return;
    if (files.some((file) => file.path.toLowerCase() === relativePath.toLowerCase())) {
      setError(`${relativePath} already exists in this workspace. Open that file from the list instead.`);
      return;
    }
    try {
      setError('');
      const legacy = await api(`/latex-documents/${legacyDoc.id}`);
      const content = legacy.source || '';
      if (!/@[A-Za-z]+\s*\{/.test(content) && !(await confirm(`The old workspace “${legacyDoc.title}” does not look like a BibTeX file. Import its contents into ${relativePath} anyway so you can edit them?`, { title: 'Import anyway?', confirmLabel: 'Import', tone: 'primary' }))) return;
      await api(`/latex-documents/${selectedId}/file${fileQuery(relativePath)}`, {
        method: 'PUT',
        body: JSON.stringify({ content }),
      });
      const nextFiles = await refreshFiles(selectedId);
      await readWorkspaceFile(selectedId, relativePath, nextFiles);
      setError('');
    } catch (e) {
      setError(e.message || 'Could not import the old BibTeX workspace.');
    }
  }

  async function handleAssetUpload(event) {
    const chosen = Array.from(event.target.files || []);
    event.target.value = '';
    if (!selectedId || !chosen.length) return;
    setUploading(true);
    setError('');
    try {
      let lastUploadedPath = null;
      for (const inputFile of chosen) {
        const file = await makePdfLatexCompatibleAsset(inputFile);
        const folder = assetFolder.trim().replace(/\\/g, '/').replace(/^\/+|\/+$/g, '');
        const path = folder ? `${folder}/${file.name}` : file.name;
        const formData = new FormData();
        formData.append('file', file);
        formData.append('path', path);
        await upload(`/latex-documents/${selectedId}/files`, formData);
        lastUploadedPath = path;
      }
      const nextFiles = await refreshFiles(selectedId);
      if (lastUploadedPath) await readWorkspaceFile(selectedId, lastUploadedPath, nextFiles);
      setAssetVersion((version) => version + 1);
    } catch (e) {
      setError(e.message || 'Could not upload the asset. PNG/JPG/PDF, SVG/WebP (converted to PNG), CSV graph data, and LaTeX source files are supported.');
    } finally {
      setUploading(false);
    }
  }

  async function buildPdf() {
    if (!selectedId || compiling || saving) return;
    setError('');
    setShowLog(false);
    if (dirty && activeIsText) {
      const saved = await saveCurrentFile({ quiet: false });
      if (!saved) return;
    }
    setCompiling(true);
    setCompileResult(null);
    try {
      const result = await api(`/latex-documents/${selectedId}/compile`, { method: 'POST' });
      setCompileResult(result);
      setDocumentData((previous) => previous ? { ...previous, compiled: Boolean(result.success && result.compiled), last_compile_log: result.log || '' } : previous);
      setShowLog(!result.success);
      if (result.success && result.compiled) {
        setPreviewVersion((version) => version + 1);
        setError('');
      } else {
        setError('PDF build did not complete. Open Build log for the LaTeX or bibliography error.');
      }
      if (onDocumentsChanged) await onDocumentsChanged();
    } catch (e) {
      setError(e.message || 'Could not compile the LaTeX source.');
    } finally {
      setCompiling(false);
    }
  }

  async function insertSnippetIntoMain(snippet, packages = [], preambleCommands = []) {
    if (!selectedId) return;
    try {
      setError('');
      if (dirty) {
        const saved = await saveCurrentFile({ quiet: true });
        if (!saved) throw new Error('Save the current file before inserting the snippet.');
      }
      const response = await api(`/latex-documents/${selectedId}/file${fileQuery('main.tex')}`);
      let current = response.content || '';
      for (const packageName of packages) {
        const safePackage = packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const packageRegex = new RegExp(`\\\\usepackage(?:\\[[^\\]]*\\])?\\{[^}]*\\b${safePackage}\\b[^}]*\\}`);
        if (!packageRegex.test(current)) {
          const classLine = current.match(/\\documentclass(?:\[[^\]]*\])?\{[^}]+\}[^\r\n]*/);
          const packageLine = `\\usepackage{${packageName}}`;
          current = classLine ? current.replace(classLine[0], `${classLine[0]}\n${packageLine}`) : `${packageLine}\n${current}`;
        }
      }
      for (const command of preambleCommands) {
        const commandName = command.match(/^\\[a-zA-Z@]+/)?.[0];
        if (!commandName || !current.includes(commandName)) {
          const packageLine = current.match(/\\usepackage(?:\[[^\]]*\])?\{pgfplots\}[^\r\n]*/);
          if (packageLine) current = current.replace(packageLine[0], `${packageLine[0]}\n${command}`);
          else {
            const classLine = current.match(/\\documentclass(?:\[[^\]]*\])?\{[^}]+\}[^\r\n]*/);
            current = classLine ? current.replace(classLine[0], `${classLine[0]}\n${command}`) : `${command}\n${current}`;
          }
        }
      }
      const nextSource = /\\end\{document\}/.test(current)
        ? current.replace(/\\end\{document\}/, `${snippet}\n\n\\end{document}`)
        : `${current}\n\n${snippet}\n`;
      const nextFiles = await refreshFiles(selectedId);
      setActivePath('main.tex');
      setActiveFile(nextFiles.find((file) => file.path === 'main.tex') || { path: 'main.tex', name: 'main.tex', extension: '.tex', kind: 'text' });
      setSource(nextSource);
      setDirty(true);
      setSidebarOpen(false);
    } catch (e) {
      setError(e.message || 'Could not insert the snippet into main.tex.');
    }
  }

  async function insertAssetSnippet() {
    const escaped = activePath.replace(/\\/g, '/').replace(/[{}]/g, '');
    const snippet = `\n\\begin{figure}[htbp]\n  \\centering\n  \\includegraphics[width=0.9\\linewidth]{${escaped}}\n  \\caption{Add a caption for this figure.}\n  \\label{fig:${activePath.split('/').pop().replace(/\.[^.]+$/, '').replace(/[^A-Za-z0-9:.-]+/g, '-')}}\n\\end{figure}`;
    await insertSnippetIntoMain(snippet);
  }

  async function insertTikzDiagram() {
    const snippet = `
\\begin{figure}[htbp]
  \\centering
  \\begin{tikzpicture}
    \\node[draw, rounded corners, fill=blue!8, minimum width=3cm, minimum height=0.9cm] (input) at (0,0) {Input};
    \\node[draw, rounded corners, fill=green!8, minimum width=3cm, minimum height=0.9cm] (process) at (4.3,0) {Processing};
    \\draw[->, thick] (input) -- (process);
  \\end{tikzpicture}
  \\caption{Add a caption for this diagram.}
  \\label{fig:tikz-diagram}
\\end{figure}`;
    await insertSnippetIntoMain(snippet, ['tikz']);
  }

  async function insertPgfplotsGraph(dataPath = null) {
    const plot = dataPath
      ? `\\addplot table [x index=0, y index=1, col sep=comma] {${dataPath.replace(/\\/g, '/').replace(/[{}]/g, '')}};`
      : `\\addplot coordinates {(0, 1) (1, 3) (2, 2) (3, 4)};`;
    const snippet = `
\\begin{figure}[htbp]
  \\centering
  \\begin{tikzpicture}
    \\begin{axis}[xlabel={X axis}, ylabel={Y axis}, grid=major, width=0.85\\linewidth]
      ${plot}
    \\end{axis}
  \\end{tikzpicture}
  \\caption{Add a caption for this graph.}
  \\label{fig:pgfplots-graph}
\\end{figure}`;
    await insertSnippetIntoMain(snippet, ['pgfplots'], ['\\pgfplotsset{compat=1.18}']);
  }

  async function insertBibCommands() {
    const bibPath = activePath.replace(/\\/g, '/').replace(/\.bib$/i, '').replace(/[{}]/g, '');
    const snippet = `\n\\bibliographystyle{IEEEtran}\n\\bibliography{${bibPath}}`;
    await insertSnippetIntoMain(snippet);
  }

  const saveShortcutAction = useCallback(() => {
    if (!selectedId) return;
    // Ctrl/Cmd+S builds from main.tex to refresh the preview; saving a .bib or
    // included .tex file only saves that file and never tries to compile it.
    if (activePath === 'main.tex') buildPdf();
    else saveCurrentFile();
  // The keyboard listener should always use the latest editor state.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId, activePath, source, dirty, activeIsText, saving, compiling]);

  useEffect(() => {
    const handleKey = (event) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        saveShortcutAction();
      }
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'b') {
        event.preventDefault();
        buildPdf();
      }
      if (event.key === 'Escape') setSidebarOpen(false);
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [saveShortcutAction]);

  const activeAssetUrl = selectedId ? fileUrl(`/latex-documents/${selectedId}/asset${fileQuery(activePath)}&v=${assetVersion}`) : '';
  const previewUrl = selectedId ? fileUrl(`/latex-documents/${selectedId}/pdf?v=${previewVersion}`) : '';

  async function saveLatexFile(kind) {
    if (!selectedId) return;
    setError(''); setNotice('');
    const title = safeName(documentData?.title || selectedListDoc?.title || 'document');
    try {
      const saved = kind === 'source'
        ? await downloadFile(`/latex-documents/${selectedId}/source`, `${title}.tex`)
        : await downloadFile(`/latex-documents/${selectedId}/download`, `${title}.pdf`);
      if (saved) setNotice(`Saved to ${saved}`);
    } catch (e) { setError(e.message || 'Could not save the file.'); }
  }

  return (
    <div className="latex-studio-shell">
      <header className="latex-studio-topbar">
        <div className="latex-topbar-left">
          <button className="latex-files-toggle" title="Open project files" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={19} /><span>Files</span></button>
          <span className="latex-topbar-divider" />
          <div className="latex-crumb"><span>{projectName}</span><ChevronRight size={14} /><b>{documentData?.title || selectedListDoc?.title || 'LaTeX Studio'}</b><ChevronRight size={14} /><small>{activePath}</small></div>
        </div>
        <div className="latex-topbar-actions">
          <span className="latex-saving-status">{saving ? 'Saving…' : compiling ? 'Building PDF…' : dirty ? 'Unsaved changes' : 'All changes saved'}</span>
          <button className="button button-muted" onClick={() => saveCurrentFile()} disabled={!activeIsText || saving || !dirty}><Save size={15} /> Save file</button>
          <button className="button button-primary" onClick={buildPdf} disabled={!selectedId || saving || compiling}><Play size={15} /> {compiling ? 'Building…' : 'Build PDF'}</button>
          {hasCompiledPdf && <button className="button button-ghost" onClick={() => saveLatexFile('pdf')}><Download size={15} /> Save PDF</button>}
          {selectedId && <button className="button button-ghost" onClick={() => saveLatexFile('source')} title="Save main.tex"><FileCode2 size={15} /> Save .tex</button>}
          <button className="latex-back-button" onClick={onBack}><ArrowLeft size={15} /> Back to project</button>
        </div>
      </header>

      <div className="latex-studio">
        {!selectedId || !selectedListDoc ? <div className="workspace-empty"><div className="empty-icon"><Code2 size={28} /></div><h3>Your writing space</h3><p>Create a LaTeX workspace to edit <code>main.tex</code>, manage BibTeX references and add image assets.</p><button className="button button-primary" onClick={() => { setSidebarOpen(true); setShowNewDoc(true); }}><FilePlus2 size={15} /> Create LaTeX workspace</button></div> : <>
          <section className="latex-active-workspace">
            <div className="latex-file-context-bar">
              <div className="latex-active-file-label">{activeFile?.kind === 'image' ? <ImageIcon size={16} /> : <FileCode2 size={16} />}<b>{activePath}</b>{dirty && <span className="latex-unsaved-dot" title="Unsaved changes" />}</div>
              <div className="latex-context-actions">
                {activeIsImage || activeIsPdfAsset ? <button className="button button-muted button-sm" onClick={insertAssetSnippet}><Code2 size={14} /> Insert into main.tex</button> : null}
                {activeExtension === '.bib' && <button className="button button-muted button-sm" onClick={insertBibCommands}><Code2 size={14} /> Add bibliography commands</button>}
                {['.tex', '.tikz', '.pgf'].includes(activeExtension) && <button className="button button-muted button-sm" onClick={insertTikzDiagram}><Code2 size={14} /> Insert TikZ diagram</button>}
                {['.tex', '.tikz', '.pgf'].includes(activeExtension) && <button className="button button-muted button-sm" onClick={() => insertPgfplotsGraph()}><Code2 size={14} /> Insert PGFPlots graph</button>}
                {['.csv', '.dat'].includes(activeExtension) && <button className="button button-muted button-sm" onClick={() => insertPgfplotsGraph(activePath)}><Code2 size={14} /> Plot this data</button>}
                {activeIsText && <span className="latex-language-badge">{activeExtension === '.bib' ? 'BibTeX' : ['.csv', '.dat'].includes(activeExtension) ? 'Graph data' : 'LaTeX source'}</span>}
              </div>
            </div>

            {error && <div className="latex-error"><XCircle size={16} /> <span>{error}</span><button className="text-button" onClick={() => setError('')}>Dismiss</button></div>}
            {notice && <div className="latex-notice"><CheckCircle2 size={16} /> <span>{notice}</span><button className="text-button" onClick={() => setNotice('')}>Dismiss</button></div>}

            <div className="latex-split-view">
              <div className="source-pane">
                <div className="pane-topline"><span><span className="pane-dot blue-dot" /> {activePath}</span><span>{activeIsText ? (activeExtension === '.bib' ? 'BibTeX' : 'Source') : activeFile?.kind === 'image' ? 'Image asset' : activeFile?.kind === 'pdf' ? 'PDF asset' : 'File'}</span></div>
                {loadingDoc ? <div className="editor-loading"><LoaderCircle className="spin" /> Loading file…</div> : activeIsText ? <>
                  <MonacoEditor
                    key={`${selectedId}:${activePath}`}
                    height="100%"
                    language={activeExtension === '.bib' ? 'bibtex' : ['.csv', '.dat', '.txt', '.cfg', '.def', '.bst'].includes(activeExtension) ? 'plaintext' : 'latex'}
                    theme={editorTheme}
                    beforeMount={registerResearchLanguages}
                    loading={<div className="editor-loading"><LoaderCircle className="spin" /> Loading code editor…</div>}
                    onMount={handleEditorMount}
                    value={source}
                    onChange={(value) => { setSource(value ?? ''); setDirty(true); }}
                    options={{ minimap: { enabled: false }, glyphMargin: true, fontSize: 15, fontFamily: "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", lineNumbers: 'on', scrollBeyondLastLine: false, wordWrap: 'on', automaticLayout: true, tabSize: 2, padding: { top: 18, bottom: 18 }, renderLineHighlight: 'line', cursorBlinking: 'smooth', suggestOnTriggerCharacters: true }}
                  />
                  {activeExtension === '.bib' && <div className="latex-file-tip"><BookOpen size={15} /><div><b>This is a bibliography file</b><span>Save it here; do not compile the .bib file directly. Then choose Build PDF to compile <code>main.tex</code> and resolve citations using BibTeX or Biber.</span></div></div>}
                  {activeExtension === '.tex' && activePath !== 'main.tex' && <div className="latex-file-tip"><FileCode2 size={15} /><div><b>Included source file</b><span>Include it from <code>main.tex</code> with <code>\input&#123;{activePath.replace(/\.tex$/i, '')}&#125;</code> or <code>\include&#123;…&#125;</code>.</span></div></div>}
                </> : activeIsImage ? <div className="latex-asset-preview"><img src={activeAssetUrl} alt={activePath} /><div><b>{activePath}</b><span>{formatFileSize(activeFile?.size)} · Asset stored with this LaTeX workspace</span><p>Use Insert into main.tex to add a figure environment at the end of your document.</p>{['.svg', '.eps', '.webp'].includes(activeExtension) && <small className="latex-format-note">For reliable pdfLaTeX builds, convert this asset to PNG, JPG, or PDF first. SVG, EPS, and WebP can require external conversion.</small>}</div></div> : activeIsPdfAsset ? <div className="latex-asset-pdf"><PdfPreview url={activeAssetUrl} /></div> : <div className="latex-asset-preview"><FileText size={42} /><div><b>{activePath}</b><span>{formatFileSize(activeFile?.size)}</span></div></div>}
              </div>

              <div className="preview-pane">
                <div className="pane-topline"><span><span className={`pane-dot ${hasCompiledPdf ? 'green-dot' : 'gray-dot'}`} /> PDF preview — main.tex</span><div className="preview-tools">{compileResult?.success && <span className="build-success"><CheckCircle2 size={14} /> Build successful</span>}<button className="text-button" onClick={() => setShowLog((value) => !value)}>{showLog ? 'Hide log' : 'Build log'}</button></div></div>
                {hasCompiledPdf ? <div className="latex-preview-frame"><PdfPreview key={`${selectedId}:${previewVersion}`} url={previewUrl} /></div> : <div className="preview-placeholder"><div className="preview-paper-icon"><FileText size={36} /></div><h3>PDF preview will appear here</h3><p>Save your source or bibliography, then build <code>main.tex</code>. The compiler runs BibTeX/Biber passes when the document requests a bibliography.</p><button className="button button-primary" onClick={buildPdf} disabled={saving || compiling}><Play size={15} /> Build PDF</button></div>}
                {showLog && <div className="build-log-panel"><div><b>Compiler output</b><button className="text-button" onClick={() => setShowLog(false)}>Close</button></div><pre>{compileResult?.log || documentData?.last_compile_log || 'No build has run yet. If citations show [?], confirm the citation key, bibliography filename, and whether BibTeX/Biber completed successfully.'}</pre></div>}
              </div>
            </div>

            <div className="latex-statusbar"><span><span className={`status-indicator ${hasCompiledPdf ? 'status-green' : ''}`} /> {compiling ? 'Compiling main.tex' : saving ? 'Saving file' : dirty ? 'Unsaved changes' : hasCompiledPdf ? 'PDF preview is available' : 'Ready to edit'}</span><span>{activePath} · UTF-8 · Local compilation</span></div>
          </section>
        </>}
      </div>

      {sidebarOpen && <div className="latex-drawer-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) setSidebarOpen(false); }}>
        <aside className="latex-file-drawer" aria-label="LaTeX project files">
          <div className="latex-drawer-header"><div><div className="panel-eyebrow">WRITING WORKSPACE</div><h2>Project files</h2><p>{projectName}</p></div><button className="icon-button" title="Close files panel" onClick={() => setSidebarOpen(false)}><X size={19} /></button></div>

          <section className="latex-drawer-section">
            <div className="latex-drawer-section-title"><span>LATEX WORKSPACES</span><button title="Create LaTeX workspace" onClick={() => setShowNewDoc((value) => !value)}><Plus size={15} /></button></div>
            {showNewDoc && <form className="latex-drawer-form" onSubmit={createDocument}><label>Workspace name<input autoFocus value={newTitle} onChange={(event) => setNewTitle(event.target.value)} placeholder="e.g. Main research paper" required /></label><div><button className="button button-primary button-sm" type="submit">Create</button><button className="button button-muted button-sm" type="button" onClick={() => setShowNewDoc(false)}>Cancel</button></div></form>}
            {documents.map((doc) => <div key={doc.id} className="latex-workspace-row">
              <button className={`latex-workspace-choice ${String(doc.id) === String(selectedId) ? 'active' : ''}`} onClick={() => switchWorkspace(doc)}><FileText size={16} /><span><b>{doc.title}</b><small>{doc.compiled ? 'Last build available' : 'Workspace'}</small></span>{String(doc.id) === String(selectedId) && <span className="latex-selected-check"><CheckCircle2 size={14} /></span>}</button>
              <button type="button" className="mini-icon-button danger latex-delete-action" title={`Delete workspace ${doc.title}`} aria-label={`Delete workspace ${doc.title}`} disabled={deletingId === doc.id} onClick={() => deleteWorkspaceDocument(doc)}>{deletingId === doc.id ? <LoaderCircle size={14} className="spin" /> : <Trash2 size={14} />}</button>
            </div>)}
            {!documents.length && <p className="latex-drawer-empty">Create a workspace to start writing.</p>}
          </section>

          {selectedId && <section className="latex-drawer-section latex-drawer-file-section">
            <div className="latex-drawer-section-title"><span>FILES IN THIS WORKSPACE</span><button title="Create a .tex or .bib file" onClick={() => { setShowNewFile((value) => !value); setError(''); }}><FilePlus2 size={15} /></button></div>
            {showNewFile && <form className="latex-drawer-form" onSubmit={createProjectFile}><label>Relative filename<input autoFocus value={newFilePath} onChange={(event) => setNewFilePath(event.target.value)} placeholder="references.bib or figures/plot.tex" required /></label><small>Text files are saved locally and never compiled directly.</small><div><button className="button button-primary button-sm" type="submit">Create file</button><button className="button button-muted button-sm" type="button" onClick={() => setShowNewFile(false)}>Cancel</button></div></form>}
            {documents.filter((doc) => String(doc.id) !== String(selectedId) && extensionOf(doc.title || '') === '.bib' && !files.some((file) => file.path.toLowerCase() === (doc.title || '').split('/').pop().toLowerCase())).map((legacyDoc) => <button key={`legacy-${legacyDoc.id}`} className="latex-import-legacy" onClick={() => importLegacyBibWorkspace(legacyDoc)}><FilePlus2 size={14} /><span><b>Import old “{legacyDoc.title}” workspace</b><small>Copy its saved source into a real .bib file here</small></span></button>)}
            <div className="latex-upload-assets-row"><label className="latex-folder-label"><span>Upload folder</span><input value={assetFolder} onChange={(event) => setAssetFolder(event.target.value)} placeholder="figures (optional)" /></label><button className="button button-muted button-sm" onClick={() => fileInputRef.current?.click()} disabled={uploading}><UploadCloud size={14} /> {uploading ? 'Uploading…' : 'Upload asset'}</button><input ref={fileInputRef} hidden type="file" accept={FILE_ACCEPT} multiple onChange={handleAssetUpload} /></div>
            <div className="latex-project-file-list">
              {files.map((file) => { const Icon = iconForFile(file); return <div key={file.path} className="latex-project-file-row">
                <button className={`latex-project-file ${file.path === activePath ? 'active' : ''}`} onClick={() => switchToFile(file.path)}><Icon size={16} /><span><b>{file.name}</b><small>{file.path !== file.name ? file.path : `${file.kind === 'text' ? 'Editable source' : file.kind === 'image' ? 'Figure / image' : 'PDF asset'} · ${formatFileSize(file.size)}`}</small></span></button>
                {file.path.toLowerCase() !== 'main.tex' && <button type="button" className="mini-icon-button danger latex-delete-action" title={`Delete ${file.path}`} aria-label={`Delete ${file.path}`} disabled={deletingPath === file.path} onClick={() => deleteWorkspaceFile(file)}>{deletingPath === file.path ? <LoaderCircle size={14} className="spin" /> : <Trash2 size={14} />}</button>}
              </div>; })}
              {files.length === 0 && <p className="latex-drawer-empty">No source files have been found in this workspace yet.</p>}
            </div>
            <div className="latex-drawer-help"><BookOpen size={15} /><p><b>LaTeX files, figures & bibliography</b>Use PNG/JPG/PDF photos, create TikZ diagrams and PGFPlots graphs, and upload CSV data when needed. Create <code>references.bib</code>, save your BibTeX entries, and use <code>\cite&#123;key&#125;</code> in <code>main.tex</code>. Select the .bib file and choose <em>Add bibliography commands</em>, or add <code>\bibliographystyle&#123;IEEEtran&#125;</code> and <code>\bibliography&#123;references&#125;</code> before <code>\end&#123;document&#125;</code>. Then Build PDF.</p></div>
          </section>}
        </aside>
      </div>}
    </div>
  );
}
