// Who is reading a CLI's output. One rule for every Hraness CLI; the Rust twin
// is `hraness_cli_kit::audience`. See docs/permissions.md § Audience.

export type Audience = 'human' | 'agent' | 'quiet';

/**
 * Exact environment names that mark an agent session. Only exact names count:
 * a prefix such as `CODEX_` also matches human configuration like `CODEX_HOME`.
 */
export const AGENT_MARKERS = [
  'AI_AGENT', 'CLAUDECODE', 'CODEX_SANDBOX', 'CODEX_SANDBOX_NETWORK_DISABLED', 'CURSOR_AGENT', 'GEMINI_CLI',
] as const;

export interface AudienceInput {
  env?: NodeJS.ProcessEnv;
  /** Defaults to `process.stderr.isTTY`. */
  stderrIsTTY?: boolean;
}

/**
 * 1. `HRANESS_AUDIENCE` = `human` | `agent` | `quiet` (`off` means `quiet`) wins.
 * 2. Any agent marker with a nonempty value → `agent`.
 * 3. stderr is a terminal → `human`.
 * 4. Otherwise → `quiet`.
 */
export function detectAudience(input: AudienceInput = {}): Audience {
  const env = input.env ?? process.env;
  const override = env.HRANESS_AUDIENCE?.trim().toLowerCase();
  if (override === 'human' || override === 'agent' || override === 'quiet') return override;
  if (override === 'off') return 'quiet';
  if (AGENT_MARKERS.some(name => (env[name] ?? '') !== '')) return 'agent';
  return (input.stderrIsTTY ?? process.stderr.isTTY === true) ? 'human' : 'quiet';
}
