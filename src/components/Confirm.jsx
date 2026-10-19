// In-app confirmation dialogs. Native window.confirm() is not reliable inside every
// desktop webview, and these match the Research Dome look.
import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { AlertTriangle } from 'lucide-react';

const ConfirmContext = createContext(null);

export function ConfirmProvider({ children }) {
  const [dialog, setDialog] = useState(null);
  const resolver = useRef(null);

  const confirm = useCallback((message, { title = 'Please confirm', confirmLabel = 'Confirm', tone = 'danger' } = {}) => (
    new Promise((resolve) => {
      resolver.current = resolve;
      setDialog({ message, title, confirmLabel, tone });
    })
  ), []);

  const close = useCallback((answer) => {
    setDialog(null);
    resolver.current?.(answer);
    resolver.current = null;
  }, []);

  useEffect(() => {
    if (!dialog) return undefined;
    const onKey = (event) => {
      if (event.key === 'Escape') close(false);
      if (event.key === 'Enter') close(true);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [dialog, close]);

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      {dialog && (
        <div className="modal-scrim confirm-scrim" onMouseDown={(e) => e.target === e.currentTarget && close(false)}>
          <div className="modal-card confirm-card" role="alertdialog" aria-modal="true" aria-label={dialog.title}>
            <div className="modal-head">
              <div className={`modal-icon ${dialog.tone === 'danger' ? 'modal-icon-danger' : ''}`}><AlertTriangle size={20} /></div>
              <div><h2>{dialog.title}</h2><p>{dialog.message}</p></div>
            </div>
            <div className="modal-actions">
              <button className="button button-muted" onClick={() => close(false)}>Cancel</button>
              <button autoFocus className={`button ${dialog.tone === 'danger' ? 'button-danger' : 'button-primary'}`} onClick={() => close(true)}>{dialog.confirmLabel}</button>
            </div>
          </div>
        </div>
      )}
    </ConfirmContext.Provider>
  );
}

export function useConfirm() {
  const confirm = useContext(ConfirmContext);
  if (!confirm) throw new Error('useConfirm must be used inside <ConfirmProvider>');
  return confirm;
}
