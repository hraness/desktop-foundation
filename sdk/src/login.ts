// Opt-in login item for a product's control owner: a LaunchAgent on macOS
// and an XDG autostart entry on Linux. Built on `autostart.ts`, so the file
// carries the same owned-file header and is only ever replaced or removed
// when it is ours and unedited. Installing one is persistent configuration,
// so products expose it as a `decide` verb (`<product> control install`).

import { planAutostart, removeAutostart, setAutostart, type AutostartPlan } from './autostart.js';
import { CompanionError } from './errors.js';
import type { PlatformOptions } from './platform.js';

export interface LoginItemOptions extends PlatformOptions {
  /** The product, such as `ghostget`. The item's id is `<product>-owner`. */
  product: string;
  /** Absolute path of the program that serves the owner. */
  program: string;
  args?: string[];
  /** A display name for the Linux entry. */
  label?: string;
}
export interface LoginItemResult { path: string; changed: boolean; activation?: 'next-login' }

const PRODUCT = /^[a-z][a-z0-9-]{0,57}$/;
/** The file an owner login item would use. No side effects. */
export function planLoginItem(opts: LoginItemOptions): AutostartPlan {
  if (!PRODUCT.test(opts.product)) throw new CompanionError('unsafe_path', 'The product must be a short lowercase name.');
  if ((opts.platform ?? process.platform) === 'win32') throw new CompanionError('unsupported_target', 'Owner login items are for macOS and Linux.');
  return planAutostart({ ...opts, id: `${opts.product}-owner`, label: opts.label ?? `${opts.product} owner`, executable: opts.program, args: opts.args ?? [] });
}
/** Writes the login item. It takes effect at the next login; nothing starts now. */
export async function installLoginItem(opts: LoginItemOptions): Promise<LoginItemResult> {
  return setAutostart(planLoginItem(opts));
}
/** Removes the login item when it is ours and unedited. Does not stop a running owner. */
export async function uninstallLoginItem(opts: LoginItemOptions): Promise<LoginItemResult> {
  const { path, removed } = await removeAutostart(planLoginItem(opts));
  return { path, changed: removed };
}
