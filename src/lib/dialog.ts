/**
 * Browser-friendly facade over `@tauri-apps/plugin-dialog`.
 *
 * - In tauri mode, calls the real plugin (full native dialogs).
 * - In http mode, falls back to browser primitives:
 *   - `confirm` / `message` / `ask` → `window.confirm` / `window.alert`
 *   - `open({ directory: false })` → `<input type="file">` returning a File
 *     blob plus a synthetic absolute path. Server-side commands that expect
 *     a real on-disk path (e.g. `stage_plugin_install`) won't work in
 *     browser mode without a future upload endpoint.
 */

import { TRANSPORT_MODE } from '../api/transport';

type ConfirmOptions = { title?: string; kind?: 'info' | 'warning' | 'error' };

export async function confirm(message: string, options?: ConfirmOptions | string): Promise<boolean> {
  if (TRANSPORT_MODE === 'tauri') {
    const mod = await import('@tauri-apps/plugin-dialog');
    return mod.confirm(message, options as never);
  }
  const title = typeof options === 'string' ? options : options?.title;
  return window.confirm(title ? `${title}\n\n${message}` : message);
}

export async function message(message: string, options?: ConfirmOptions | string): Promise<void> {
  if (TRANSPORT_MODE === 'tauri') {
    const mod = await import('@tauri-apps/plugin-dialog');
    await mod.message(message, options as never);
    return;
  }
  const title = typeof options === 'string' ? options : options?.title;
  window.alert(title ? `${title}\n\n${message}` : message);
}

export async function ask(message: string, options?: ConfirmOptions | string): Promise<boolean> {
  if (TRANSPORT_MODE === 'tauri') {
    const mod = await import('@tauri-apps/plugin-dialog');
    return mod.ask(message, options as never);
  }
  const title = typeof options === 'string' ? options : options?.title;
  return window.confirm(title ? `${title}\n\n${message}` : message);
}

type OpenOptions = {
  directory?: boolean;
  multiple?: boolean;
  filters?: Array<{ name: string; extensions: string[] }>;
  title?: string;
};

/**
 * In browser mode this surfaces a file picker that returns the *browser-side*
 * file. The caller will receive the File object's `path` is synthesised — the
 * Rust backend cannot read the file directly. Callers that need a server-side
 * path should switch to a server-side file-tree browser (Phase 3).
 */
export async function open(
  options?: OpenOptions,
): Promise<string | string[] | null | { name: string; data: ArrayBuffer }> {
  if (TRANSPORT_MODE === 'tauri') {
    const mod = await import('@tauri-apps/plugin-dialog');
    return mod.open(options as never) as never;
  }

  if (options?.directory) {
    window.alert('Directory pickers are not supported in browser mode.');
    return null;
  }

  return await new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    if (options?.multiple) input.multiple = true;
    if (options?.filters?.length) {
      input.accept = options.filters
        .flatMap((f) => f.extensions.map((e) => `.${e.replace(/^\./, '')}`))
        .join(',');
    }
    input.onchange = async () => {
      const files = Array.from(input.files ?? []);
      if (!files.length) {
        resolve(null);
        return;
      }
      const f = files[0];
      // Browser cannot resolve a real OS path; surface the file name with a
      // sentinel prefix so callers fail loudly if they pass it through to a
      // backend that expects a real path.
      resolve(`browser-upload://${f.name}`);
    };
    input.click();
  });
}
