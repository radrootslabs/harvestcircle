import { canonicalPublicKey } from '../contracts/public-key.ts';
import { RELAY_BUDGETS } from './budgets.ts';

export interface PublicRelay {
  readonly origin: string;
  readonly read: boolean;
  readonly write: boolean;
  readonly nip50: boolean;
}
export interface InboxRelay {
  readonly origin: string;
  readonly read: boolean;
  readonly write: boolean;
}
export interface RelayManifest {
  readonly schemaVersion: 1;
  readonly public: readonly PublicRelay[];
  readonly inbox: readonly InboxRelay[];
  readonly postingEnabled: boolean;
  readonly messagingEnabled: boolean;
  readonly operatorDenylist: readonly string[];
}

declare const admittedPolicy: unique symbol;
export type RelayPolicy = Readonly<{ readonly [admittedPolicy]: true }>;
// Only this owner retains the admitted JSON. Detached views cannot change egress.
const snapshots = new WeakMap<RelayPolicy, string>();
function fields(
  value: unknown,
  names: readonly string[]
): value is Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  let count = 0;
  for (const key in value) {
    if (!names.includes(key)) return false;
    count++;
  }
  return count === names.length;
}

function publicAddress(host: string): boolean {
  if (host.startsWith('[')) {
    // Canonical global-unicast IPv6 only; mapped IPv4, local, multicast and
    // unspecified forms fail closed. This is literal admission, never DNS or
    // IP-at-connect enforcement (the browser owns connection resolution).
    const head = host.slice(1, -1).split(':');
    const first = Number.parseInt(head[0], 16);
    return (
      first >= 0x2000 &&
      first <= 0x3fff &&
      !(first === 0x2001 && ['db8', '0', '2', '10', '20'].includes(head[1]))
    );
  }
  if (/^[0-9.]+$/.test(host)) {
    const [a, b, c] = host.split('.').map(Number);
    return (
      a !== 0 &&
      a !== 10 &&
      a !== 127 &&
      a < 224 &&
      !(a === 100 && b >= 64 && b <= 127) &&
      !(a === 169 && b === 254) &&
      !(a === 172 && b >= 16 && b <= 31) &&
      !(a === 192 && [0, 168].includes(b)) &&
      !(a === 198 && ([18, 19].includes(b) || (b === 51 && c === 100))) &&
      !(a === 203 && b === 0 && c === 113)
    );
  }
  const labels = host.split('.');
  return (
    labels.length >= 2 &&
    host.length <= 253 &&
    labels.every((label) =>
      /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label)
    ) &&
    !/(?:^|\.)(?:localhost|local|internal|invalid|test|example|onion)$/.test(
      host
    ) &&
    !host.endsWith('.home.arpa')
  );
}

export function canonicalRelayOrigin(value: unknown): string | undefined {
  if (
    typeof value !== 'string' ||
    value.length > 8192 ||
    !value.isWellFormed() ||
    !/^wss:\/\//.test(value) ||
    /[\s\\%\p{Cc}\p{Cf}]/u.test(value)
  )
    return undefined;
  try {
    const url = new URL(value);
    // A canonical origin has no userinfo, resource path, query or fragment,
    // case/port normalization, percent encoding or trailing slash aliases.
    if (
      url.origin !== value ||
      url.username ||
      url.password ||
      url.port === '0' ||
      !publicAddress(url.hostname)
    )
      return undefined;
    return url.origin;
  } catch {
    return undefined;
  }
}

function publicRelays(value: unknown): readonly PublicRelay[] | undefined {
  if (!Array.isArray(value) || value.length > RELAY_BUDGETS.public)
    return undefined;
  const rows = (value as unknown[]).map((entry) => {
    if (!fields(entry, ['origin', 'read', 'write', 'nip50'])) return undefined;
    const { origin: rawOrigin, read, write, nip50 } = entry;
    const origin = canonicalRelayOrigin(rawOrigin);
    if (
      !origin ||
      typeof read !== 'boolean' ||
      typeof write !== 'boolean' ||
      typeof nip50 !== 'boolean' ||
      (!read && !write) ||
      (nip50 && !read)
    )
      return undefined;
    return { origin, read, write, nip50 };
  });
  if (
    rows.some((row) => !row) ||
    rows.some((row, index) =>
      rows.some((other, j) => index !== j && row?.origin === other?.origin)
    )
  )
    return undefined;
  return rows as readonly PublicRelay[];
}
function inboxRelays(value: unknown): readonly InboxRelay[] | undefined {
  if (!Array.isArray(value) || value.length > RELAY_BUDGETS.inbox)
    return undefined;
  const rows = (value as unknown[]).map((entry) => {
    if (!fields(entry, ['origin', 'read', 'write'])) return undefined;
    const { origin: rawOrigin, read, write } = entry;
    const origin = canonicalRelayOrigin(rawOrigin);
    if (
      !origin ||
      typeof read !== 'boolean' ||
      typeof write !== 'boolean' ||
      (!read && !write)
    )
      return undefined;
    return { origin, read, write };
  });
  if (
    rows.some((row) => !row) ||
    rows.some((row, index) =>
      rows.some((other, j) => index !== j && row?.origin === other?.origin)
    )
  )
    return undefined;
  return rows as readonly InboxRelay[];
}

// Accept bounded JSON, as with verified event ingress. Executable objects,
// getters, symbols, caller prototypes and post-admission mutations stay outside.
// 64 KiB bounds this public configuration parser; it is not a transport budget.
export function validateRelayPolicy(raw: unknown): RelayPolicy | undefined {
  if (
    typeof raw !== 'string' ||
    raw.length > 65536 ||
    !raw.isWellFormed() ||
    new TextEncoder().encode(raw).length > 65536
  )
    return undefined;
  try {
    const row: unknown = JSON.parse(raw);
    if (
      !fields(row, [
        'schemaVersion',
        'public',
        'inbox',
        'postingEnabled',
        'messagingEnabled',
        'operatorDenylist'
      ]) ||
      row.schemaVersion !== 1 ||
      typeof row.postingEnabled !== 'boolean' ||
      typeof row.messagingEnabled !== 'boolean' ||
      !Array.isArray(row.operatorDenylist)
    )
      return undefined;
    const publicEntries = publicRelays(row.public);
    const inboxEntries = inboxRelays(row.inbox);
    if (!publicEntries || !inboxEntries) return undefined;
    const denylist = (row.operatorDenylist as unknown[]).map(
      canonicalPublicKey
    );
    if (
      denylist.some((key) => !key) ||
      denylist.some((key, index) => denylist.indexOf(key) !== index)
    )
      return undefined;
    const token = Object.freeze({}) as RelayPolicy;
    snapshots.set(
      token,
      JSON.stringify({
        schemaVersion: 1,
        public: publicEntries,
        inbox: inboxEntries,
        postingEnabled: row.postingEnabled,
        messagingEnabled: row.messagingEnabled,
        operatorDenylist: denylist
      })
    );
    return token;
  } catch {
    return undefined;
  }
}

export function readRelayPolicy(policy: RelayPolicy): RelayManifest {
  const raw = snapshots.get(policy);
  if (!raw) throw new Error('relay_policy_invalid');
  return JSON.parse(raw) as RelayManifest;
}
// Fixed public sources never accept hints. Private targets additionally need
// verified recipient preferences; intersection does not authenticate kind10050.
// Callers get detached target arrays and cannot mutate the retained policy.
export function publicRelayTargets(
  policy: RelayPolicy,
  operation: 'read' | 'write'
): readonly string[] {
  return readRelayPolicy(policy)
    .public.filter((entry) => entry[operation])
    .map((entry) => entry.origin);
}
export function inboxRelayTargets(
  policy: RelayPolicy,
  recipientPreferences: readonly string[],
  operation: 'read' | 'write'
): readonly string[] {
  return readRelayPolicy(policy)
    .inbox.filter(
      (entry) => entry[operation] && recipientPreferences.includes(entry.origin)
    )
    .map((entry) => entry.origin);
}
// Source launch-input preparation only. Operator/retention/independent-client
// qualification and real permissions remain separate mandatory gates.
export function requireLaunchRelayPolicy(policy: RelayPolicy): RelayPolicy {
  const manifest = readRelayPolicy(policy);
  if (
    !manifest.public.some((relay) => relay.read) ||
    !manifest.public.some(({ write }) => write) ||
    !manifest.inbox.some((relay) => relay.read) ||
    !manifest.inbox.some(({ write }) => write)
  )
    throw new Error('relay_launch_inputs_missing');
  return policy;
}
