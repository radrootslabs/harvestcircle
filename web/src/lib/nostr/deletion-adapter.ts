import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';

declare const deletionBrand: unique symbol;
export type DeletionRequest = Readonly<{ readonly [deletionBrand]: true }>;
type Target = Readonly<{ tagIndex: number; rawTag: readonly string[] }>;
export type DeletionRequestSnapshot = Readonly<{
  id: string;
  pubkey: string;
  created_at: number;
  eventTargets: readonly (Target & { readonly eventId: string })[];
  addressTargets: readonly (Target & { readonly coordinate: string })[];
  kindAdvisories: readonly (Target & { readonly kind: number })[];
  diagnostics: readonly (Target & { readonly code: string })[];
  rawTags: readonly (readonly string[])[];
}>;
const owners = new WeakMap<
  DeletionRequest,
  Readonly<{
    proof: VerifiedEnvelope;
    projection: DeletionRequestSnapshot;
  }>
>();
const encoder = new TextEncoder();
function emptyRows<T>(): T[] {
  return [];
}
// Generic Lib189 Nip01Coordinate, deliberately separate from Food/naddr.
// Rust u32 text parsing accepts a plus and leading zeroes; identifiers stay raw.
export function canonicalDeletionCoordinate(value: string): string | undefined {
  if (!boundedUtf8(value, 4096)) return undefined;
  const first = value.indexOf(':'),
    second = value.indexOf(':', first + 1);
  if (first < 0 || second < 0) return undefined;
  const kindText = value.slice(0, first);
  if (!/^\+?[0-9]+$/u.test(kindText)) return undefined;
  const kind = Number(kindText),
    identifier = value.slice(second + 1);
  const replaceable =
    kind === 0 || kind === 3 || (kind >= 10000 && kind <= 19999);
  if (
    (!replaceable && !(kind >= 30000 && kind <= 39999)) ||
    (replaceable && identifier !== '')
  )
    return undefined;
  const pubkey = canonicalPublicKey(
    value.slice(first + 1, second).toLowerCase()
  );
  return pubkey === undefined ? undefined : `${kind}:${pubkey}:${identifier}`;
}
function compareUtf8(a: string, b: string): number {
  const x = encoder.encode(a),
    y = encoder.encode(b);
  for (let i = 0; i < Math.min(x.length, y.length); i++)
    if (x[i] !== y[i]) return x[i] - y[i];
  return x.length - y.length;
}
// Verification already enforces the selected content/tag/element/aggregate and
// 256KiB wire budgets. The minimal kind5 projection cannot exceed that raw wire.
// Admission establishes references only, never author authority or deletion.
export function admitDeletionRequest(
  proof: VerifiedEnvelope
):
  | Readonly<{ ok: true; value: DeletionRequest }>
  | Readonly<{ ok: false; code: string }> {
  const event = verifiedEnvelopeSnapshot(proof);
  if (!event) return { ok: false, code: 'deletion_proof_invalid' };
  if (event.kind !== 5) return { ok: false, code: 'unsupported_kind' };
  let eventTargets = emptyRows<Target & { eventId: string }>();
  let addressTargets = emptyRows<Target & { coordinate: string }>();
  for (let tagIndex = 0; tagIndex < event.tags.length; tagIndex++) {
    const rawTag = event.tags[tagIndex],
      name = rawTag[0],
      value = rawTag[1];
    if (name === 'e') {
      if (value === undefined)
        return { ok: false, code: 'deletion_event_target_shape' };
      if (!/^[0-9a-fA-F]{64}$/u.test(value))
        return { ok: false, code: 'deletion_event_target_invalid' };
      const eventId = value.toLowerCase();
      if (!eventTargets.some((v) => v.eventId === eventId))
        eventTargets = eventTargets.concat({ tagIndex, rawTag, eventId });
    } else if (name === 'a') {
      if (value === undefined)
        return { ok: false, code: 'deletion_address_target_shape' };
      const coordinate = canonicalDeletionCoordinate(value);
      if (coordinate === undefined)
        return { ok: false, code: 'deletion_address_target_invalid' };
      if (!addressTargets.some((v) => v.coordinate === coordinate))
        addressTargets = addressTargets.concat({
          tagIndex,
          rawTag,
          coordinate
        });
    }
  }
  if (eventTargets.length === 0 && addressTargets.length === 0)
    return { ok: false, code: 'deletion_target_missing' };
  let kindAdvisories = emptyRows<Target & { kind: number }>();
  let diagnostics = emptyRows<Target & { code: string }>();
  for (let tagIndex = 0; tagIndex < event.tags.length; tagIndex++) {
    const rawTag = event.tags[tagIndex];
    if (rawTag[0] !== 'k') continue;
    const value = rawTag[1],
      kind = Number(value);
    const suffix =
      value === undefined
        ? 'shape'
        : !/^(?:0|[1-9][0-9]*)$/u.test(value) || kind > 65535
          ? 'invalid'
          : kindAdvisories.some((v) => v.kind === kind)
            ? 'duplicate'
            : undefined;
    if (suffix)
      diagnostics = diagnostics.concat({
        tagIndex,
        rawTag,
        code: `deletion_kind_advisory_${suffix}_ignored`
      });
    else kindAdvisories = kindAdvisories.concat({ tagIndex, rawTag, kind });
  }
  if (eventTargets.length === 0)
    for (const advisory of kindAdvisories)
      if (
        !addressTargets.some(
          (v) =>
            Number(v.coordinate.slice(0, v.coordinate.indexOf(':'))) ===
            advisory.kind
        )
      )
        diagnostics = diagnostics.concat({
          tagIndex: advisory.tagIndex,
          rawTag: advisory.rawTag,
          code: 'deletion_kind_advisory_conflict_ignored'
        });
  eventTargets.sort((a, b) => compareUtf8(a.eventId, b.eventId));
  addressTargets.sort((a, b) => compareUtf8(a.coordinate, b.coordinate));
  kindAdvisories.sort((a, b) => a.kind - b.kind);
  diagnostics.sort((a, b) => a.tagIndex - b.tagIndex);
  const token = Object.freeze({}) as DeletionRequest;
  owners.set(token, {
    proof,
    projection: {
      id: event.id,
      pubkey: event.pubkey,
      created_at: event.created_at,
      eventTargets,
      addressTargets,
      kindAdvisories,
      diagnostics,
      rawTags: event.tags
    }
  });
  return { ok: true, value: token };
}
// Detached public snapshots cannot alter the retained tombstone or forge it.
export function deletionRequestSnapshot(
  request: DeletionRequest
): DeletionRequestSnapshot | undefined {
  const owner = owners.get(request);
  return owner === undefined
    ? undefined
    : (JSON.parse(JSON.stringify(owner.projection)) as DeletionRequestSnapshot);
}
// The original bounded signed public wire remains held by its genuine proof.
// This grants no signing capability, runtime store or remote-erasure authority.
export function deletionRequestEnvelope(
  request: DeletionRequest
): VerifiedEnvelope | undefined {
  return owners.get(request)?.proof;
}
