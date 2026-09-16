import { spawn } from 'node:child_process';

function checkedUrl(address: string): URL {
  if (typeof address !== 'string' || address.length > 4096 || !/^[\x21-\x7e]+$/.test(address) || address.includes('\\')) throw new Error('unsupported-browser-url');
  const authority = /^(https?):\/\/([^/?#]+)/.exec(address)?.[2];
  if (!authority || authority.includes('@')) throw new Error('unsupported-browser-url');
  let url: URL;
  try { url = new URL(address); } catch { throw new Error('unsupported-browser-url'); }
  if (url.username || url.password || !(url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)))) throw new Error('unsupported-browser-url');
  return url;
}

/** Internal seam for deterministic process tests. Not part of the package API. */
export function createBrowserOpener(spawnProcess: typeof spawn = spawn, timeoutMs = 10_000): (address: string) => Promise<void> {
  let opening = false;
  return async address => {
    const url = checkedUrl(address);
    if (opening) throw new Error('browser-open-busy');
    opening = true;
    try {
      const program = process.platform === 'darwin' ? '/usr/bin/open' : process.platform === 'win32' ? 'rundll32.exe' : 'xdg-open';
      const argv = process.platform === 'win32' ? ['url.dll,FileProtocolHandler', url.href] : [url.href];
      await new Promise<void>((resolve, reject) => {
        let child;
        try { child = spawnProcess(program, argv, { stdio: 'ignore', windowsHide: true, shell: false }); }
        catch { reject(new Error('browser-open-failed')); return; }
        let failed = false;
        let timedOut = false;
        const timer = setTimeout(() => { timedOut = true; child.kill('SIGKILL'); }, timeoutMs);
        child.once('error', () => { failed = true; });
        // Keep ownership until close, including after timeout. Never retry a
        // navigation whose launcher may already have reached the browser.
        child.once('close', code => {
          clearTimeout(timer);
          if (timedOut) reject(new Error('browser-open-timeout'));
          else if (failed || code !== 0) reject(new Error('browser-open-failed'));
          else resolve();
        });
      });
    } finally { opening = false; }
  };
}

/**
 * Explicit human browser handoff. No launch on construction or import. Accepts
 * HTTPS and the existing loopback HTTP dashboard contract, never credentials.
 * Product-owned public routing parameters are allowed; personal data is not.
 * Success means the launcher exited, not that the person viewed or approved UI.
 */
export const openBrowser = createBrowserOpener();
