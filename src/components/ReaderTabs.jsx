import { useEffect, useMemo, useRef, useState } from 'react';
import { LibraryBig, Plus, Search, X } from 'lucide-react';

/**
 * Browser-style tab strip for the PDF reader. Every open paper keeps its own tab,
 * so you can move between papers without going back to the library.
 */
export default function ReaderTabs({ tabs, activeId, papers, onSelect, onClose, onOpen, onBack }) {
  const [picking, setPicking] = useState(false);
  const [filter, setFilter] = useState('');
  const pickerRef = useRef(null);
  const activeRef = useRef(null);

  useEffect(() => { activeRef.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }, [activeId, tabs.length]);

  useEffect(() => {
    if (!picking) return undefined;
    const away = (event) => { if (pickerRef.current && !pickerRef.current.contains(event.target)) setPicking(false); };
    document.addEventListener('mousedown', away);
    return () => document.removeEventListener('mousedown', away);
  }, [picking]);

  const openIds = useMemo(() => new Set(tabs.map((tab) => String(tab.id))), [tabs]);
  const needle = filter.trim().toLowerCase();
  const candidates = useMemo(() => papers
    .filter((paper) => !needle || `${paper.title} ${paper.original_filename} ${paper.category_name || ''}`.toLowerCase().includes(needle))
    .sort((a, b) => Number(openIds.has(String(a.id))) - Number(openIds.has(String(b.id))) || a.citation_number - b.citation_number), [papers, needle, openIds]);

  function choose(paper) {
    setPicking(false);
    setFilter('');
    onOpen(paper.id);
  }

  return (
    <div className="reader-tabs" role="tablist" aria-label="Open papers">
      <button className="reader-tabs-home" onClick={onBack} title="Back to the library (your tabs stay open)"><LibraryBig size={16} /><span>Library</span></button>
      <div className="reader-tabs-scroll">
        {tabs.map((tab) => {
          const active = String(tab.id) === String(activeId);
          return (
            <div
              key={tab.id}
              ref={active ? activeRef : null}
              role="tab"
              aria-selected={active}
              tabIndex={0}
              className={`reader-tab ${active ? 'active' : ''}`}
              title={tab.title}
              onClick={() => onSelect(tab.id)}
              onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onSelect(tab.id); } }}
              onMouseDown={(event) => { if (event.button === 1) event.preventDefault(); }}
              onAuxClick={(event) => { if (event.button === 1) { event.preventDefault(); onClose(tab.id); } }}
            >
              <span className="reader-tab-ref">[{tab.citation_number}]</span>
              <span className="reader-tab-title">{tab.title}</span>
              <button className="reader-tab-close" aria-label={`Close ${tab.title}`} title="Close tab (Ctrl+W)" onClick={(event) => { event.stopPropagation(); onClose(tab.id); }}><X size={13} /></button>
            </div>
          );
        })}
      </div>
      <div className="reader-tabs-new" ref={pickerRef}>
        <button className="reader-tab-add" onClick={() => setPicking((value) => !value)} title="Open another paper in a new tab" aria-label="Open another paper in a new tab" aria-expanded={picking}><Plus size={16} /></button>
        {picking && <div className="reader-picker" role="dialog" aria-label="Open a paper in a new tab">
          <label className="reader-picker-search"><Search size={14} /><input autoFocus value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Find a paper…" onKeyDown={(event) => {
            if (event.key === 'Escape') { event.nativeEvent.stopPropagation(); setPicking(false); }
            if (event.key === 'Enter' && candidates[0]) choose(candidates[0]);
          }} /></label>
          <div className="reader-picker-list">
            {candidates.length === 0 && <p className="reader-picker-empty">No matching papers.</p>}
            {candidates.map((paper) => <button key={paper.id} className="reader-picker-item" onClick={() => choose(paper)}>
              <span className="reader-tab-ref">[{paper.citation_number}]</span>
              <span className="reader-picker-text"><b>{paper.title}</b><small>{paper.category_name || 'Uncategorized'}</small></span>
              {openIds.has(String(paper.id)) && <em>open</em>}
            </button>)}
          </div>
        </div>}
      </div>
    </div>
  );
}
