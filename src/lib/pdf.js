import { pdfjs } from 'react-pdf';

// Worker bundled with the app (legacy build, see vite.config.js) — fully offline.
pdfjs.GlobalWorkerOptions.workerSrc = new URL('pdfjs-dist/legacy/build/pdf.worker.min.mjs', import.meta.url).toString();

// Defined once: react-pdf reloads the document whenever this object changes identity.
// isEvalSupported:false keeps PDF.js compatible with the app's strict content-security policy.
export const PDF_OPTIONS = { isEvalSupported: false };

export { pdfjs };
