import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Document, Page } from 'react-pdf';
import { ArrowDownToLine, ArrowLeft, ArrowRight, Check, Crop, FileDown, LoaderCircle, Minus, Plus, RectangleHorizontal, Save, Trash2, X } from 'lucide-react';
import { api, downloadFile, fileUrl, safeName, upload } from '../lib/backend';
import { PDF_OPTIONS } from '../lib/pdf';

const COLORS = [
  { name: 'Yellow', value: '#fff3a6' },
  { name: 'Mint', value: '#b7f0d0' },
  { name: 'Blue', value: '#c2e0ff' },
  { name: 'Pink', value: '#ffd0d9' },
  { name: 'Purple', value: '#e0d2ff' },
];

function normalizedRects(clientRects, bounds) {
  return Array.from(clientRects).map((rect) => {
    const left = Math.max(bounds.left, rect.left);
    const top = Math.max(bounds.top, rect.top);
    const right = Math.min(bounds.right, rect.right);
    const bottom = Math.min(bounds.bottom, rect.bottom);
    return {
      x: Math.max(0, Math.min(1, (left - bounds.left) / bounds.width)),
      y: Math.max(0, Math.min(1, (top - bounds.top) / bounds.height)),
      width: Math.max(0, Math.min(1, (right - left) / bounds.width)),
      height: Math.max(0, Math.min(1, (bottom - top) / bounds.height)),
    };
  }).filter((rect) => rect.width > 0.002 && rect.height > 0.001);
}

export default function PdfReader({ paper, projectId, startPage = 1, active = true, onPageChange, onCloseTab, onClose, onDeletePaper }) {
  const pageShell = useRef(null);
  const dragStart = useRef(null);
  const [numPages, setNumPages] = useState(0);
  const [pageNumber, setPageNumber] = useState(startPage || 1);
  const [zoom, setZoom] = useState(1);
  const [pageWidth, setPageWidth] = useState(790);
  const [tool, setTool] = useState('select');
  const [annotations, setAnnotations] = useState([]);
  const [selectedText, setSelectedText] = useState('');
  const [selectedRects, setSelectedRects] = useState([]);
  const [color, setColor] = useState(COLORS[0].value);
  const [boxLabel, setBoxLabel] = useState('');
  const [drawingRect, setDrawingRect] = useState(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const pdfFile = useMemo(() => ({ url: fileUrl(`/papers/${paper.id}/file`) }), [paper.id]);
  const [cropFile, setCropFile] = useState(null);
  const [cropPreview, setCropPreview] = useState('');
  const [cropTitle, setCropTitle] = useState('Research figure');

  const loadAnnotations = useCallback(async () => {
    try {
      const data = await api(`/papers/${paper.id}/annotations`);
      setAnnotations(data);
    } catch (e) {
      setError(e.message);
    }
  }, [paper.id]);

  useEffect(() => { loadAnnotations(); }, [loadAnnotations]);
  // Jumping to a page from elsewhere (a saved-sentence citation, a tab) moves this reader there.
  useEffect(() => { setPageNumber(startPage || 1); }, [startPage]);
  useEffect(() => { onPageChange?.(paper.id, pageNumber); }, [paper.id, pageNumber, onPageChange]);
  useEffect(() => {
    const setWidth = () => setPageWidth(Math.max(420, Math.min(1000, window.innerWidth - (window.innerWidth > 1100 ? 470 : 48))));
    setWidth();
    window.addEventListener('resize', setWidth);
    return () => window.removeEventListener('resize', setWidth);
  }, []);
  useEffect(() => {
    if (!active) return undefined;
    const keyDown = (event) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [onClose, active]);

  const pageAnnotations = useMemo(() => annotations.filter((a) => a.page_number === pageNumber), [annotations, pageNumber]);
  const sentenceAnnotations = useMemo(() => annotations.filter((a) => a.annotation_type === 'sentence'), [annotations]);

  function captureSelection() {
    if (tool !== 'select') return;
    const selection = window.getSelection();
    const text = selection?.toString().trim() || '';
    if (!text || !pageShell.current) return;
    const range = selection.getRangeAt(0);
    const bounds = pageShell.current.getBoundingClientRect();
    const rects = normalizedRects(range.getClientRects(), bounds);
    if (rects.length) {
      setSelectedText(text);
      setSelectedRects(rects);
    }
  }

  async function saveSentence() {
    if (!selectedText || !selectedRects.length) return;
    setSaving(true); setError('');
    try {
      await api(`/papers/${paper.id}/annotations`, {
        method: 'POST',
        body: JSON.stringify({ page_number: pageNumber, annotation_type: 'sentence', text: selectedText, color, rects: selectedRects }),
      });
      window.getSelection()?.removeAllRanges();
      setSelectedText(''); setSelectedRects([]);
      await loadAnnotations();
    } catch (e) { setError(e.message); }
    finally { setSaving(false); }
  }

  function getNormalizedPointerRect(event) {
    const bounds = pageShell.current.getBoundingClientRect();
    const px = Math.max(0, Math.min(bounds.width, event.clientX - bounds.left));
    const py = Math.max(0, Math.min(bounds.height, event.clientY - bounds.top));
    const sx = dragStart.current.x * bounds.width;
    const sy = dragStart.current.y * bounds.height;
    return {
      x: Math.min(sx, px) / bounds.width,
      y: Math.min(sy, py) / bounds.height,
      width: Math.abs(px - sx) / bounds.width,
      height: Math.abs(py - sy) / bounds.height,
    };
  }

  function pointerDown(event) {
    if (tool === 'select' || !pageShell.current) return;
    event.preventDefault();
    const bounds = pageShell.current.getBoundingClientRect();
    dragStart.current = {
      x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
      y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height)),
    };
    setDrawingRect({ x: dragStart.current.x, y: dragStart.current.y, width: 0, height: 0 });
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  function pointerMove(event) {
    if (!dragStart.current || !pageShell.current) return;
    setDrawingRect(getNormalizedPointerRect(event));
  }

  async function pointerUp(event) {
    if (!dragStart.current || !pageShell.current) return;
    const rect = getNormalizedPointerRect(event);
    dragStart.current = null;
    setDrawingRect(null);
    if (rect.width < 0.006 || rect.height < 0.006) return;
    if (tool === 'rectangle') {
      setSaving(true); setError('');
      try {
        await api(`/papers/${paper.id}/annotations`, {
          method: 'POST',
          body: JSON.stringify({ page_number: pageNumber, annotation_type: 'rectangle', text: boxLabel.trim(), color, rects: [rect] }),
        });
        setTool('select');
        await loadAnnotations();
      } catch (e) { setError(e.message); }
      finally { setSaving(false); }
    } else if (tool === 'crop') {
      try {
        const canvas = pageShell.current.querySelector('canvas');
        if (!canvas) throw new Error('The PDF page canvas is not ready yet.');
        const sx = Math.round(rect.x * canvas.width);
        const sy = Math.round(rect.y * canvas.height);
        const sw = Math.max(1, Math.round(rect.width * canvas.width));
        const sh = Math.max(1, Math.round(rect.height * canvas.height));
        const output = document.createElement('canvas');
        output.width = sw; output.height = sh;
        const context = output.getContext('2d');
        context.drawImage(canvas, sx, sy, sw, sh, 0, 0, sw, sh);
        const blob = await new Promise((resolve) => output.toBlob(resolve, 'image/png'));
        if (!blob) throw new Error('Could not crop that selection. Please try again.');
        const file = new File([blob], `paper-${paper.citation_number}-page-${pageNumber}-crop.png`, { type: 'image/png' });
        setCropFile(file);
        setCropPreview(URL.createObjectURL(blob));
        setCropTitle(`Paper ${paper.citation_number} figure`);
      } catch (e) { setError(e.message); }
    }
  }

  async function saveCrop() {
    if (!cropFile) return;
    setSaving(true); setError('');
    try {
      const form = new FormData();
      form.append('project_id', String(projectId));
      form.append('paper_id', String(paper.id));
      form.append('page_number', String(pageNumber));
      form.append('title', cropTitle);
      form.append('file', cropFile);
      await upload('/images', form);
      if (cropPreview) URL.revokeObjectURL(cropPreview);
      setCropPreview(''); setCropFile(null); setTool('select');
    } catch (e) { setError(e.message); }
    finally { setSaving(false); }
  }

  async function deleteAnnotation(id) {
    setError('');
    try { await api(`/annotations/${id}`, { method: 'DELETE' }); await loadAnnotations(); }
    catch (e) { setError(e.message); }
  }

  async function saveCopy(kind) {
    setError(''); setNotice('');
    try {
      const stem = safeName(paper.original_filename.replace(/\.pdf$/i, ''));
      const saved = kind === 'annotated'
        ? await downloadFile(`/papers/${paper.id}/annotated-file`, `${stem}_annotated.pdf`)
        : await downloadFile(`/papers/${paper.id}/download`, `${stem}.pdf`);
      if (saved) setNotice(`Saved to ${saved}`);
    } catch (e) { setError(e.message || 'Could not save the PDF.'); }
  }

  function goPage(next) {
    setPageNumber(Math.max(1, Math.min(numPages || 1, next)));
    setSelectedText(''); setSelectedRects([]); window.getSelection()?.removeAllRanges();
  }

  return (
    <div className={`reader-backdrop ${active ? '' : 'reader-hidden'}`} role="tabpanel" aria-hidden={!active} aria-label={`Reading ${paper.title}`}>
      <div className="reader-window">
        <header className="reader-header">
          <div className="reader-heading">
            <button className="icon-button dark-icon" title="Back to project" onClick={onClose}><ArrowLeft size={18} /></button>
            <div><div className="reader-title">{paper.title}</div><div className="reader-subtitle">[{paper.citation_number}] · {paper.category_name} · {paper.original_filename}</div></div>
          </div>
          <div className="reader-actions">
            {onDeletePaper && <button className="button button-danger button-sm" onClick={onDeletePaper} title="Delete this research paper"><Trash2 size={14} /> Delete paper</button>}
            <button className="button button-muted button-sm" onClick={() => saveCopy('original')} title="Save the original PDF"><FileDown size={15} /> Original PDF</button>
            <button className="button button-muted button-sm" onClick={() => saveCopy('annotated')} title="Save a copy with your highlights and boxes drawn on it"><ArrowDownToLine size={15} /> Annotated PDF</button>
            <button className="icon-button dark-icon" title="Close this tab (Ctrl+W)" onClick={onCloseTab || onClose}><X size={18} /></button>
          </div>
        </header>
        <div className="reader-toolbar">
          <div className="reader-tool-group">
            <button className={`tool-button ${tool === 'select' ? 'active' : ''}`} onClick={() => setTool('select')}><span className="tool-cursor">↖</span> Select text</button>
            <button className={`tool-button ${tool === 'rectangle' ? 'active' : ''}`} onClick={() => { setTool('rectangle'); setSelectedText(''); }}><RectangleHorizontal size={15} /> Box highlight</button>
            <button className={`tool-button ${tool === 'crop' ? 'active' : ''}`} onClick={() => { setTool('crop'); setSelectedText(''); }}><Crop size={15} /> Crop image</button>
            {tool === 'rectangle' && <input className="box-label-input" value={boxLabel} onChange={(e) => setBoxLabel(e.target.value)} placeholder="Optional box label" aria-label="Box label" />}
          </div>
          <div className="reader-tool-group reader-pages">
            <button className="icon-button" disabled={pageNumber <= 1} onClick={() => goPage(pageNumber - 1)} aria-label="Previous page"><ArrowLeft size={15} /></button>
            <span className="page-indicator"><b>{pageNumber}</b> / {numPages || '—'}</span>
            <button className="icon-button" disabled={pageNumber >= numPages} onClick={() => goPage(pageNumber + 1)} aria-label="Next page"><ArrowRight size={15} /></button>
            <span className="tool-divider" />
            <button className="icon-button" onClick={() => setZoom((z) => Math.max(.65, +(z - .1).toFixed(2)))} aria-label="Zoom out"><Minus size={15} /></button>
            <span className="zoom-indicator">{Math.round(zoom * 100)}%</span>
            <button className="icon-button" onClick={() => setZoom((z) => Math.min(1.6, +(z + .1).toFixed(2)))} aria-label="Zoom in"><Plus size={15} /></button>
          </div>
        </div>
        <div className="reader-content">
          <div className="pdf-stage">
            <div className="pdf-stage-note">{tool === 'select' ? 'Select a sentence in the PDF, then save it as an Important Sentence.' : tool === 'rectangle' ? 'Drag a transparent colored box around a table, figure, or passage.' : 'Drag around a diagram or image. It will only be saved to Important Images after you confirm.'}</div>
            <div className="pdf-sheet-wrap">
              <Document
                file={pdfFile}
                options={PDF_OPTIONS}
                onLoadSuccess={({ numPages: count }) => { setNumPages(count); setPageNumber((p) => Math.min(Math.max(1, p), count)); }}
                onLoadError={(e) => setError(`Could not load PDF: ${e.message}`)}
                loading={<div className="loading-state"><LoaderCircle className="spin" /> Loading PDF…</div>}
                error={<div className="empty-state">Unable to render this PDF in the browser.</div>}
              >
                <div className="pdf-page-shell" ref={pageShell} onMouseUp={captureSelection}>
                  <Page pageNumber={pageNumber} width={pageWidth * zoom} renderTextLayer renderAnnotationLayer={false} />
                  <div className="pdf-annotation-visuals" aria-hidden="true">
                    {pageAnnotations.map((annotation) => (annotation.rects || []).map((rect, i) => (
                      <div key={`${annotation.id}-${i}`} className={annotation.annotation_type === 'sentence' ? 'saved-sentence-mark' : 'saved-box-mark'} title={annotation.text || 'Box highlight'} style={{ left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%`, backgroundColor: annotation.annotation_type === 'sentence' ? `${annotation.color}40` : `${annotation.color}12`, borderColor: annotation.color }} />
                    )))}
                    {drawingRect && <div className="live-draw-mark" style={{ left: `${drawingRect.x * 100}%`, top: `${drawingRect.y * 100}%`, width: `${drawingRect.width * 100}%`, height: `${drawingRect.height * 100}%`, borderColor: color, backgroundColor: tool === 'rectangle' ? `${color}12` : `${color}30` }} />}
                  </div>
                  {tool !== 'select' && <div className="pdf-draw-surface" onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} style={{ cursor: tool === 'crop' ? 'crosshair' : 'cell' }} />}
                </div>
              </Document>
            </div>
          </div>
          <aside className="reader-side-panel">
            <div className="side-panel-title-row"><div><h3>Mark & collect</h3><p>Saved to this paper automatically</p></div><span className="count-pill">{annotations.length}</span></div>
            {tool === 'select' && <div className="sentence-save-panel">
              <div className="panel-eyebrow">IMPORTANT SENTENCES</div>
              {selectedText ? <><div className="selected-quote">“{selectedText}”</div><div className="color-picker-row">{COLORS.map((c) => <button key={c.value} title={c.name} className={`color-dot ${color === c.value ? 'chosen' : ''}`} style={{ backgroundColor: c.value }} onClick={() => setColor(c.value)} />)}</div><button className="button button-primary button-full" disabled={saving} onClick={saveSentence}>{saving ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />} Save selected sentence</button></> : <div className="hint-box"><span className="hint-icon">↗</span><span>Select a sentence in the PDF. It will be saved here and in your project’s <b>Important Sentences</b> section.</span></div>}
            </div>}
            {tool === 'rectangle' && <div className="tool-instructions"><div className="panel-eyebrow">BOX HIGHLIGHT</div><p>Choose a color and drag a box around the content. Boxes remain on this PDF but are not added to Important Sentences or Important Images.</p><div className="color-picker-row">{COLORS.map((c) => <button key={c.value} title={c.name} className={`color-dot ${color === c.value ? 'chosen' : ''}`} style={{ backgroundColor: c.value }} onClick={() => setColor(c.value)} />)}</div></div>}
            {tool === 'crop' && <div className="tool-instructions"><div className="panel-eyebrow">CROP TO IMPORTANT IMAGES</div><p>Drag around only the figure or diagram you want to keep. A confirmation step will let you name it before saving.</p></div>}
            <div className="saved-mark-list">
              <div className="saved-list-heading"><span>Saved on this paper</span><span>{pageAnnotations.length} on page {pageNumber}</span></div>
              {pageAnnotations.length === 0 ? <div className="empty-mini">No annotations on this page yet.</div> : pageAnnotations.map((item) => <div className="saved-mark-item" key={item.id}>
                <span className="mark-color" style={{ backgroundColor: item.color }} />
                <div className="saved-mark-copy"><b>{item.annotation_type === 'sentence' ? 'Important sentence' : 'Box highlight'}</b><p>{item.text || `Page ${item.page_number} region`}</p><small>Page {item.page_number}</small></div>
                <button className="mini-icon-button" title="Delete annotation" onClick={() => deleteAnnotation(item.id)}><Trash2 size={14} /></button>
              </div>)}
              {sentenceAnnotations.length > 0 && <div className="reader-sentence-count"><Check size={14} /> {sentenceAnnotations.length} important sentence{sentenceAnnotations.length === 1 ? '' : 's'} saved from this paper</div>}
            </div>
            {notice && <div className="inline-notice">{notice}</div>}
            {error && <div className="inline-error">{error}</div>}
          </aside>
        </div>
      </div>
      {cropPreview && <div className="modal-scrim crop-scrim"><div className="crop-confirm-card">
        <button className="modal-close" onClick={() => { URL.revokeObjectURL(cropPreview); setCropPreview(''); setCropFile(null); }}><X size={18} /></button>
        <div className="modal-eyebrow">IMPORTANT IMAGES</div><h2>Save cropped figure</h2><p>This crop is separate from sentence highlights and PDF box annotations.</p>
        <div className="crop-preview-wrap"><img src={cropPreview} alt="Cropped research figure preview" /></div>
        <label className="field-label">Image name<input value={cropTitle} onChange={(e) => setCropTitle(e.target.value)} placeholder="e.g. Quantum circuit diagram" /></label>
        <div className="modal-actions"><button className="button button-muted" onClick={() => { URL.revokeObjectURL(cropPreview); setCropPreview(''); setCropFile(null); }}>Cancel</button><button className="button button-primary" disabled={saving} onClick={saveCrop}>{saving ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />} Save to Important Images</button></div>
      </div></div>}
    </div>
  );
}
