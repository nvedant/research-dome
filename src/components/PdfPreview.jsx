// Scrollable multi-page PDF viewer used for the LaTeX preview and PDF assets.
// Rendered with PDF.js rather than <iframe>, because Linux's WebKitGTK webview has no
// built-in PDF viewer; this works identically on Windows, macOS and Linux.
import { useEffect, useMemo, useRef, useState } from 'react';
import { Document, Page } from 'react-pdf';
import { LoaderCircle, Minus, Plus } from 'lucide-react';
import { PDF_OPTIONS } from '../lib/pdf';

function LazyPage({ pageNumber, width }) {
  const holder = useRef(null);
  const [visible, setVisible] = useState(pageNumber <= 2);

  useEffect(() => {
    if (visible || !holder.current) return undefined;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) { setVisible(true); observer.disconnect(); }
    }, { rootMargin: '900px 0px' });
    observer.observe(holder.current);
    return () => observer.disconnect();
  }, [visible]);

  return (
    <div ref={holder} className="pdf-preview-page" style={{ width, minHeight: visible ? undefined : Math.round(width * 1.294) }}>
      {visible && <Page pageNumber={pageNumber} width={width} renderTextLayer={false} renderAnnotationLayer={false} />}
    </div>
  );
}

export default function PdfPreview({ url }) {
  const scroller = useRef(null);
  const [numPages, setNumPages] = useState(0);
  const [containerWidth, setContainerWidth] = useState(640);
  const [zoom, setZoom] = useState(1);
  const [error, setError] = useState('');
  const file = useMemo(() => ({ url }), [url]);

  useEffect(() => {
    const node = scroller.current;
    if (!node) return undefined;
    const measure = () => setContainerWidth(Math.max(280, node.clientWidth - 40));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const pageWidth = Math.round(containerWidth * zoom);

  return (
    <div className="pdf-preview">
      <div className="pdf-preview-tools">
        <button className="icon-button" onClick={() => setZoom((z) => Math.max(.5, +(z - .15).toFixed(2)))} aria-label="Zoom out"><Minus size={15} /></button>
        <span>{Math.round(zoom * 100)}%</span>
        <button className="icon-button" onClick={() => setZoom((z) => Math.min(2.5, +(z + .15).toFixed(2)))} aria-label="Zoom in"><Plus size={15} /></button>
        {numPages > 0 && <span className="pdf-preview-pages">{numPages} page{numPages === 1 ? '' : 's'}</span>}
      </div>
      <div className="pdf-preview-scroll" ref={scroller}>
        {error ? <div className="inline-error">{error}</div> : (
          <Document
            file={file}
            options={PDF_OPTIONS}
            onLoadSuccess={({ numPages: count }) => { setNumPages(count); setError(''); }}
            onLoadError={(e) => setError(`Could not display this PDF: ${e.message}`)}
            loading={<div className="loading-state"><LoaderCircle className="spin" size={18} /> Loading preview…</div>}
          >
            {Array.from({ length: numPages }, (_, index) => <LazyPage key={index + 1} pageNumber={index + 1} width={pageWidth} />)}
          </Document>
        )}
      </div>
    </div>
  );
}
