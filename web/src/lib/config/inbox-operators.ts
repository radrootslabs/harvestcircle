import { exactLocalFields } from '../contracts/local-records.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  canonicalRelayOrigin,
  readRelayPolicy,
  validateRelayPolicy,
  type RelayPolicy
} from './relays.ts';
import { RELAY_BUDGETS } from './budgets.ts';

export type InboxOperatorDeclaration = Readonly<{
  origin: string;
  auth: 'not_required' | 'explicit_required' | 'unsupported';
  recipientRead: 'supported' | 'unsupported';
  namedArchiveAck: 'supported' | 'unsupported';
  exactReadback: 'supported' | 'unsupported';
  retention: 'operator_defined' | 'unsupported';
}>;
export type InboxOperatorManifest = Readonly<{
  schemaVersion: 1;
  revision: number;
  authority: 'unconfigured' | 'source_fixture' | 'operator';
  policyWire: string;
  evidenceSha256: string | null;
  relays: readonly InboxOperatorDeclaration[];
}>;
declare const operatorBrand: unique symbol;
export type InboxOperatorQualification = Readonly<{
  [operatorBrand]: true;
}>;
const declarations = new WeakMap<InboxOperatorQualification, string>();
const supported = (value: unknown) =>
  value === 'supported' || value === 'unsupported';

// Source-owned declarations only. Admission authenticates no operator action,
// grants no socket/signer/publication permission and never supplies readiness.
// Production consumes the checked-in deployment owner; fixtures retain their
// separate authority label. Actual access facts and Q evidence remain required.
export function validateInboxOperatorQualification(
  raw: unknown
): InboxOperatorQualification | undefined {
  if (
    typeof raw !== 'string' ||
    raw.length > 65536 ||
    !raw.isWellFormed() ||
    new TextEncoder().encode(raw).length > 65536
  )
    return undefined;
  try {
    const row: unknown = JSON.parse(raw);
    // Config has one exact canonical representation. Unlike stored protocol
    // wires, it needs no whitespace compatibility. Duplicate fields, escaped
    // aliases and ambiguous number spellings cannot survive this comparison.
    if (
      JSON.stringify(row) !== raw ||
      !exactLocalFields(row, [
        'schemaVersion',
        'revision',
        'authority',
        'policyWire',
        'evidenceSha256',
        'relays'
      ]) ||
      row.schemaVersion !== 1 ||
      safeUnsignedInteger(row.revision) === undefined ||
      (row.revision as number) < 1 ||
      !['unconfigured', 'source_fixture', 'operator'].includes(
        row.authority as string
      ) ||
      typeof row.policyWire !== 'string' ||
      !Array.isArray(row.relays) ||
      row.relays.length > RELAY_BUDGETS.inbox
    )
      return undefined;
    const policy = validateRelayPolicy(row.policyWire);
    if (!policy) return undefined;
    const manifest = readRelayPolicy(policy);
    if (JSON.stringify(manifest) !== row.policyWire) return undefined;
    if (
      row.authority === 'unconfigured'
        ? row.relays.length !== 0 || row.evidenceSha256 !== null
        : row.relays.length === 0 ||
          typeof row.evidenceSha256 !== 'string' ||
          !/^[0-9a-f]{64}$/.test(row.evidenceSha256)
    )
      return undefined;
    for (const entry of row.relays as unknown[]) {
      if (
        !exactLocalFields(entry, [
          'origin',
          'auth',
          'recipientRead',
          'namedArchiveAck',
          'exactReadback',
          'retention'
        ]) ||
        typeof entry.origin !== 'string' ||
        canonicalRelayOrigin(entry.origin) !== entry.origin ||
        !manifest.inbox.some(({ origin }) => origin === entry.origin) ||
        !['not_required', 'explicit_required', 'unsupported'].includes(
          entry.auth as string
        ) ||
        !supported(entry.recipientRead) ||
        !supported(entry.namedArchiveAck) ||
        !supported(entry.exactReadback) ||
        (entry.retention !== 'operator_defined' &&
          entry.retention !== 'unsupported')
      )
        return undefined;
    }
    if (
      new Set(
        (row.relays as InboxOperatorDeclaration[]).map(({ origin }) => origin)
      ).size !== row.relays.length
    )
      return undefined;
    const token = Object.freeze({}) as InboxOperatorQualification;
    declarations.set(token, raw);
    return token;
  } catch {
    return undefined;
  }
}

// A detached view is public routing/qualification metadata, never a capability.
export function inboxOperatorQualificationSnapshot(
  token: InboxOperatorQualification
): InboxOperatorManifest | undefined {
  const raw = declarations.get(token);
  return raw === undefined
    ? undefined
    : (JSON.parse(raw) as InboxOperatorManifest);
}

// The complete original config, including evidence, version and every origin,
// is the frozen binding. A policy's detached view cannot change that binding.
export function inboxOperatorQualificationWire(
  token: InboxOperatorQualification,
  policy: RelayPolicy
): string | undefined {
  try {
    const raw = declarations.get(token);
    return raw !== undefined &&
      (JSON.parse(raw) as InboxOperatorManifest).policyWire ===
        JSON.stringify(readRelayPolicy(policy))
      ? raw
      : undefined;
  } catch {
    return undefined;
  }
}
