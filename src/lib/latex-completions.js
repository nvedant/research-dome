// Offline autocomplete for the LaTeX editor.
//
// Everything is read from the files in the current LaTeX workspace (through the
// local backend), so suggestions work without an internet connection:
//   \cite{…} \citep{…} \citet{…} …   -> keys from every .bib file (with author/title/year)
//   \includegraphics{…}              -> image and PDF assets in the workspace
//   \ref{…} \eqref{…} \cref{…} …     -> \label{…} names found in the .tex files
//   \input{…} \include{…}            -> .tex files
//   \bibliography{…}                 -> .bib files (without extension)
//   \addbibresource{…}               -> .bib files (with extension)

const CITE_COMMANDS = /^(?:[a-zA-Z]*cite[a-zA-Z]*|nocite|parencite|textcite|autocite|footcite|smartcite|supercite|fullcite|footfullcite)$/;
const REF_COMMANDS = /^(?:ref|eqref|pageref|autoref|nameref|cref|Cref|vref|Vref|labelcref|cpageref)$/;
const GRAPHICS_COMMANDS = /^(?:includegraphics|includepdf)$/;
const INPUT_COMMANDS = /^(?:input|include|subfile|subimport)$/;
const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.pdf', '.svg', '.eps', '.webp']);
const TEX_EXTENSIONS = new Set(['.tex', '.tikz', '.pgf']);

let context = null; // { documentId, files, version, readFile(path) -> Promise<string>, getLiveSource() -> {path, text} }
let registered = false;
let cache = new Map();
let cacheKey = '';

/** Called by the editor whenever the workspace, its files or its saved content changes. */
export function setLatexCompletionContext(next) {
  context = next;
  const key = next ? `${next.documentId}:${next.version}` : '';
  if (key !== cacheKey) {
    cache = new Map();
    cacheKey = key;
  }
}

function extensionOf(path = '') {
  const index = path.lastIndexOf('.');
  return index < 0 ? '' : path.slice(index).toLowerCase();
}

async function readCached(path) {
  if (!context) return '';
  if (cache.has(path)) return cache.get(path);
  const pending = context.readFile(path).catch(() => '');
  cache.set(path, pending);
  return pending;
}

/** Reads a brace-balanced or quoted BibTeX field value starting at `start`. */
function readBibValue(text, start) {
  let i = start;
  while (i < text.length && /\s/.test(text[i])) i += 1;
  if (text[i] === '{') {
    let depth = 0;
    let out = '';
    for (; i < text.length; i += 1) {
      const ch = text[i];
      if (ch === '{') { depth += 1; if (depth === 1) continue; }
      if (ch === '}') { depth -= 1; if (depth === 0) break; }
      out += ch;
    }
    return out;
  }
  if (text[i] === '"') {
    let out = '';
    for (i += 1; i < text.length && text[i] !== '"'; i += 1) out += text[i];
    return out;
  }
  const match = /^[^,\s}]+/.exec(text.slice(i));
  return match ? match[0] : '';
}

const clean = (value) => value.replace(/[{}\\]/g, '').replace(/\s+/g, ' ').trim();

/** Parses the entries of a .bib file into { key, type, author, title, year }. */
export function parseBib(text) {
  const entries = [];
  const header = /@([A-Za-z]+)\s*[{(]\s*([^,\s{}]+)\s*,/g;
  const positions = [];
  let match;
  while ((match = header.exec(text))) {
    const type = match[1].toLowerCase();
    if (['comment', 'string', 'preamble'].includes(type)) continue;
    positions.push({ type, key: match[2], bodyStart: header.lastIndex, index: match.index });
  }
  positions.forEach((item, i) => {
    const body = text.slice(item.bodyStart, positions[i + 1]?.index ?? text.length);
    const field = (name) => {
      const found = new RegExp(`(?:^|[\\s,])${name}\\s*=\\s*`, 'i').exec(body);
      return found ? clean(readBibValue(body, found.index + found[0].length)) : '';
    };
    entries.push({
      key: item.key,
      type: item.type,
      author: field('author') || field('editor'),
      title: field('title'),
      year: field('year') || field('date').slice(0, 4),
    });
  });
  return entries;
}

function shortAuthors(author) {
  if (!author) return '';
  const names = author.split(/\s+and\s+/i).map((name) => name.split(',')[0].trim().split(/\s+/).pop());
  return names.length > 2 ? `${names[0]} et al.` : names.join(' & ');
}

async function collectBibEntries() {
  const bibFiles = (context?.files || []).filter((file) => extensionOf(file.path) === '.bib');
  const lists = await Promise.all(bibFiles.map(async (file) => parseBib(await readCached(file.path)).map((entry) => ({ ...entry, file: file.path }))));
  return lists.flat();
}

async function collectLabels() {
  const live = context?.getLiveSource?.();
  const texFiles = (context?.files || []).filter((file) => TEX_EXTENSIONS.has(extensionOf(file.path)));
  const labels = [];
  const seen = new Set();
  await Promise.all(texFiles.map(async (file) => {
    const text = live && live.path === file.path ? live.text : await readCached(file.path);
    for (const m of text.matchAll(/\\label\s*\{([^}]+)\}/g)) {
      if (!seen.has(m[1])) { seen.add(m[1]); labels.push({ label: m[1], file: file.path }); }
    }
  }));
  return labels;
}

/** Works out which command argument the cursor is inside, if any. */
function detectArgument(linePrefix) {
  const match = /\\([a-zA-Z]+)\*?(?:\s*\[[^\]]*\])*\s*\{([^{}]*)$/.exec(linePrefix);
  if (!match) return null;
  return { command: match[1], typed: match[2] };
}

export function registerLatexCompletions(monaco) {
  if (registered) return;
  registered = true;

  monaco.languages.registerCompletionItemProvider('latex', {
    triggerCharacters: ['{', ',', '/', ' '],
    async provideCompletionItems(model, position) {
      if (!context) return { suggestions: [] };
      const linePrefix = model.getLineContent(position.lineNumber).slice(0, position.column - 1);
      const arg = detectArgument(linePrefix);
      if (!arg) return { suggestions: [] };

      // Token being typed: after the last comma (for \cite{a,b}) or the whole argument for paths.
      const isList = CITE_COMMANDS.test(arg.command) || REF_COMMANDS.test(arg.command) || arg.command === 'bibliography';
      const typedToken = isList ? arg.typed.slice(arg.typed.lastIndexOf(',') + 1).trimStart() : arg.typed;
      const afterCursor = model.getLineContent(position.lineNumber).slice(position.column - 1);
      const tail = /^[^},\s]*/.exec(afterCursor)?.[0] ?? '';
      const range = new monaco.Range(
        position.lineNumber,
        position.column - typedToken.length,
        position.lineNumber,
        position.column + tail.length,
      );
      const kind = monaco.languages.CompletionItemKind;
      const suggestions = [];

      if (CITE_COMMANDS.test(arg.command)) {
        const entries = await collectBibEntries();
        entries.forEach((entry, index) => {
          const who = shortAuthors(entry.author);
          suggestions.push({
            label: { label: entry.key, description: [who, entry.year].filter(Boolean).join(' ') },
            kind: kind.Reference,
            insertText: entry.key,
            filterText: `${entry.key} ${entry.author} ${entry.title}`,
            detail: `${entry.type} · ${entry.file}`,
            documentation: { value: [entry.title && `**${entry.title}**`, entry.author, entry.year].filter(Boolean).join('\n\n') },
            sortText: String(index).padStart(5, '0'),
            range,
          });
        });
      } else if (GRAPHICS_COMMANDS.test(arg.command)) {
        (context.files || [])
          .filter((file) => IMAGE_EXTENSIONS.has(extensionOf(file.path)))
          .forEach((file, index) => suggestions.push({
            label: file.path,
            kind: kind.File,
            insertText: file.path,
            detail: `${file.kind === 'pdf' ? 'PDF' : 'Image'} · ${(file.size / 1024).toFixed(file.size > 102400 ? 0 : 1)} KB`,
            sortText: String(index).padStart(5, '0'),
            range,
          }));
      } else if (REF_COMMANDS.test(arg.command)) {
        (await collectLabels()).forEach((item, index) => suggestions.push({
          label: item.label,
          kind: kind.Variable,
          insertText: item.label,
          detail: item.file,
          sortText: String(index).padStart(5, '0'),
          range,
        }));
      } else if (INPUT_COMMANDS.test(arg.command)) {
        (context.files || [])
          .filter((file) => TEX_EXTENSIONS.has(extensionOf(file.path)))
          .forEach((file, index) => suggestions.push({
            label: file.path.replace(/\.tex$/i, ''),
            kind: kind.File,
            insertText: file.path.replace(/\.tex$/i, ''),
            detail: file.path,
            sortText: String(index).padStart(5, '0'),
            range,
          }));
      } else if (arg.command === 'bibliography' || arg.command === 'addbibresource') {
        const strip = arg.command === 'bibliography';
        (context.files || [])
          .filter((file) => extensionOf(file.path) === '.bib')
          .forEach((file, index) => suggestions.push({
            label: strip ? file.path.replace(/\.bib$/i, '') : file.path,
            kind: kind.File,
            insertText: strip ? file.path.replace(/\.bib$/i, '') : file.path,
            detail: file.path,
            sortText: String(index).padStart(5, '0'),
            range,
          }));
      }
      return { suggestions };
    },
  });
}
