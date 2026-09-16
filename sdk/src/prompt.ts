import { spawn } from 'node:child_process';
import type { Readable, Writable } from 'node:stream';
import { ensureBinary, type ReleaseManifest } from './install.js';
import { packagedManifest } from './client.js';
import { MAX_FRAME_BYTES } from './protocol.js';

export const PROMPT_LIMITS = {
  title: 128,
  message: 512,
  valueChars: 4096,
  timeoutSeconds: 600,
  defaultTimeoutSeconds: 120,
} as const;

export interface PromptRequest {
  title: string;
  message: string;
  /** Hide the entered text (default true). */
  secret?: boolean;
  /** Existing secret to edit; travels on the private stdin channel only. */
  prefill?: string;
  /** Auto-dismiss deadline, 1-600 (default 120). */
  timeoutSeconds?: number;
}
export type PromptResult =
  | { status: 'submitted'; value: string }
  | { status: 'cancelled' | 'timeout' | 'unavailable' };
export interface PromptBinaryOptions {
  binary?: string;
  binaryArgs?: readonly string[];
  manifest?: ReleaseManifest;
  cacheDir?: string;
}
export interface PromptIo {
  input: Readable & { isTTY?: boolean; setRawMode?: (mode: boolean) => void };
  output: Writable;
}

const unsafe = /[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u;
function display(value: unknown, max: number, field: string): asserts value is string {
  if (typeof value !== 'string' || !value || [...value].length > max || unsafe.test(value)) {
    throw new Error(`invalid-prompt-${field}`);
  }
}
export interface ValidatedPromptRequest {
  title: string;
  message: string;
  secret: boolean;
  prefill?: string;
  timeoutSeconds: number;
}
export function validatePromptRequest(request: PromptRequest): ValidatedPromptRequest {
  if (!request || typeof request !== 'object' || Array.isArray(request)) throw new Error('invalid-prompt');
  const allowed = new Set(['title', 'message', 'secret', 'prefill', 'timeoutSeconds']);
  if (Object.keys(request).some(key => !allowed.has(key))) throw new Error('invalid-prompt');
  display(request.title, PROMPT_LIMITS.title, 'title');
  display(request.message, PROMPT_LIMITS.message, 'message');
  if (request.secret !== undefined && typeof request.secret !== 'boolean') throw new Error('invalid-prompt-secret');
  if (request.prefill !== undefined && (typeof request.prefill !== 'string' || [...request.prefill].length > PROMPT_LIMITS.valueChars)) throw new Error('invalid-prompt-prefill');
  const timeoutSeconds = request.timeoutSeconds ?? PROMPT_LIMITS.defaultTimeoutSeconds;
  if (!Number.isSafeInteger(timeoutSeconds) || timeoutSeconds < 1 || timeoutSeconds > PROMPT_LIMITS.timeoutSeconds) throw new Error('invalid-prompt-timeout');
  return { title: request.title, message: request.message, secret: request.secret ?? true, ...(request.prefill !== undefined ? { prefill: request.prefill } : {}), timeoutSeconds };
}

function parsePromptResult(line: string): PromptResult {
  if (Buffer.byteLength(line) > MAX_FRAME_BYTES) throw new Error('oversize-prompt-frame');
  const value: unknown = JSON.parse(line);
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid-prompt-frame');
  const v = value as Record<string, unknown>;
  if (v.type === 'error') {
    if (typeof v.code === 'string' && /^[a-z0-9-]{1,80}$/.test(v.code)) throw new Error(`prompt-runner-${v.code}`);
    throw new Error('invalid-prompt-frame');
  }
  if (v.version !== 1 || v.type !== 'prompt-result') throw new Error('invalid-prompt-frame');
  const keys = Object.keys(v);
  if (keys.some(key => !['type', 'version', 'status', 'value'].includes(key))) throw new Error('invalid-prompt-frame');
  if (v.status === 'submitted') {
    if (typeof v.value !== 'string' || [...v.value].length > PROMPT_LIMITS.valueChars) throw new Error('invalid-prompt-frame');
    return { status: 'submitted', value: v.value };
  }
  if (v.status === 'cancelled' || v.status === 'timeout' || v.status === 'unavailable') {
    if ('value' in v) throw new Error('invalid-prompt-frame');
    return { status: v.status };
  }
  throw new Error('invalid-prompt-frame');
}

async function resolveBinary(options: PromptBinaryOptions): Promise<{ program: string; args: string[] }> {
  if (options.binary) return { program: options.binary, args: [...(options.binaryArgs ?? [])] };
  const manifest = options.manifest ?? await packagedManifest();
  const binary = await ensureBinary({ manifest, cacheDir: options.cacheDir });
  return { program: binary.path, args: [...(options.binaryArgs ?? [])] };
}

/** Runs one bounded `--prompt-probe` without showing a dialog. */
export async function promptCapability(options: PromptBinaryOptions): Promise<{ capable: boolean; detail: string }> {
  const binary = await resolveBinary(options);
  const child = spawn(binary.program, [...binary.args, '--prompt-probe'], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
  const stdout = await new Promise<Buffer>((resolve, reject) => {
    const chunks: Buffer[] = [];
    child.stdout.on('data', (chunk: Buffer) => chunks.push(chunk));
    child.once('error', reject);
    child.once('close', () => resolve(Buffer.concat(chunks)));
    setTimeout(() => { child.kill(); reject(new Error('prompt-probe-timeout')); }, 10_000);
  });
  const value: unknown = JSON.parse(stdout.toString('utf8').trim().split('\n')[0] ?? '');
  const v = value as Record<string, unknown>;
  if (!v || v.type !== 'prompt-capability' || v.version !== 1 || typeof v.capable !== 'boolean' || typeof v.detail !== 'string' || Object.keys(v).length !== 4) throw new Error('invalid-prompt-frame');
  return { capable: v.capable, detail: v.detail };
}

/**
 * Renders one native dialog through the pinned companion binary. The spec and
 * result travel only over the private stdio channel; argv never carries them.
 */
export async function promptNative(request: PromptRequest, options: PromptBinaryOptions = {}): Promise<PromptResult> {
  const spec = validatePromptRequest(request);
  const binary = await resolveBinary(options);
  const child = spawn(binary.program, [...binary.args, '--prompt'], { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
  let pending: Buffer<ArrayBufferLike> = Buffer.alloc(0), result: PromptResult | undefined;
  const deadline = setTimeout(() => { child.kill(); }, spec.timeoutSeconds * 1000 + 10_000);
  try {
    return await new Promise<PromptResult>((resolve, reject) => {
      child.stdout.on('data', (chunk: Buffer) => {
        if (result) return;
        let offset = 0;
        while (offset < chunk.length) {
          const end = chunk.indexOf(10, offset);
          const part = chunk.subarray(offset, end < 0 ? chunk.length : end);
          if (pending.length + part.length > MAX_FRAME_BYTES) { reject(new Error('oversize-prompt-frame')); return; }
          pending = pending.length ? Buffer.concat([pending, part]) : part;
          if (end < 0) break;
          const done = pending; pending = Buffer.alloc(0); offset = end + 1;
          try { result = parsePromptResult(done.toString('utf8')); resolve(result); } catch (error) { reject(error); }
          return;
        }
      });
      child.once('error', () => reject(new Error('prompt-launch-failed')));
      child.once('close', code => { result ? resolve(result) : reject(new Error(`prompt-runner-exit-${code ?? 1}`)); });
      child.stdin.on('error', () => reject(new Error('prompt-pipe-closed')));
      const frame = JSON.stringify({ type: 'prompt-request', version: 1, ...spec }) + '\n';
      child.stdin.end(frame);
    });
  } finally {
    clearTimeout(deadline);
    if (child.exitCode === null) child.kill();
  }
}

/** Interactive TTY fallback: hidden entry on stderr/stdout with a masked echo. */
export async function promptTui(request: PromptRequest, io?: PromptIo): Promise<PromptResult> {
  const spec = validatePromptRequest(request);
  const input = io?.input ?? process.stdin;
  const output = io?.output ?? process.stderr;
  if (!input.isTTY) return { status: 'unavailable' };
  return await new Promise<PromptResult>(resolve => {
    const bytes: number[] = [];
    const timer = setTimeout(() => finish({ status: 'timeout' }), spec.timeoutSeconds * 1000);
    const finish = (result: PromptResult) => {
      clearTimeout(timer);
      input.removeListener('data', onData);
      input.setRawMode?.(false);
      input.pause();
      output.write('\n');
      bytes.length = 0;
      resolve(result);
    };
    const onData = (chunk: Buffer) => {
      for (const byte of chunk) {
        if (byte === 3 || byte === 4) return finish({ status: 'cancelled' });
        if (byte === 10 || byte === 13) return finish({ status: 'submitted', value: Buffer.from(bytes).toString('utf8') });
        if (byte === 8 || byte === 127) {
          // Drop the whole trailing codepoint: continuation bytes plus their lead.
          if (bytes.length) {
            while ((bytes.pop()! & 0xc0) === 0x80 && bytes.length) { /* rest of the sequence */ }
            output.write('\b \b');
          }
          continue;
        }
        if (bytes.length >= PROMPT_LIMITS.valueChars) continue;
        bytes.push(byte);
        output.write('*');
      }
    };
    output.write(`${spec.title}\n${spec.message}: `);
    input.setRawMode?.(true);
    input.on('data', onData);
    input.resume();
  });
}

/**
 * Ask for a secret with the friendliest surface available: the native dialog
 * first, then the TTY hidden prompt when the host cannot render one.
 */
export async function promptSecret(request: PromptRequest, options: PromptBinaryOptions & { io?: PromptIo } = {}): Promise<PromptResult> {
  const result = await promptNative(request, options);
  if (result.status !== 'unavailable') return result;
  return await promptTui(request, options.io);
}
