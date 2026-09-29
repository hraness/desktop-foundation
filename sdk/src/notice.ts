// Wire types for the helper's one-shot `--notice` and `--prompt` modes. The
// menu bar protocol (snapshots, menu items, symbols) was removed in 1.0.

/** The largest JSON line the helper reads or writes, newline included. */
export const MAX_FRAME_BYTES = 256 * 1024;

/** Permission kinds, as used in notice requests and permission errors. */
export type PermissionKind =
  | 'full-disk-access' | 'automation' | 'contacts' | 'accessibility'
  | 'screen-recording' | 'camera' | 'microphone' | 'local-network'
  | 'incoming-connections' | 'notifications' | 'login-item' | 'keychain'
  | 'developer-tools' | 'gatekeeper';

/** Kinds with a System Settings pane (keychain and developer-tools have none). */
export type SettingsPermissionKind = Exclude<PermissionKind, 'keychain' | 'developer-tools'>;

/** One-shot `hraness-helper --notice` request. */
export interface NoticeRequest {
  type: 'notice-request';
  version: 1;
  title: string;
  message: string;
  primary: string;
  secondary?: string;
  settings?: SettingsPermissionKind;
  timeoutSeconds?: number;
}
export type NoticeStatus = 'primary' | 'secondary' | 'settings' | 'timeout' | 'unavailable';
export interface NoticeResult { type: 'notice-result'; version: 1; status: NoticeStatus }
