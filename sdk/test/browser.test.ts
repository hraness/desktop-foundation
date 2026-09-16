import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { spawn, type ChildProcess } from 'node:child_process';
import { createBrowserOpener } from '../src/browser.js';

test('invalid addresses never spawn and diagnostics contain no input', async () => {
  const open = createBrowserOpener((() => { throw new Error('must not launch'); }) as typeof spawn);
  for (const address of ['', 'file:///tmp/a', 'https://', 'https:///example.com', 'https://@example.com', 'https://user:secret@example.com', 'https://example.com/\n', 'https://example.com/\\other', 'https://example.com/\u202etext', 'https://example.com/a b', 'http://example.com', `https://example.com/${'a'.repeat(4096)}`]) {
    await assert.rejects(open(address), { message: 'unsupported-browser-url' });
  }
});

test('explicit handoff has one URL argument, no shell, shared admission, and closes before reuse', async () => {
  const children: EventEmitter[] = [];
  const open = createBrowserOpener(((program: string, args: string[], options: unknown) => {
    assert.ok(['/usr/bin/open', 'rundll32.exe', 'xdg-open'].includes(program));
    assert.deepEqual(options, { stdio: 'ignore', windowsHide: true, shell: false });
    assert.equal(args.at(-1), children.length === 0 ? 'https://example.com/?product=tool&source=desktop#support' : 'http://localhost:3000/');
    const child = new EventEmitter(); children.push(child); return child as ChildProcess;
  }) as typeof spawn);
  const pending = open('https://example.com/?product=tool&source=desktop#support');
  await assert.rejects(open('https://example.com/'), { message: 'browser-open-busy' });
  children[0]!.emit('exit', 0);
  await assert.rejects(open('https://example.com/'), { message: 'browser-open-busy' });
  children[0]!.emit('close', 0); await pending;
  const again = open('http://localhost:3000/'); children[1]!.emit('close', 0); await again;
});

test('spawn failure is generic and leaves opener reusable', async () => {
  const open = createBrowserOpener((() => { throw new Error('private raw argv'); }) as typeof spawn);
  await assert.rejects(open('https://example.com'), { message: 'browser-open-failed' });
  await assert.rejects(open('https://example.com'), { message: 'browser-open-failed' });
});

test('timeout kills and awaits a real child without opening a browser', async () => {
  let child: ChildProcess | undefined;
  const open = createBrowserOpener((() => {
    child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
    return child;
  }) as typeof spawn, 100);
  await assert.rejects(open('https://example.com/'), { message: 'browser-open-timeout' });
  assert.notEqual(child?.signalCode, null);
});
