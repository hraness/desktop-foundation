#!/usr/bin/env node
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { handleCompanionCommand, openBrowser } from './commands.js';
import { userPaths } from './platform.js';

const args = process.argv.slice(2);
if (args.includes('--help')) {
  console.log('companion [start|stop|status|doctor|install|uninstall] [--json]\nStarts a small demonstration tray. Products embed the SDK in their own menubar command.');
} else {
  try {
    process.exitCode = await handleCompanionCommand({
      appId: 'org.hraness.companion.demo', name: 'Hraness Companion', title: 'Hc',
      stateDir: join(userPaths().dataDir, 'org.hraness.companion.demo'),
      ...(process.env.HRANESS_COMPANION_BINARY ? { binary: process.env.HRANESS_COMPANION_BINARY } : {}),
      snapshot: () => [{ kind: 'label', label: 'CLI companion is running' }, { kind: 'action', id: 'docs', label: 'Open documentation' }, { kind: 'separator' }, { kind: 'quit', label: 'Quit' }],
      onAction: async id => { if (id === 'docs') await openBrowser('https://github.com/hraness/desktop-foundation'); },
      onDiagnostic: code => process.stderr.write(`companion: ${code}\n`),
    }, { args, foreground: { executable: process.execPath, args: [fileURLToPath(import.meta.url), '--foreground'] } });
  } catch (error) { console.error(error instanceof Error ? error.message : 'companion-failed'); process.exitCode = 1; }
}
