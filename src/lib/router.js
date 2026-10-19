// Tiny hash router (#/projects/1/papers). Hash routing needs no server support,
// which is exactly what a packaged desktop app serves its UI from.
import { useEffect, useMemo, useState } from 'react';

const read = () => {
  const hash = window.location.hash.replace(/^#/, '');
  return (hash.split('?')[0] || '/');
};

export function usePathname() {
  const [path, setPath] = useState(read);
  useEffect(() => {
    const onChange = () => setPath(read());
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return path;
}

export function useRouter() {
  return useMemo(() => ({
    push: (to) => { window.location.hash = to; },
    replace: (to) => { window.location.replace(`#${to}`); },
  }), []);
}
