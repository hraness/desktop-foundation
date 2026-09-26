// `companion lint-menu [--strict] [--json] <fixture.json>…`: validates and lints
// menu snapshot fixtures so CI can run the menu rules over every product state.
import { readFile } from 'node:fs/promises';
import { createCliOutput, type CliOutput } from './cli-style.js';
import { lintMenu, renderMenuTree, type MenuLintFinding } from './menu-kit.js';
import { validateSnapshot, type Snapshot } from './protocol.js';
import { FOUNDATION_ACTION_IDS, validateSnapshotV2, type SnapshotV2 } from './protocol-v2.js';

export const LINT_MENU_USAGE = `Usage: companion lint-menu [--strict] [--json] <fixture.json>...

Check menu snapshot fixtures against the menu rules: at most 10 top-level
rows, one primary action, Quit last, sentence case, no paths, IDs or commands.

Options
  --strict                  Treat every finding as an error (exit 1)
  --proper-noun <name>      A name that may be capitalized; repeat as needed
  --json                    Print machine-readable output

Example
  companion lint-menu --strict test/fixtures/menu-*.json
`;

export interface LintMenuResult { file: string; valid: boolean; error?: string; findings: MenuLintFinding[]; tree?: string }

/** Runs the command; returns the exit code. Exit 2 for usage errors, 1 when any fixture is invalid or has errors. */
export async function lintMenuCommand(args: readonly string[], output: CliOutput = createCliOutput(), readText: (path: string) => Promise<string> = path => readFile(path, 'utf8')): Promise<number> {
  const json = args.includes('--json');
  const strict = args.includes('--strict');
  const files: string[] = [];
  const properNouns: string[] = [];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index]!;
    if (arg === '--json' || arg === '--strict') continue;
    if (arg === '--help' || arg === '-h') { output.result(LINT_MENU_USAGE.trimEnd()); return 0; }
    if (arg === '--proper-noun') {
      const value = args[++index];
      if (!value) { usageError(output, json, 'Missing a name after --proper-noun.'); return 2; }
      properNouns.push(value);
      continue;
    }
    if (arg.startsWith('-')) { usageError(output, json, `Unknown option "${arg}".`); return 2; }
    files.push(arg);
  }
  if (!files.length) { usageError(output, json, 'Name at least one fixture file.'); return 2; }

  const results: LintMenuResult[] = [];
  for (const file of files) {
    let value: SnapshotV2 | Snapshot;
    try {
      value = JSON.parse(await readText(file)) as SnapshotV2 | Snapshot;
      if (value?.version === 2) validateSnapshotV2(value);
      else validateSnapshot(value as Snapshot, FOUNDATION_ACTION_IDS);
    } catch (error) {
      const code = error instanceof SyntaxError ? 'invalid-json' : (error as NodeJS.ErrnoException).code === 'ENOENT' ? 'not-found' : (error as Error).message;
      results.push({ file, valid: false, error: code, findings: [] });
      continue;
    }
    results.push({ file, valid: true, findings: lintMenu(value, { strict, properNouns }), tree: renderMenuTree(value) });
  }

  const failed = results.some(result => !result.valid || result.findings.some(finding => finding.severity === 'error'));
  if (json) {
    output.result(JSON.stringify({ ok: !failed, results: results.map(({ tree: _tree, ...rest }) => rest) }));
    return failed ? 1 : 0;
  }
  let warnings = 0, errors = 0;
  for (const result of results) {
    if (!result.valid) {
      errors++;
      output.result(`${result.file}: not a valid menu snapshot (${result.error}).`, 'fail');
      continue;
    }
    if (!result.findings.length) { output.result(`${result.file}`, 'ok'); continue; }
    output.result(`${result.file}`, result.findings.some(f => f.severity === 'error') ? 'fail' : 'warn');
    for (const finding of result.findings) {
      if (finding.severity === 'error') errors++; else warnings++;
      output.detail(`${finding.rule} at ${finding.path}: ${finding.message}`);
    }
    output.result('');
    for (const line of result.tree!.trimEnd().split('\n')) output.detail(`  ${line}`);
  }
  const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
  if (!errors && !warnings) output.result(`No menu problems in ${count(results.length, 'fixture')}.`, 'ok');
  else output.result(`${[errors ? count(errors, 'error') : '', warnings ? count(warnings, 'warning') : ''].filter(Boolean).join(', ')}.`);
  return failed ? 1 : 0;
}

function usageError(output: CliOutput, json: boolean, message: string): void {
  if (json) output.result(JSON.stringify({ ok: false, error: { code: 'usage', message, next: 'companion lint-menu --help' } }));
  else output.error({ message, next: 'companion lint-menu --help' });
}
