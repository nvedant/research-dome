import { useEffect, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import WorkspaceApp from './components/WorkspaceApp.jsx';
import { ConfirmProvider } from './components/Confirm.jsx';
import { initBackend } from './lib/backend.js';
import iconUrl from './assets/icon-512.png';

function Splash({ children }) {
  return (
    <div className="splash">
      <div className="splash-medallion"><img src={iconUrl} alt="Research Dome" /></div>
      <h1 className="splash-title">Research Dome</h1>
      {children}
    </div>
  );
}

export default function App() {
  const [state, setState] = useState({ status: 'starting', error: '' });

  useEffect(() => {
    let alive = true;
    initBackend()
      .then(() => alive && setState({ status: 'ready', error: '' }))
      .catch((error) => alive && setState({ status: 'failed', error: error.message || String(error) }));
    return () => { alive = false; };
  }, []);

  if (state.status === 'starting') {
    return <Splash><div className="splash-status"><LoaderCircle className="spin" size={16} /> Opening your library…</div></Splash>;
  }
  if (state.status === 'failed') {
    return (
      <Splash>
        <div className="splash-error" role="alert">
          <b>Research Dome could not start its local library.</b>
          <span>{state.error}</span>
          <button className="button button-primary" onClick={() => window.location.reload()}>Try again</button>
        </div>
      </Splash>
    );
  }
  return <ConfirmProvider><WorkspaceApp /></ConfirmProvider>;
}
