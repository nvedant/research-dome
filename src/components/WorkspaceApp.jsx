import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { usePathname, useRouter } from '../lib/router';
import {
  ArrowDownToLine, ArrowRight, ArrowUpRight, Check, ChevronRight, Code2, Copy,
  FileText, FolderOpen, FolderPlus, Image as ImageIcon, LayoutDashboard, LibraryBig, LoaderCircle,
  Menu, Moon, Plus, Quote, Search, Sun, Trash2, UploadCloud, X,
} from 'lucide-react';
import { api, downloadFile, fileUrl, isDesktop, openDataFolder, safeName, upload } from '../lib/backend';
import { useConfirm } from './Confirm';
import ReaderTabs from './ReaderTabs';
import iconUrl from '../assets/icon-160.png';

// PDF.js is only needed when a paper is opened.
const PdfReader = lazy(() => import('./PdfReader'));
const LaTeXStudio = lazy(() => import('./LaTeXStudio'));

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || '');
const SEARCH_HINT = isMac ? '⌘K' : 'Ctrl K';

function BrandMark({ size = 36 }) {
  return <span className="brand-mark" style={{ width: size, height: size }}><img src={iconUrl} alt="" draggable="false" /></span>;
}

const SECTIONS = [
  { id: 'papers', label: 'Papers', icon: LibraryBig },
  { id: 'sentences', label: 'Sentences', icon: Quote },
  { id: 'images', label: 'Figures', icon: ImageIcon },
  { id: 'latex', label: 'LaTeX Studio', icon: Code2 },
];

function formatDate(input) {
  if (!input) return 'Just now';
  const date = new Date(input);
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

function formatSize(bytes = 0) {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function hueFor(text = '') {
  let hash = 0;
  for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) % 360;
  return hash;
}

function InitialAvatar({ name = 'Research', size = 'md' }) {
  const letters = name.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => word[0]).join('').toUpperCase() || 'R';
  return <div className={`project-avatar avatar-${size}`} style={{ '--h': hueFor(name) }}>{letters}</div>;
}

function EmptyState({ icon: Icon, title, children, action }) {
  return (
    <div className="empty-state-card">
      <div className="empty-state-icon"><Icon size={22} /></div>
      <h3>{title}</h3>
      <p>{children}</p>
      {action}
    </div>
  );
}

function Modal({ onClose, icon: Icon, title, subtitle, children, wide = false }) {
  return (
    <div className="modal-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal-card ${wide ? 'modal-wide' : ''}`} role="dialog" aria-modal="true" aria-label={title}>
        <button className="modal-close" onClick={onClose} aria-label="Close"><X size={18} /></button>
        <div className="modal-head">
          <div className="modal-icon"><Icon size={20} /></div>
          <div><h2>{title}</h2>{subtitle && <p>{subtitle}</p>}</div>
        </div>
        {children}
      </div>
    </div>
  );
}

export default function WorkspaceApp() {
  const pathname = usePathname() || '/';
  const router = useRouter();
  const confirm = useConfirm();
  const routeParts = pathname.split('/').filter(Boolean);
  const activeProjectId = routeParts[0] === 'projects' ? routeParts[1] : null;
  const routeCategoryId = routeParts[2] === 'categories' ? routeParts[3] : null;
  const isReaderRoute = Boolean(activeProjectId && routeParts[2] === 'reader');
  const routePaperId = isReaderRoute ? routeParts[3] : null;
  const readerStartPage = isReaderRoute ? Math.max(1, Number(routeParts[4]) || 1) : 1;
  const activeTab = ['papers', 'sentences', 'images', 'latex'].includes(routeParts[2]) ? routeParts[2] : 'papers';
  const goTab = useCallback((tab) => {
    if (!activeProjectId) return;
    const target = tab === 'papers' ? `/projects/${activeProjectId}/papers` : `/projects/${activeProjectId}/${tab}`;
    setSidebarOpen(false);
    if (pathname !== target) router.push(target);
  }, [activeProjectId, pathname, router]);
  const [projects, setProjects] = useState([]);
  const [categories, setCategories] = useState([]);
  const [papers, setPapers] = useState([]);
  const [sentences, setSentences] = useState([]);
  const [images, setImages] = useState([]);
  const [latexDocuments, setLatexDocuments] = useState([]);
  const [loading, setLoading] = useState(true);
  const [projectLoading, setProjectLoading] = useState(false);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  const [localCategoryFilter, setLocalCategoryFilter] = useState('all');
  const categoryFilter = routeCategoryId || localCategoryFilter;
  const setCategoryFilter = (value) => {
    const next = String(value);
    setLocalCategoryFilter(next);
    if (!activeProjectId) return;
    const target = next !== 'all' ? `/projects/${activeProjectId}/categories/${next}` : `/projects/${activeProjectId}/papers`;
    if (pathname !== target) router.push(target);
  };
  const [showCreateProject, setShowCreateProject] = useState(false);
  const [showCreateCategory, setShowCreateCategory] = useState(false);
  const [showUpload, setShowUpload] = useState(false);
  const [categoryToDelete, setCategoryToDelete] = useState(null);
  const [categoryDeleteStrategy, setCategoryDeleteStrategy] = useState('keep');
  const [deletingCategory, setDeletingCategory] = useState(false);
  const [movingPaperId, setMovingPaperId] = useState(null);
  const [projectName, setProjectName] = useState('');
  const [projectDescription, setProjectDescription] = useState('');
  const [categoryName, setCategoryName] = useState('');
  const [categoryDescription, setCategoryDescription] = useState('');
  const [uploadCategoryId, setUploadCategoryId] = useState('');
  const [uploadFiles, setUploadFiles] = useState([]);
  const [uploadProgress, setUploadProgress] = useState('');
  const [dragOver, setDragOver] = useState(false);
  const [creating, setCreating] = useState(false);
  const [previewImage, setPreviewImage] = useState(null);
  const [toast, setToast] = useState('');
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [theme, setTheme] = useState('dark');
  const searchRef = useRef(null);

  const activeProject = projects.find((project) => String(project.id) === String(activeProjectId)) || null;
  const readerPaper = isReaderRoute ? (papers.find((paper) => String(paper.id) === String(routePaperId)) ? { ...papers.find((paper) => String(paper.id) === String(routePaperId)), startPage: readerStartPage } : null) : null;

  // ---- Reader tabs: several PDFs open at once, switch between them like browser tabs ----
  const [tabs, setTabs] = useState([]); // [{ id, page }]
  const [loadedProjectId, setLoadedProjectId] = useState(null); // which project's papers are in state
  const tabsProjectRef = useRef(null);
  const visitedRef = useRef(new Set());
  const tabsKey = (projectId) => `rd-tabs-${projectId}`;

  // Restore the tabs this project had open last time (only papers that still exist).
  useEffect(() => {
    if (!activeProjectId || loadedProjectId !== activeProjectId) return;
    if (tabsProjectRef.current !== activeProjectId) {
      tabsProjectRef.current = activeProjectId;
      let saved = [];
      try { saved = JSON.parse(localStorage.getItem(tabsKey(activeProjectId)) || '[]'); } catch { saved = []; }
      setTabs((Array.isArray(saved) ? saved : []).filter((tab) => papers.some((paper) => String(paper.id) === String(tab.id))).map((tab) => ({ id: String(tab.id), page: Math.max(1, Number(tab.page) || 1) })));
      return;
    }
    // Drop tabs whose paper was deleted.
    setTabs((current) => {
      const next = current.filter((tab) => papers.some((paper) => String(paper.id) === String(tab.id)));
      return next.length === current.length ? current : next;
    });
  }, [activeProjectId, loadedProjectId, papers]);

  useEffect(() => {
    if (!activeProjectId || tabsProjectRef.current !== activeProjectId) return;
    try { localStorage.setItem(tabsKey(activeProjectId), JSON.stringify(tabs)); } catch { /* storage unavailable */ }
  }, [tabs, activeProjectId]);

  // Opening a paper by URL (or from anywhere) makes sure it has a tab.
  useEffect(() => {
    if (!isReaderRoute || !routePaperId || loadedProjectId !== activeProjectId || !papers.some((paper) => String(paper.id) === String(routePaperId))) return;
    setTabs((current) => current.some((tab) => String(tab.id) === String(routePaperId)) ? current : [...current, { id: String(routePaperId), page: readerStartPage }]);
  }, [isReaderRoute, routePaperId, readerStartPage, loadedProjectId, activeProjectId, papers]);

  useEffect(() => { if (!isReaderRoute) visitedRef.current = new Set(); }, [isReaderRoute]);

  const rememberPage = useCallback((paperId, page) => {
    setTabs((current) => current.some((tab) => String(tab.id) === String(paperId) && tab.page !== page)
      ? current.map((tab) => String(tab.id) === String(paperId) ? { ...tab, page } : tab) : current);
  }, []);

  const tabPapers = useMemo(() => tabs.map((tab) => {
    const paper = papers.find((item) => String(item.id) === String(tab.id));
    return paper ? { ...paper, tabPage: tab.page } : null;
  }).filter(Boolean), [tabs, papers]);

  const reloadProjectData = useCallback(async () => {
    if (!activeProjectId) return;
    setProjectLoading(true); setError('');
    try {
      const [categoryData, paperData, sentenceData, imageData, latexData] = await Promise.all([
        api(`/projects/${activeProjectId}/categories`),
        api(`/projects/${activeProjectId}/papers`),
        api(`/projects/${activeProjectId}/important-sentences`),
        api(`/projects/${activeProjectId}/images`),
        api(`/projects/${activeProjectId}/latex-documents`),
      ]);
      setCategories(categoryData); setPapers(paperData); setLoadedProjectId(activeProjectId); setSentences(sentenceData); setImages(imageData); setLatexDocuments(latexData);
      setUploadCategoryId((current) => categoryData.some((cat) => String(cat.id) === String(current)) ? current : (categoryData[0] ? String(categoryData[0].id) : ''));
      return latexData;
    } catch (e) { setError(e.message); }
    finally { setProjectLoading(false); }
  }, [activeProjectId]);

  useEffect(() => {
    let alive = true;
    api('/projects').then((data) => {
      if (!alive) return;
      setProjects(data);
    }).catch((e) => alive && setError(e.message)).finally(() => alive && setLoading(false));
    return () => { alive = false; };
  }, []);

  useEffect(() => { reloadProjectData(); }, [reloadProjectData]);

  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(''), 3000);
    return () => clearTimeout(timer);
  }, [toast]);

  // Theme: main.jsx applies the saved data-theme before first paint; here we just stay in sync.
  useEffect(() => {
    setTheme(document.documentElement.dataset.theme === 'light' ? 'light' : 'dark');
  }, []);
  function toggleTheme() {
    const next = theme === 'dark' ? 'light' : 'dark';
    setTheme(next);
    document.documentElement.dataset.theme = next;
    try { localStorage.setItem('rd-theme', next); } catch {}
  }

  // Keyboard: ⌘/Ctrl+K focuses search, Escape closes dialogs and the mobile menu.
  useEffect(() => {
    const onKey = (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      } else if (event.key === 'Escape') {
        setShowCreateProject(false); setShowCreateCategory(false); setShowUpload(false);
        setPreviewImage(null); setSidebarOpen(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  function openProject(id) {
    setLocalCategoryFilter('all');
    setQuery('');
    setSidebarOpen(false);
    router.push(`/projects/${id}`);
  }

  function goHome() {
    setQuery('');
    setSidebarOpen(false);
    router.push('/');
  }

  function startUpload() {
    if (!categories.length) {
      setShowCreateCategory(true);
      setError('Create a category before uploading PDFs.');
    } else {
      setShowUpload(true);
    }
  }

  async function createProject(event) {
    event.preventDefault();
    if (!projectName.trim()) return;
    setCreating(true); setError('');
    try {
      const created = await api('/projects', { method: 'POST', body: JSON.stringify({ name: projectName.trim(), description: projectDescription.trim() }) });
      setProjects((current) => [created, ...current]);
      setLocalCategoryFilter('all');
      router.push(`/projects/${created.id}`);
      setProjectName(''); setProjectDescription(''); setShowCreateProject(false);
      setToast('Project created');
    } catch (e) { setError(e.message); }
    finally { setCreating(false); }
  }

  async function deleteProject(project) {
    if (!project?.id) return;
    const confirmed = await confirm(`Permanently delete project “${project.name}” and all of its categories, papers, annotations, saved images, and LaTeX workspaces? This cannot be undone.`, { title: 'Delete project?', confirmLabel: 'Delete project' });
    if (!confirmed) return;
    setError('');
    try {
      await api(`/projects/${project.id}`, { method: 'DELETE' });
      setProjects((current) => current.filter((item) => String(item.id) !== String(project.id)));
      if (String(activeProjectId) === String(project.id)) router.push('/');
      setToast('Project deleted');
    } catch (e) { setError(e.message || 'Could not delete the project.'); }
  }

  async function createCategory(event) {
    event.preventDefault();
    if (!categoryName.trim() || !activeProjectId) return;
    setCreating(true); setError('');
    try {
      const created = await api(`/projects/${activeProjectId}/categories`, { method: 'POST', body: JSON.stringify({ name: categoryName.trim(), description: categoryDescription.trim() }) });
      setCategories((current) => [...current, created]);
      setUploadCategoryId(String(created.id)); setCategoryFilter(String(created.id));
      setCategoryName(''); setCategoryDescription(''); setShowCreateCategory(false); setToast('Category created');
    } catch (e) { setError(e.message); }
    finally { setCreating(false); }
  }

  function addUploadFiles(fileList) {
    const incoming = Array.from(fileList || []).filter((file) => file.type === 'application/pdf' || /\.pdf$/i.test(file.name));
    if (!incoming.length) return;
    setUploadFiles((current) => {
      const seen = new Set(current.map((file) => `${file.name}:${file.size}`));
      return [...current, ...incoming.filter((file) => !seen.has(`${file.name}:${file.size}`))];
    });
  }

  async function uploadResearchPapers(event) {
    event.preventDefault();
    if (!uploadFiles.length || !uploadCategoryId || !activeProjectId) return;
    setCreating(true); setError('');
    try {
      for (let index = 0; index < uploadFiles.length; index++) {
        const file = uploadFiles[index];
        setUploadProgress(`Uploading ${index + 1} of ${uploadFiles.length}: ${file.name}`);
        const form = new FormData();
        form.append('project_id', String(activeProjectId));
        form.append('category_id', String(uploadCategoryId));
        form.append('file', file);
        await upload('/papers', form);
      }
      const uploadedCount = uploadFiles.length; setUploadFiles([]); setUploadProgress(''); setShowUpload(false); setToast(`${uploadedCount} paper${uploadedCount === 1 ? '' : 's'} uploaded`);
      await reloadProjectData();
    } catch (e) { setError(e.message); }
    finally { setCreating(false); setUploadProgress(''); }
  }

  async function deleteImage(image) {
    if (!(await confirm(`Delete “${image.title}” from Important Images?`, { title: 'Delete figure?', confirmLabel: 'Delete figure' }))) return;
    try { await api(`/images/${image.id}`, { method: 'DELETE' }); setImages((current) => current.filter((item) => item.id !== image.id)); setToast('Image clipping deleted'); }
    catch (e) { setError(e.message); }
  }

  async function deletePaper(paper, { fromReader = false } = {}) {
    const title = paper?.title || 'this research paper';
    if (!paper?.id || !(await confirm(`Delete “${title}”? This permanently removes the PDF and its saved sentence/box annotations. Cropped images already saved to Important Images will be kept.`, { title: 'Delete paper?', confirmLabel: 'Delete paper' }))) return;
    try {
      await api(`/papers/${paper.id}`, { method: 'DELETE' });
      if (fromReader) closeTab(paper.id);
      else setTabs((current) => current.filter((tab) => String(tab.id) !== String(paper.id)));
      await reloadProjectData();
      setToast('Research paper deleted');
    } catch (e) {
      if (fromReader) setToast(`Could not delete this paper: ${e.message}`);
      else setError(e.message || 'Could not delete the research paper.');
    }
  }

  async function movePaperToCategory(paper, selectedValue) {
    if (!paper?.id) return;
    const categoryId = selectedValue === 'uncategorized' ? null : Number(selectedValue);
    if (categoryId === (paper.category_id == null ? null : Number(paper.category_id))) return;
    setMovingPaperId(paper.id); setError('');
    try {
      await api(`/papers/${paper.id}`, { method: 'PATCH', body: JSON.stringify({ category_id: categoryId }) });
      await reloadProjectData();
      setToast(categoryId == null ? 'Paper moved to Uncategorized' : 'Paper category updated');
    } catch (e) {
      setError(e.message || 'Could not change this paper’s category.');
    } finally { setMovingPaperId(null); }
  }

  async function confirmDeleteCategory() {
    if (!categoryToDelete || !activeProjectId) return;
    const category = categoryToDelete;
    const deletePapers = categoryDeleteStrategy === 'delete';
    setDeletingCategory(true); setError('');
    try {
      await api(`/categories/${category.id}?delete_papers=${deletePapers ? 'true' : 'false'}`, { method: 'DELETE' });
      setCategoryToDelete(null);
      setCategoryDeleteStrategy('keep');
      if (String(categoryFilter) === String(category.id)) {
        setLocalCategoryFilter('all');
        router.push(`/projects/${activeProjectId}/papers`);
      }
      await reloadProjectData();
      setToast(deletePapers ? `Category and ${category.paper_count || 0} paper(s) deleted` : 'Category deleted; papers moved to Uncategorized');
    } catch (e) {
      setError(e.message || 'Could not delete this category. Check that the updated backend route is installed.');
    } finally { setDeletingCategory(false); }
  }

  async function copySentence(sentence) {
    const text = `“${sentence.text}” [${sentence.citation_number}]`;
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const area = document.createElement('textarea');
        area.value = text; document.body.appendChild(area); area.select();
        const ok = document.execCommand('copy'); area.remove();
        if (!ok) throw new Error('copy failed');
      }
      setToast('Copied with citation');
    } catch { setError('Could not copy to the clipboard.'); }
  }

  async function deleteSentence(sentence) {
    if (!sentence?.id || !(await confirm('Delete this saved Important Sentence? The source PDF itself will remain.', { title: 'Delete sentence?', confirmLabel: 'Delete sentence' }))) return;
    try {
      await api(`/annotations/${sentence.id}`, { method: 'DELETE' });
      setSentences((current) => current.filter((item) => item.id !== sentence.id));
      setPapers((current) => current.map((paper) => String(paper.id) === String(sentence.paper_id) ? { ...paper, annotation_count: Math.max(0, Number(paper.annotation_count || 0) - 1) } : paper));
      setToast('Important Sentence deleted');
    } catch (e) { setError(e.message || 'Could not delete this sentence.'); }
  }

  function openPaperById(paperId, pageNumber = 1) {
    const paper = papers.find((item) => String(item.id) === String(paperId));
    if (!paper || !activeProjectId) return;
    const page = Math.max(1, Number(pageNumber) || 1);
    setTabs((current) => current.some((tab) => String(tab.id) === String(paper.id))
      ? current.map((tab) => String(tab.id) === String(paper.id) ? { ...tab, page } : tab)
      : [...current, { id: String(paper.id), page }]);
    router.push(`/projects/${activeProjectId}/reader/${paper.id}/${page}`);
  }

  function selectTab(paperId) {
    const tab = tabs.find((item) => String(item.id) === String(paperId));
    if (tab && activeProjectId) router.push(`/projects/${activeProjectId}/reader/${tab.id}/${tab.page}`);
  }

  function closeTab(paperId) {
    const index = tabs.findIndex((tab) => String(tab.id) === String(paperId));
    if (index < 0) return;
    const remaining = tabs.filter((tab) => String(tab.id) !== String(paperId));
    visitedRef.current.delete(String(paperId));
    setTabs(remaining);
    if (isReaderRoute && String(routePaperId) === String(paperId) && activeProjectId) {
      const next = remaining[index] || remaining[index - 1];
      router.push(next ? `/projects/${activeProjectId}/reader/${next.id}/${next.page}` : `/projects/${activeProjectId}/papers`);
    }
  }

  // Keyboard: Ctrl+W closes the current tab, Ctrl+Tab / Ctrl+Shift+Tab cycle through them.
  useEffect(() => {
    if (!isReaderRoute) return undefined;
    const onKey = (event) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const index = tabs.findIndex((tab) => String(tab.id) === String(routePaperId));
      if (event.key.toLowerCase() === 'w' && index >= 0) { event.preventDefault(); closeTab(routePaperId); }
      else if (event.key === 'Tab' && tabs.length > 1) {
        event.preventDefault();
        const next = tabs[(index + (event.shiftKey ? -1 : 1) + tabs.length) % tabs.length];
        selectTab(next.id);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  async function saveImageAs(image) {
    try {
      const saved = await downloadFile(`/images/${image.id}/file`, `${safeName(image.title)}.png`);
      if (saved) setToast('Figure saved');
    } catch (e) { setError(e.message || 'Could not save the figure.'); }
  }

  const needle = query.trim().toLowerCase();

  const filteredProjects = useMemo(() => projects.filter((project) => !needle || `${project.name} ${project.description || ''}`.toLowerCase().includes(needle)), [projects, needle]);

  const filteredPapers = useMemo(() => papers.filter((paper) => {
    const categoryMatches = categoryFilter === 'all'
      || (categoryFilter === 'uncategorized' ? paper.category_id == null : String(paper.category_id) === String(categoryFilter));
    const queryMatches = !needle || `${paper.title} ${paper.original_filename} ${paper.category_name}`.toLowerCase().includes(needle);
    return categoryMatches && queryMatches;
  }), [papers, categoryFilter, needle]);

  const filteredSentences = useMemo(() => sentences.filter((sentence) => !needle || `${sentence.text} ${sentence.paper_title} ${sentence.category_name}`.toLowerCase().includes(needle)), [sentences, needle]);
  const filteredImages = useMemo(() => images.filter((image) => !needle || `${image.title} ${image.paper_title || ''} ${image.category_name || ''}`.toLowerCase().includes(needle)), [images, needle]);

  const sectionCounts = { papers: papers.length, sentences: sentences.length, images: images.length, latex: latexDocuments.length };
  const activeCategory = categories.find((c) => String(c.id) === String(categoryFilter));
  const searchEnabled = !activeProject || ['papers', 'sentences', 'images'].includes(activeTab);
  const searchPlaceholder = !activeProject ? 'Search projects' : activeTab === 'sentences' ? 'Search sentences' : activeTab === 'images' ? 'Search figures' : 'Search papers';

  if (loading || (isReaderRoute && projectLoading && !readerPaper)) {
    return <div className="app-loading"><BrandMark size={64} /><LoaderCircle className="spin" size={22} /><span>{isReaderRoute ? 'Opening paper…' : 'Opening Research Dome…'}</span></div>;
  }

  if (isReaderRoute) {
    if (!readerPaper) return <div className="app-loading app-loading-error"><LibraryBig size={24} /><span>{error || 'That paper could not be found in this project.'}</span><button className="button button-primary" onClick={() => router.push(`/projects/${activeProjectId}/papers`)}>Back to papers</button></div>;
    visitedRef.current.add(String(routePaperId));
    const openTabs = tabPapers.some((tab) => String(tab.id) === String(routePaperId)) ? tabPapers : [...tabPapers, { ...readerPaper, tabPage: readerPaper.startPage }];
    const backToLibrary = () => { router.push(`/projects/${activeProjectId}/papers`); reloadProjectData(); };
    return <div className="reader-shell">
      <ReaderTabs tabs={openTabs} activeId={routePaperId} papers={papers} onSelect={selectTab} onClose={closeTab} onOpen={(id) => openPaperById(id, 1)} onBack={backToLibrary} />
      <div className="reader-stack">
        {openTabs.filter((tab) => visitedRef.current.has(String(tab.id)) || String(tab.id) === String(routePaperId)).map((tab) => <Suspense key={tab.id} fallback={String(tab.id) === String(routePaperId) ? <div className="app-loading"><LoaderCircle className="spin" size={22} /><span>Loading PDF reader…</span></div> : null}>
          <PdfReader paper={tab} projectId={activeProjectId} active={String(tab.id) === String(routePaperId)} startPage={tab.tabPage || 1} onPageChange={rememberPage} onDeletePaper={() => deletePaper(tab, { fromReader: true })} onCloseTab={() => closeTab(tab.id)} onClose={backToLibrary} />
        </Suspense>)}
      </div>
      {toast && <div className="toast-message" role="status"><span><Check size={14} /></span>{toast}</div>}
    </div>;
  }

  return (
    <div className={`app-shell ${activeTab === 'latex' && activeProject ? 'latex-focus' : ''}`}>
      {sidebarOpen && <div className="sidebar-scrim" onClick={() => setSidebarOpen(false)} />}
      <aside className={`sidebar ${sidebarOpen ? 'open' : ''}`}>
        <button className="brand-lockup" onClick={goHome} aria-label="Research Dome home">
          <BrandMark size={44} />
          <span className="brand-text"><span className="brand-name">Research Dome</span><span className="brand-tagline">Library &amp; Study</span></span>
        </button>

        <nav className="side-scroll" aria-label="Workspace">
          <button className={`side-link ${!activeProject ? 'selected' : ''}`} onClick={goHome}><LayoutDashboard size={17} /><span>All projects</span><span className="side-count">{projects.length}</span></button>

          <div className="side-heading"><span>Projects</span><button title="New project" aria-label="New project" onClick={() => setShowCreateProject(true)}><Plus size={15} /></button></div>

          <div className="side-projects">
            {projects.map((project) => {
              const isActive = String(activeProjectId) === String(project.id);
              return (
                <div key={project.id} className={`side-project ${isActive ? 'active' : ''}`}>
                  <button className={`side-project-row ${isActive ? 'selected' : ''}`} onClick={() => openProject(project.id)}>
                    <InitialAvatar name={project.name} size="sm" />
                    <span className="side-project-name">{project.name}</span>
                    <ChevronRight size={14} className="side-chevron" />
                  </button>
                  {isActive && <div className="side-sub">
                    {SECTIONS.map((section) => { const Icon = section.icon; const count = sectionCounts[section.id]; return (
                      <button key={section.id} className={`side-sub-link ${activeTab === section.id ? 'selected' : ''}`} onClick={() => goTab(section.id)}>
                        <Icon size={15} /><span>{section.label}</span>{count > 0 && <span className="side-count">{count}</span>}
                      </button>
                    ); })}
                  </div>}
                </div>
              );
            })}
            {projects.length === 0 && <p className="side-empty">No projects yet.</p>}
          </div>
        </nav>

        <div className="sidebar-foot">
          <button className="side-link" onClick={() => setShowCreateProject(true)}><Plus size={17} /><span>New project</span></button>
          <button className="side-link" onClick={toggleTheme}>{theme === 'dark' ? <Sun size={17} /> : <Moon size={17} />}<span>{theme === 'dark' ? 'Parchment theme' : 'Ink theme'}</span></button>
          {isDesktop() && <button className="side-link" onClick={() => openDataFolder().catch((e) => setError(e.message || String(e)))}><FolderOpen size={17} /><span>Open data folder</span></button>}
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <button className="icon-button menu-button" aria-label="Open menu" onClick={() => setSidebarOpen(true)}><Menu size={19} /></button>
          <div className="breadcrumbs">
            <button onClick={goHome}>Projects</button>
            {activeProject && <><ChevronRight size={14} /><button onClick={() => goTab('papers')} className="crumb-project">{activeProject.name}</button><ChevronRight size={14} /><b>{SECTIONS.find((s) => s.id === activeTab)?.label}</b></>}
            {!activeProject && <><ChevronRight size={14} /><b>All projects</b></>}
          </div>
          <div className="topbar-actions">
            {searchEnabled && <label className="global-search"><Search size={16} />
              <input ref={searchRef} value={query} onChange={(e) => setQuery(e.target.value)} placeholder={searchPlaceholder} aria-label={searchPlaceholder} />
              {query ? <button type="button" className="search-clear" aria-label="Clear search" onClick={() => setQuery('')}><X size={14} /></button> : <kbd>{SEARCH_HINT}</kbd>}
            </label>}
            {activeProject ? <button className="button button-primary" onClick={startUpload}><UploadCloud size={16} /> Upload papers</button> : <button className="button button-primary" onClick={() => setShowCreateProject(true)}><Plus size={16} /> New project</button>}
          </div>
        </header>

        {error && <div className="global-error" role="alert"><span>{error}</span><button onClick={() => setError('')} aria-label="Dismiss error"><X size={15} /></button></div>}

        {!activeProject ? <section className="page">
          <div className="page-head">
            <div><h1>Projects</h1><p>Each project keeps its own papers, saved evidence, figures and LaTeX drafts together.</p></div>
            <img className="page-medallion" src={iconUrl} alt="" draggable="false" />
          </div>
          {projects.length === 0 ? <EmptyState icon={LibraryBig} title="Create your first project" action={<button className="button button-primary" onClick={() => setShowCreateProject(true)}><Plus size={16} /> New project</button>}>
            A project is one research topic. Add categories, upload PDFs, highlight sentences while you read, crop figures, and draft your paper in LaTeX — all in one place.
          </EmptyState> : filteredProjects.length === 0 ? <EmptyState icon={Search} title="No projects match your search">Try a different name or clear the search box.</EmptyState> : <div className="project-card-grid">
            {filteredProjects.map((project) => <button key={project.id} className="project-card" onClick={() => openProject(project.id)}>
              <div className="project-card-top"><InitialAvatar name={project.name} /><ArrowUpRight size={18} className="project-card-arrow" /></div>
              <h3>{project.name}</h3>
              <p>{project.description || 'No description yet.'}</p>
              <div className="project-card-foot"><span>Created {formatDate(project.created_at)}</span></div>
            </button>)}
            {!needle && <button className="project-card project-card-new" onClick={() => setShowCreateProject(true)}><span className="new-plus"><Plus size={20} /></span><b>New project</b></button>}
          </div>}
        </section> : <section className={`page ${activeTab === 'latex' ? 'page-latex' : ''}`}>
          {activeTab !== 'latex' && <div className="page-head project-head">
            <div className="project-head-main">
              <InitialAvatar name={activeProject.name} size="lg" />
              <div><h1>{activeProject.name}</h1>{activeProject.description && <p>{activeProject.description}</p>}</div>
            </div>
            <button type="button" className="button button-danger button-sm" onClick={() => deleteProject(activeProject)}><Trash2 size={14} /> Delete project</button>
          </div>}

          {activeTab !== 'latex' && <div className="stat-strip">
            {SECTIONS.map((section) => { const Icon = section.icon; return <button key={section.id} className={`stat ${activeTab === section.id ? 'active' : ''}`} onClick={() => goTab(section.id)}><Icon size={16} /><span className="stat-label">{section.label}</span><b>{sectionCounts[section.id]}</b></button>; })}
          </div>}

          {activeTab === 'papers' && <div className="papers-page">
            <div className="chip-bar" role="tablist" aria-label="Filter by category">
              <button className={`chip ${categoryFilter === 'all' ? 'active' : ''}`} onClick={() => setCategoryFilter('all')}>All papers <span>{papers.length}</span></button>
              {categories.map((category) => <div key={category.id} className={`category-chip-group ${String(categoryFilter) === String(category.id) ? 'active' : ''}`}>
                <button title={category.description || category.name} className={`chip ${String(categoryFilter) === String(category.id) ? 'active' : ''}`} onClick={() => setCategoryFilter(String(category.id))}>{category.name} <span>{category.paper_count ?? papers.filter((p) => String(p.category_id) === String(category.id)).length}</span></button>
                <button type="button" className="category-chip-delete" title={`Delete category ${category.name}`} aria-label={`Delete category ${category.name}`} onClick={(event) => { event.stopPropagation(); setCategoryToDelete(category); setCategoryDeleteStrategy('keep'); }}><Trash2 size={13} /></button>
              </div>)}
              {papers.some((paper) => paper.category_id == null) && <button className={`chip ${categoryFilter === 'uncategorized' ? 'active' : ''}`} onClick={() => setCategoryFilter('uncategorized')}>Uncategorized <span>{papers.filter((paper) => paper.category_id == null).length}</span></button>}
              <button className="chip chip-add" onClick={() => setShowCreateCategory(true)}><Plus size={14} /> Category</button>
            </div>
            {activeCategory?.description && <p className="category-description">{activeCategory.description}</p>}
            {tabPapers.length > 0 && <div className="open-tabs-strip" aria-label="Papers open in tabs">
              <span className="open-tabs-label"><FileText size={14} /> Open in tabs</span>
              {tabPapers.map((paper) => <span key={paper.id} className="open-tab-chip">
                <button className="open-tab-chip-main" title={paper.title} onClick={() => openPaperById(paper.id, paper.tabPage)}><b>[{paper.citation_number}]</b> {paper.title}<small>p. {paper.tabPage}</small></button>
                <button className="open-tab-chip-close" aria-label={`Close ${paper.title}`} title="Close tab" onClick={() => closeTab(paper.id)}><X size={12} /></button>
              </span>)}
            </div>}

            {projectLoading ? <div className="table-card skeleton-list" aria-busy="true">{[0, 1, 2, 3].map((i) => <div key={i} className="skeleton-row"><span /><span /><span /></div>)}</div>
              : categories.length === 0 ? <EmptyState icon={FolderPlus} title="Start with a category" action={<button className="button button-primary" onClick={() => setShowCreateCategory(true)}><Plus size={16} /> Create a category</button>}>
                Categories organise the literature in this project, for example Related Work, Methods, Datasets or Evaluation. Every paper you upload belongs to one.
              </EmptyState>
              : filteredPapers.length === 0 ? <EmptyState icon={FileText} title={papers.length === 0 ? 'No papers yet' : 'No papers match'} action={papers.length === 0 ? <button className="button button-primary" onClick={() => setShowUpload(true)}><UploadCloud size={16} /> Upload PDFs</button> : null}>
                {papers.length === 0 ? 'Upload your first PDF and choose the category it belongs to.' : 'Try another category or clear the search.'}
              </EmptyState>
              : <div className="table-card"><table className="papers-table">
                <thead><tr><th className="col-ref">Ref</th><th>Paper</th><th className="col-cat">Category</th><th className="col-date">Uploaded</th><th className="col-size">Size</th><th className="col-notes">Notes</th><th className="col-act" /></tr></thead>
                <tbody>{filteredPapers.map((paper) => <tr key={paper.id} onClick={() => openPaperById(paper.id, 1)}>
                  <td className="col-ref"><span className="paper-reference">[{paper.citation_number}]</span></td>
                  <td><button className="paper-title-button" onClick={(e) => { e.stopPropagation(); openPaperById(paper.id, 1); }}><span className="paper-file-icon"><FileText size={17} /></span><span className="paper-title-text"><b>{paper.title}</b><small>{paper.original_filename}</small></span></button></td>
                  <td className="col-cat"><select className="paper-category-select" aria-label={`Category for ${paper.title}`} value={paper.category_id == null ? 'uncategorized' : String(paper.category_id)} disabled={movingPaperId === paper.id} onClick={(event) => event.stopPropagation()} onChange={(event) => movePaperToCategory(paper, event.target.value)}>{categories.map((category) => <option key={category.id} value={String(category.id)}>{category.name}</option>)}<option value="uncategorized">Uncategorized</option></select></td>
                  <td className="col-date td-muted">{formatDate(paper.uploaded_at)}</td>
                  <td className="col-size td-muted">{formatSize(paper.file_size)}</td>
                  <td className="col-notes td-muted"><span className="annotation-count" title="Saved annotations"><Quote size={13} /> {paper.annotation_count || 0}</span></td>
                  <td className="col-act"><div className="paper-row-actions"><button className="table-open-button" onClick={(event) => { event.stopPropagation(); openPaperById(paper.id, 1); }}>Read <ArrowRight size={14} /></button><button type="button" className="mini-icon-button danger paper-delete-button" title={`Delete ${paper.title}`} aria-label={`Delete ${paper.title}`} onClick={(event) => { event.stopPropagation(); deletePaper(paper); }}><Trash2 size={14} /></button></div></td>
                </tr>)}</tbody>
              </table></div>}
          </div>}

          {activeTab === 'sentences' && <div className="sentences-page">
            <div className="list-head"><h2>Important sentences</h2><span className="subtle-count">{filteredSentences.length} saved</span></div>
            {filteredSentences.length === 0 ? <EmptyState icon={Quote} title={sentences.length ? 'No sentences match' : 'No saved sentences yet'} action={!sentences.length ? <button className="button button-muted" onClick={() => goTab('papers')}>Open a paper <ArrowRight size={15} /></button> : null}>
              {sentences.length ? 'Try a different search.' : 'While reading a PDF, select text and choose “Save selected sentence”. It will appear here with its citation and page.'}
            </EmptyState> : <div className="sentence-list">{filteredSentences.map((sentence) => <article className="sentence-card" key={sentence.id} style={{ '--mark': sentence.color || 'var(--accent)' }}>
              <p className="sentence-quote">{sentence.text}</p>
              <div className="sentence-foot">
                <div className="sentence-meta">
                  <button className="cite-pill" title={`${sentence.paper_title} (${sentence.category_name})`} onClick={() => openPaperById(sentence.paper_id, sentence.page_number)}>[{sentence.citation_number}]</button>
                  <button className="sentence-source" onClick={() => openPaperById(sentence.paper_id, sentence.page_number)}>{sentence.paper_title}</button>
                  <span className="sentence-dot" />
                  <span>Page {sentence.page_number}</span>
                  {sentence.category_name && <><span className="sentence-dot" /><span>{sentence.category_name}</span></>}
                </div>
                <div className="sentence-actions">
                  <button className="mini-icon-button" title="Copy with citation" aria-label="Copy with citation" onClick={() => copySentence(sentence)}><Copy size={15} /></button>
                  <button className="mini-icon-button" title="Open source page" aria-label="Open source page" onClick={() => openPaperById(sentence.paper_id, sentence.page_number)}><ArrowUpRight size={16} /></button>
                  <button className="mini-icon-button danger" title="Delete Important Sentence" aria-label="Delete Important Sentence" onClick={() => deleteSentence(sentence)}><Trash2 size={14} /></button>
                </div>
              </div>
            </article>)}</div>}
          </div>}

          {activeTab === 'images' && <div className="images-page">
            <div className="list-head"><h2>Figures</h2><span className="subtle-count">{filteredImages.length} saved</span></div>
            {filteredImages.length === 0 ? <EmptyState icon={ImageIcon} title={images.length ? 'No figures match' : 'No figures yet'} action={!images.length ? <button className="button button-muted" onClick={() => goTab('papers')}>Open a paper <ArrowRight size={15} /></button> : null}>
              {images.length ? 'Try a different search.' : 'Open a paper, choose “Crop image”, drag around a diagram and confirm. Only confirmed crops are collected here.'}
            </EmptyState> : <div className="important-images-grid">{filteredImages.map((image) => <article className="important-image-card" key={image.id}>
              <button className="clipping-image-button" onClick={() => setPreviewImage(image)} aria-label={`Preview ${image.title}`}><img src={fileUrl(`/images/${image.id}/file`)} alt={image.title} loading="lazy" /></button>
              <div className="important-image-info">
                <div>
                  <h3>{image.title}</h3>
                  <p>{image.paper_title ? <button className="plain-link" onClick={() => openPaperById(image.paper_id, image.page_number || 1)}>[{papers.find((paper) => paper.id === image.paper_id)?.citation_number ?? '?'}] {image.paper_title}</button> : 'Uploaded clipping'}{image.page_number ? ` · p. ${image.page_number}` : ''}</p>
                </div>
                <button className="mini-icon-button danger" title="Delete figure" aria-label="Delete figure" onClick={() => deleteImage(image)}><Trash2 size={15} /></button>
              </div>
            </article>)}</div>}
          </div>}

          {activeTab === 'latex' && <Suspense fallback={<div className="app-loading app-loading-inline"><LoaderCircle className="spin" size={22} /><span>Opening LaTeX Studio…</span></div>}><LaTeXStudio projectId={activeProject.id} projectName={activeProject.name} documents={latexDocuments} onDocumentsChanged={reloadProjectData} onBack={() => router.push(`/projects/${activeProject.id}/papers`)} /></Suspense>}
        </section>}
      </main>

      {showCreateProject && <Modal onClose={() => setShowCreateProject(false)} icon={LibraryBig} title="New project" subtitle="One research topic: its papers, evidence, figures and LaTeX drafts.">
        <form onSubmit={createProject}>
          <label className="field-label">Name<input autoFocus value={projectName} onChange={(e) => setProjectName(e.target.value)} placeholder="e.g. Quantum Computing in Cyber Security" required maxLength={200} /></label>
          <label className="field-label">Description <span className="optional-label">Optional</span><textarea value={projectDescription} onChange={(e) => setProjectDescription(e.target.value)} placeholder="What question are you investigating?" rows={3} /></label>
          <div className="modal-actions"><button type="button" className="button button-muted" onClick={() => setShowCreateProject(false)}>Cancel</button><button type="submit" className="button button-primary" disabled={creating || !projectName.trim()}>{creating ? <LoaderCircle size={15} className="spin" /> : <Plus size={15} />} Create project</button></div>
        </form>
      </Modal>}

      {showCreateCategory && activeProject && <Modal onClose={() => setShowCreateCategory(false)} icon={FolderPlus} title="New category" subtitle={`Group papers in ${activeProject.name}.`}>
        <form onSubmit={createCategory}>
          <label className="field-label">Name<input autoFocus value={categoryName} onChange={(e) => setCategoryName(e.target.value)} placeholder="e.g. Quantum Feature Mapping" required maxLength={160} /></label>
          <label className="field-label">Description <span className="optional-label">Optional</span><textarea value={categoryDescription} onChange={(e) => setCategoryDescription(e.target.value)} placeholder="What kinds of papers belong here?" rows={3} /></label>
          <div className="category-example-row"><span>Quick fill</span><button type="button" onClick={() => setCategoryName('Related Work')}>Related Work</button><button type="button" onClick={() => setCategoryName('Methods')}>Methods</button><button type="button" onClick={() => setCategoryName('Evaluation')}>Evaluation</button></div>
          <div className="modal-actions"><button type="button" className="button button-muted" onClick={() => setShowCreateCategory(false)}>Cancel</button><button type="submit" className="button button-primary" disabled={creating || !categoryName.trim()}>{creating ? <LoaderCircle size={15} className="spin" /> : <Plus size={15} />} Create category</button></div>
        </form>
      </Modal>}

      {categoryToDelete && <Modal onClose={() => { if (!deletingCategory) setCategoryToDelete(null); }} icon={Trash2} title={`Delete “${categoryToDelete.name}”?`} subtitle="Choose what should happen to the papers in this category.">
        <div className="category-delete-summary"><b>{categoryToDelete.paper_count ?? papers.filter((paper) => String(paper.category_id) === String(categoryToDelete.id)).length} paper(s)</b><span>Category descriptions and the category itself will be removed. This action cannot be undone.</span></div>
        <label className={`category-delete-option ${categoryDeleteStrategy === 'keep' ? 'selected' : ''}`}><input type="radio" name="category-delete-strategy" value="keep" checked={categoryDeleteStrategy === 'keep'} onChange={() => setCategoryDeleteStrategy('keep')} /><span><b>Keep the research papers</b><small>Move them to <strong>Uncategorized</strong>. Their PDFs and annotations remain.</small></span></label>
        <label className={`category-delete-option destructive ${categoryDeleteStrategy === 'delete' ? 'selected' : ''}`}><input type="radio" name="category-delete-strategy" value="delete" checked={categoryDeleteStrategy === 'delete'} onChange={() => setCategoryDeleteStrategy('delete')} /><span><b>Delete the category and its papers</b><small>Permanently remove all papers and their annotations. Saved crops remain in Important Images without their source link.</small></span></label>
        <div className="modal-actions"><button type="button" className="button button-muted" onClick={() => setCategoryToDelete(null)} disabled={deletingCategory}>Cancel</button><button type="button" className="button button-danger" onClick={confirmDeleteCategory} disabled={deletingCategory}>{deletingCategory ? <LoaderCircle size={15} className="spin" /> : <Trash2 size={15} />} Delete category</button></div>
      </Modal>}

      {showUpload && activeProject && <Modal wide onClose={() => setShowUpload(false)} icon={UploadCloud} title="Upload papers" subtitle={`Add PDFs to ${activeProject.name}. Files are stored on your disk.`}>
        <form onSubmit={uploadResearchPapers}>
          <label className="field-label">Category<select value={uploadCategoryId} onChange={(e) => setUploadCategoryId(e.target.value)} required><option value="" disabled>Select a category</option>{categories.map((category) => <option value={category.id} key={category.id}>{category.name}</option>)}</select></label>
          <label className={`upload-dropzone ${dragOver ? 'dragging' : ''}`} onDragOver={(e) => { e.preventDefault(); setDragOver(true); }} onDragLeave={() => setDragOver(false)} onDrop={(e) => { e.preventDefault(); setDragOver(false); addUploadFiles(e.dataTransfer.files); }}>
            <input type="file" accept="application/pdf,.pdf" multiple onChange={(e) => { addUploadFiles(e.target.files); e.target.value = ''; }} />
            <span className="upload-drop-icon"><UploadCloud size={22} /></span>
            <b>Drop PDFs here or click to browse</b>
            <span>Multiple files supported · up to 100 MB each</span>
          </label>
          {uploadFiles.length > 0 && <ul className="upload-file-list">{uploadFiles.map((file) => <li key={`${file.name}:${file.size}`}><FileText size={16} /><span className="upload-file-name">{file.name}</span><small>{formatSize(file.size)}</small><button type="button" className="mini-icon-button" aria-label={`Remove ${file.name}`} onClick={() => setUploadFiles((current) => current.filter((item) => item !== file))}><X size={14} /></button></li>)}</ul>}
          {uploadProgress && <div className="upload-progress"><LoaderCircle className="spin" size={15} /> {uploadProgress}</div>}
          <div className="modal-actions"><button type="button" className="button button-muted" onClick={() => setShowUpload(false)}>Cancel</button><button type="submit" className="button button-primary" disabled={creating || !uploadFiles.length || !uploadCategoryId}>{creating ? <LoaderCircle size={15} className="spin" /> : <UploadCloud size={15} />} Upload {uploadFiles.length || ''} paper{uploadFiles.length === 1 ? '' : 's'}</button></div>
        </form>
      </Modal>}

      {previewImage && <div className="modal-scrim image-lightbox" onMouseDown={(e) => e.target === e.currentTarget && setPreviewImage(null)}><div className="image-lightbox-card"><button className="modal-close" onClick={() => setPreviewImage(null)} aria-label="Close"><X size={18} /></button><div className="lightbox-image"><img src={fileUrl(`/images/${previewImage.id}/file`)} alt={previewImage.title} /></div><div className="lightbox-info"><div><h3>{previewImage.title}</h3><p>{previewImage.paper_title || 'Project image clipping'}{previewImage.page_number ? ` · Page ${previewImage.page_number}` : ''}</p></div><button className="button button-muted" onClick={() => saveImageAs(previewImage)}><ArrowDownToLine size={15} /> Save as…</button></div></div></div>}
      {toast && <div className="toast-message" role="status"><span><Check size={14} /></span>{toast}</div>}
    </div>
  );
}
