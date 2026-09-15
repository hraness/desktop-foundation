import { readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
const files = readdirSync('dist/test', { recursive: true }).filter(name => name.endsWith('.test.js')).map(name => join('dist/test', name));
if (!files.length) throw new Error('No SDK tests built');
const result = spawnSync(process.execPath, ['--test', ...files], { stdio: 'inherit' });
process.exitCode = result.status ?? 1;
