#!/usr/bin/env node
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { exitQuietlyOnBrokenPipe } from './cli-style.js';
import { handleCompanionCommand, openBrowser } from './commands.js';
import { lintMenuCommand } from './lint-menu-command.js';
import { layout } from './menu-kit.js';
import { userPaths } from './platform.js';

const HELP = `Usage: companion [command] [--json]

A demo menu bar icon for desktop-foundation. Products embed the SDK in their
own menubar command.

Commands
  start       Show the demo menu bar icon (default)
  stop        Remove it
  status      Show whether it's running
  install     Open it at login
  uninstall   Stop opening it at login
  doctor      Check this computer can show it
  lint-menu   Check menu snapshot fixtures against the menu rules

Options
  -h, --help  Show this help
  --json      Print machine-readable output`;

exitQuietlyOnBrokenPipe();
const args = process.argv.slice(2);
if (args[0] === 'lint-menu') {
  process.exitCode = await lintMenuCommand(args.slice(1));
} else if (args.includes('--help') || args.includes('-h') || args[0] === 'help') {
  console.log(HELP);
} else {
  try {
    process.exitCode = await handleCompanionCommand({
      appId: 'org.hraness.companion.demo', name: 'Hraness Companion', mark: { symbol: 'mark.agent', letters: 'Hc' },
      stateDir: join(userPaths().dataDir, 'org.hraness.companion.demo'),
      ...(process.env.HRANESS_COMPANION_BINARY ? { binary: process.env.HRANESS_COMPANION_BINARY } : {}),
      snapshot: () => layout({
        name: 'Hraness Companion',
        status: { kind: 'status', symbol: 'status.running', label: 'Running' },
        primary: { kind: 'action', id: 'docs', label: 'Open documentation', symbol: 'action.help', opens: 'browser' },
        openAtLogin: true,
      }),
      onAction: async id => { if (id === 'docs') await openBrowser('https://github.com/hraness/desktop-foundation'); },
      onDiagnostic: code => process.stderr.write(`companion: ${code}\n`),
    }, { args, command: 'companion', foreground: { executable: process.execPath, args: [fileURLToPath(import.meta.url), '--foreground'] } });
  } catch (error) { console.error(error instanceof Error ? error.message : 'companion-failed'); process.exitCode = 1; }
}
