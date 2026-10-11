import { navigationHref } from '../navigation-url.ts';
import { canonicalRelayOrigin } from './relays.ts';

export interface SupportManifest {
  readonly schemaVersion: 1;
  readonly owner: string;
  readonly destination: string;
}
declare const supportBrand: unique symbol;
export type SupportPolicy = Readonly<{ [supportBrand]: true }>;
const policies = new WeakMap<SupportPolicy, string>();

function destination(value: string): string | undefined {
  if (!value) return '';
  if (!navigationHref(value)) return undefined;
  try {
    const url = new URL(value);
    if (url.search || url.hash || url.username || url.password)
      return undefined;
    if (url.protocol === 'https:') {
      if (url.href !== value || !canonicalRelayOrigin('wss://' + url.host))
        return undefined;
      return value;
    }
    if (url.protocol !== 'mailto:' || !value.startsWith('mailto:'))
      return undefined;
    // One conservative, literal address. No recipient list, encoded header,
    // subject/body carrier or automatically attached report content.
    const address = value.slice(7);
    const parts = address.split('@');
    if (parts.length !== 2) return undefined;
    const [local, host] = parts;
    if (
      !local ||
      local.length > 64 ||
      !/^[A-Za-z0-9._+-]+$/.test(local) ||
      local.startsWith('.') ||
      local.endsWith('.') ||
      local.includes('..') ||
      !canonicalRelayOrigin('wss://' + host) ||
      new URL('https://' + host).hostname !== host
    )
      return undefined;
    return value;
  } catch {
    return undefined;
  }
}

// Public configuration only, bounded before JSON parsing. Caller objects and
// getters never enter this owner. Admission is not operator qualification.
export function validateSupportPolicy(raw: unknown): SupportPolicy | undefined {
  if (
    typeof raw !== 'string' ||
    raw.length > 16384 ||
    !raw.isWellFormed() ||
    new TextEncoder().encode(raw).length > 16384
  )
    return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value))
      return undefined;
    const row = value as Record<string, unknown>;
    let count = 0;
    for (const key in row) {
      if (!['schemaVersion', 'owner', 'destination'].includes(key))
        return undefined;
      count++;
    }
    if (
      count !== 3 ||
      row.schemaVersion !== 1 ||
      typeof row.owner !== 'string' ||
      typeof row.destination !== 'string' ||
      !row.owner.isWellFormed() ||
      row.owner !== row.owner.trim() ||
      new TextEncoder().encode(row.owner).length > 256 ||
      /[\p{Cc}\p{Cf}]/u.test(row.owner)
    )
      return undefined;
    const href = destination(row.destination);
    if (href === undefined) return undefined;
    const token = Object.freeze({}) as SupportPolicy;
    policies.set(
      token,
      JSON.stringify({ schemaVersion: 1, owner: row.owner, destination: href })
    );
    return token;
  } catch {
    return undefined;
  }
}
export function readSupportPolicy(policy: SupportPolicy): SupportManifest {
  const raw = policies.get(policy);
  if (!raw) throw Error('support_policy_invalid');
  return JSON.parse(raw) as SupportManifest;
}
export function requireLaunchSupportPolicy(
  policy: SupportPolicy
): SupportPolicy {
  const actual = readSupportPolicy(policy);
  if (!actual.owner || !actual.destination)
    throw Error('support_launch_inputs_missing');
  return policy;
}
