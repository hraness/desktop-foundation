export type CompanionErrorCode =
  | 'unsupported_target' | 'invalid_manifest' | 'download_failed' | 'integrity_failed'
  | 'unsafe_path' | 'cache_conflict' | 'autostart_conflict' | 'os_approval_required';

export class CompanionError extends Error {
  constructor(readonly code: CompanionErrorCode, message: string, readonly guidance?: string) {
    super(message);
    this.name = 'CompanionError';
  }
  toJSON() { return { code: this.code, message: this.message, guidance: this.guidance }; }
}
