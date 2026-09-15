import { homedir } from 'node:os';
import { posix, win32 } from 'node:path';
import { CompanionError } from './errors.js';
const { isAbsolute, join } = posix;

export const TARGETS = [
  'aarch64-apple-darwin', 'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
] as const;
export type PlatformTarget = typeof TARGETS[number];
export interface PlatformOptions {
  platform?: NodeJS.Platform;
  arch?: string;
  env?: NodeJS.ProcessEnv;
  home?: string;
}
export function resolveTarget(options: PlatformOptions = {}): PlatformTarget {
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const cpu = arch === 'arm64' ? 'aarch64' : arch === 'x64' ? 'x86_64' : undefined;
  const os = { darwin: 'apple-darwin', win32: 'pc-windows-msvc', linux: 'unknown-linux-gnu' }[platform as 'darwin' | 'win32' | 'linux'];
  if (!cpu || !os) throw new CompanionError('unsupported_target', `Unsupported companion target: ${platform}/${arch}.`, 'Use the product CLI without its optional tray companion.');
  return `${cpu}-${os}` as PlatformTarget;
}
export function userPaths(options: PlatformOptions = {}) {
  const platform = options.platform ?? process.platform;
  const env = options.env ?? process.env;
  const home = options.home ?? homedir();
  const path = platform === 'win32' ? win32 : { isAbsolute, join };
  if (!path.isAbsolute(home)) throw new CompanionError('unsafe_path', 'The home directory must be absolute.');
  const absolute = (value: string | undefined, fallback: string) => value && path.isAbsolute(value) ? value : fallback;
  if (platform === 'darwin') return {
    dataDir: join(home, 'Library', 'Application Support', 'hraness-companion'),
    configDir: join(home, 'Library', 'Application Support', 'hraness-companion'),
    cacheDir: join(home, 'Library', 'Caches', 'hraness-companion'),
  };
  if (platform === 'win32') return {
    dataDir: win32.join(absolute(env.LOCALAPPDATA, win32.join(home, 'AppData', 'Local')), 'hraness-companion'),
    cacheDir: win32.join(absolute(env.LOCALAPPDATA, win32.join(home, 'AppData', 'Local')), 'hraness-companion', 'Cache'),
    configDir: win32.join(absolute(env.APPDATA, win32.join(home, 'AppData', 'Roaming')), 'hraness-companion'),
  };
  if (platform === 'linux') return {
    dataDir: join(absolute(env.XDG_DATA_HOME, join(home, '.local', 'share')), 'hraness-companion'),
    configDir: join(absolute(env.XDG_CONFIG_HOME, join(home, '.config')), 'hraness-companion'),
    cacheDir: join(absolute(env.XDG_CACHE_HOME, join(home, '.cache')), 'hraness-companion'),
  };
  throw new CompanionError('unsupported_target', `Unsupported companion platform: ${platform}.`);
}

export interface PlatformDiagnostic { code: string; severity: 'info' | 'warning' | 'error'; message: string; guidance?: string }
/** Diagnostics do not change OS policy, clear quarantine, or attempt to approve software. */
export function diagnosePlatform(options: PlatformOptions = {}): PlatformDiagnostic[] {
  const platform = options.platform ?? process.platform;
  const env = options.env ?? process.env;
  try { resolveTarget(options); } catch (error) {
    return [{ code: 'unsupported_target', severity: 'error', message: (error as Error).message }];
  }
  if (platform === 'darwin') return [{
    code: 'macos_approval', severity: 'info', message: 'This is an unbundled executable. macOS may request approval for downloaded software.',
    guidance: 'Verify the release and SHA-256 first. If macOS blocks it, ask the human to use Privacy & Security → Open Anyway when available. Do not disable Gatekeeper or remove quarantine automatically. https://support.apple.com/102445',
  }];
  if (platform === 'win32') return [{
    code: 'windows_approval', severity: 'info', message: 'Windows may show SmartScreen or application-control policy prompts for unsigned executables.',
    guidance: 'Verify the release and SHA-256. Ask the human to review any warning; organization policy can require an administrator. Smart App Control has no per-app exception; use the CLI if policy blocks the companion. Never disable Defender or application-control policy.',
  }];
  const diagnostics: PlatformDiagnostic[] = [];
  if (!env.DISPLAY && !env.WAYLAND_DISPLAY) diagnostics.push({ code: 'graphical_session_missing', severity: 'error', message: 'No X11 or Wayland display was found.', guidance: 'Run the CLI inside the signed-in graphical desktop session; the CLI itself remains usable without a tray.' });
  if (!env.DBUS_SESSION_BUS_ADDRESS) diagnostics.push({ code: 'session_bus_missing', severity: 'warning', message: 'No desktop D-Bus session address was found.', guidance: 'Start from the user graphical session. Do not start the tray through sudo or a system service.' });
  diagnostics.push({ code: 'linux_native_dependencies', severity: 'info', message: 'The GNU Linux binary needs GTK 3, WebKitGTK 4.1, and an Ayatana AppIndicator or compatible AppIndicator library.', guidance: 'Install the distribution equivalents of libgtk-3-0, libwebkit2gtk-4.1-0, and libayatana-appindicator3-1. musl-only distributions need a supported GNU environment or a source build.' });
  diagnostics.push({ code: 'tray_host_unverified', severity: 'warning', message: 'A graphical session does not guarantee that a tray host is available.', guidance: 'KDE and similar desktops usually expose a system tray. GNOME may require an AppIndicator extension enabled by the user. Use the CLI if the desktop does not provide a tray.' });
  return diagnostics;
}
