// Talks to the embedded Rust + SQLite service.
//
// Desktop: the Tauri shell starts the service on a private loopback port and tells
// us its address and a per-launch secret through `backend_info`.
// Browser (development only): falls back to `cargo run -p research-dome-core --bin dome-server`.

let apiBase = (import.meta.env.VITE_API_URL || 'http://127.0.0.1:8000/api').replace(/\/$/, '');
let token = import.meta.env.VITE_API_TOKEN || 'dev-token';
let dataDir = '';

export const isDesktop = () => typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);

export async function initBackend() {
  if (isDesktop()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const info = await invoke('backend_info');
    apiBase = info.apiBase.replace(/\/$/, '');
    token = info.token;
    dataDir = info.dataDir;
  }
  // Fail early (with a readable message) if the service is not answering.
  await api('/health');
}

export const getDataDir = () => dataDir;

export async function api(path, options = {}) {
  const headers = new Headers(options.headers || {});
  headers.set('X-Dome-Token', token);
  if (options.body && !(options.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }
  let response;
  try {
    response = await fetch(`${apiBase}${path}`, { ...options, headers, cache: 'no-store' });
  } catch {
    throw new Error('Could not reach the Research Dome service. Please restart the app.');
  }
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try {
      const payload = await response.json();
      message = payload.detail || payload.message || message;
    } catch { /* keep default message */ }
    throw new Error(message);
  }
  if (response.status === 204) return null;
  return response.json();
}

/** URL for <img>, PDF.js and downloads. Carries the secret as a query parameter. */
export function fileUrl(path) {
  const separator = path.includes('?') ? '&' : '?';
  return `${apiBase}${path}${separator}token=${encodeURIComponent(token)}`;
}

export function upload(path, formData) {
  return api(path, { method: 'POST', body: formData });
}

/**
 * Save an API download to disk. On desktop this opens the native "Save as…"
 * dialog; resolves to the saved path, or null if the user cancelled.
 */
export async function downloadFile(path, suggestedName) {
  if (isDesktop()) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke('save_download', { path, suggestedName });
  }
  const response = await fetch(fileUrl(path));
  if (!response.ok) throw new Error(`Download failed (${response.status})`);
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = suggestedName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
  return suggestedName;
}

export async function openDataFolder() {
  if (!isDesktop()) return;
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('open_data_folder');
}

export function safeName(name = 'download') {
  return name.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^[._]+|[._]+$/g, '') || 'download';
}
