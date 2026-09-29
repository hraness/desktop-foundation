import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Tests run from dist/test; the contract lives at the repository root.
export const contractPath = (name: string) => fileURLToPath(new URL(`../../contract/${name}`, import.meta.url));
export const readContract = (name: string) => JSON.parse(readFileSync(contractPath(name), 'utf8'));
export const goldenPath = (name: string) => fileURLToPath(new URL(`../../sdk/test/golden/${name}`, import.meta.url));

type Schema = Record<string, any>;
/** The subset of JSON Schema that contract/envelope.schema.json uses. Returns problems, empty when valid. */
export function validate(root: Schema, value: unknown, schema: Schema = root, at = '$'): string[] {
  if (schema.$ref) return validate(root, value, schema.$ref.replace(/^#\//, '').split('/').reduce((s: Schema, k: string) => s[k], root), at);
  if (schema.oneOf) {
    const passing = schema.oneOf.filter((s: Schema) => validate(root, value, s, at).length === 0).length;
    return passing === 1 ? [] : [`${at}: matched ${passing} of oneOf`];
  }
  const problems: string[] = [];
  if ('const' in schema && value !== schema.const) problems.push(`${at}: not ${JSON.stringify(schema.const)}`);
  if (schema.enum && !schema.enum.includes(value)) problems.push(`${at}: not in enum`);
  if (schema.type === 'string') {
    if (typeof value !== 'string') return [`${at}: not a string`];
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) problems.push(`${at}: does not match ${schema.pattern}`);
    if (schema.minLength && value.length < schema.minLength) problems.push(`${at}: too short`);
  }
  if (schema.type === 'array') {
    if (!Array.isArray(value)) return [`${at}: not an array`];
    value.forEach((item, i) => problems.push(...validate(root, item, schema.items ?? {}, `${at}[${i}]`)));
  }
  if (schema.type === 'object') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return [`${at}: not an object`];
    const record = value as Record<string, unknown>;
    for (const key of schema.required ?? []) if (!(key in record)) problems.push(`${at}.${key}: missing`);
    for (const [key, item] of Object.entries(record)) {
      const sub = schema.properties?.[key];
      if (sub) problems.push(...validate(root, item, sub, `${at}.${key}`));
      else if (schema.additionalProperties === false) problems.push(`${at}.${key}: not allowed`);
    }
  }
  return problems;
}
