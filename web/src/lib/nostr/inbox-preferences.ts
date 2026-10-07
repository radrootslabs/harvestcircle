import { canonicalPublicKey } from '../contracts/public-key.ts';
import { canonicalRelayOrigin } from '../config/relays.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';

declare const preferenceBrand: unique symbol;
export type InboxPreference = Readonly<{ [preferenceBrand]: true }>;
type Status = 'supported' | 'unsupported';
type Saved = Readonly<{
  wire: string;
  author: string;
  id: string;
  createdAt: number;
  status: Status;
  relays: readonly string[];
}>;
export type InboxPreferenceResult =
  | Readonly<{
      status: 'rejected';
      reason: 'invalid_owner' | 'invalid_envelope' | 'wrong_author';
    }>
  | Readonly<{ status: 'unrelated' }>
  | Readonly<{ status: Status; value: InboxPreference }>;
const preferences = new WeakMap<InboxPreference, Saved>();

// Parse only. A compatible advertised list proves neither current discovery
// completeness nor allowlist intersection, access, readiness or consent to write.
// Generic ingress bounds and SDK hashing/signature checks remain authoritative.
export function readInboxPreference(
  raw: unknown,
  intendedAuthor: unknown
): InboxPreferenceResult {
  const author = canonicalPublicKey(intendedAuthor);
  if (!author) return { status: 'rejected', reason: 'invalid_owner' };
  if (typeof raw !== 'string')
    return { status: 'rejected', reason: 'invalid_envelope' };
  const proof = verifyEnvelope(raw);
  if (!proof.ok) return { status: 'rejected', reason: 'invalid_envelope' };
  const event = verifiedEnvelopeSnapshot(proof.value);
  if (!event) return { status: 'rejected', reason: 'invalid_envelope' };
  if (event.pubkey !== author)
    return { status: 'rejected', reason: 'wrong_author' };
  if (event.kind !== 10050) return { status: 'unrelated' };
  const relays: string[] = [];
  function appendRelay(origin: string) {
    relays.push(origin);
  }
  let compatible = true;
  for (const tag of event.tags) {
    if (tag[0] !== 'relay') continue;
    const origin = tag.length === 2 ? canonicalRelayOrigin(tag[1]) : undefined;
    if (!origin) compatible = false;
    else if (!relays.includes(origin)) appendRelay(origin);
  }
  const status =
    compatible && relays.length !== 0 ? 'supported' : 'unsupported';
  const token = Object.freeze({}) as InboxPreference;
  preferences.set(token, {
    wire: raw,
    author,
    id: event.id,
    createdAt: event.created_at,
    status,
    relays: status === 'supported' ? relays : []
  });
  // A verified unsupported head survives for the later latest-head resolver.
  // Never turn malformed/newer data into absence or silently use a partial list.
  return { status, value: token };
}
export function inboxPreferenceSnapshot(token: InboxPreference):
  | Readonly<{
      author: string;
      id: string;
      createdAt: number;
      status: Status;
      relays: string[];
    }>
  | undefined {
  const saved = preferences.get(token);
  return (
    saved && {
      author: saved.author,
      id: saved.id,
      createdAt: saved.createdAt,
      status: saved.status,
      relays: saved.relays.slice()
    }
  );
}
// Exact original, including whitespace, duplicate/unknown tags, content and
// bounded extra fields. Extra root fields are preserved data, not signed claims.
// Later explicit review owns any update/removal; this adapter never rewrites it.
export function inboxPreferenceWire(
  token: InboxPreference
): string | undefined {
  return preferences.get(token)?.wire;
}
