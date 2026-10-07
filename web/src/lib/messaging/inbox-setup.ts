import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import {
  inboxPreferenceSnapshot,
  inboxPreferenceWire
} from '../nostr/inbox-preferences.ts';
import {
  canonicalRelayOrigin,
  readRelayPolicy,
  publicRelayTargets,
  inboxRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  decodePublicRecord,
  type PublicRecordHandle
} from '../persistence/records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';

declare const reviewBrand: unique symbol;
export type InboxSetupReview = Readonly<{ [reviewBrand]: true }>;
type Preview = {
  author: string;
  originalWire: string | null;
  baseId: string | null;
  wire: string;
  hash: string;
  policyFingerprint: string;
  selectedInboxes: string[];
  destinations: string[];
  removedTagIndices: number[];
  removedExtraFields: string[];
  globalEffect: string;
};
type Saved = Readonly<{
  identity: IdentitySession;
  session: symbol;
  policy: RelayPolicy;
  previewWire: string;
}>;
const reviews = new WeakMap<InboxSetupReview, Saved>();
const globalEffect =
  'This replaces your public inbox preference for other clients. Relay acceptance and readback are still required; this review does not publish.';
function base(resolver: InboxResolver, policy: RelayPolicy, owner: string) {
  const resolution = inboxResolutionSnapshot(resolver),
    discovery = publicRelayTargets(policy, 'read');
  if (
    resolution.author !== owner ||
    resolution.coverage !== 'bounded-eose' ||
    resolution.status === 'inconclusive' ||
    discovery.length === 0 ||
    discovery.length !== resolution.sources.length ||
    !discovery.every((origin) =>
      resolution.sources.some(
        (row) => row.source === origin && row.state === 'eose'
      )
    )
  )
    return undefined;
  if (resolution.status === 'missing')
    return !resolution.head && !resolution.knownBase
      ? { wire: null, id: null, createdAt: undefined }
      : undefined;
  const head = resolution.head && inboxPreferenceSnapshot(resolution.head),
    wire = resolution.head && inboxPreferenceWire(resolution.head);
  return head &&
    wire &&
    head.author === owner &&
    head.id === resolution.knownBase?.id
    ? { wire, id: head.id, createdAt: head.createdAt }
    : undefined;
}
// Explicit source review only. The clock is an internal observation port like
// the resolver clock; production uses actual wall time, tests control it. No
// caller event extras enter the canonical signing contract implicitly.
export async function reviewInboxSetup(
  identity: IdentitySession,
  resolver: InboxResolver,
  policy: RelayPolicy,
  raw: unknown,
  observeNow: () => number = () => Math.floor(Date.now() / 1000)
): Promise<
  | Readonly<{ status: 'review'; review: InboxSetupReview }>
  | Readonly<{ status: 'blocked'; reason: string }>
> {
  try {
    const capture = identityMessagingOwnership(identity),
      manifest = readRelayPolicy(policy);
    if (
      !capture ||
      !capture.current() ||
      !manifest.messagingEnabled ||
      !manifest.postingEnabled
    )
      return { status: 'blocked', reason: 'unavailable' };
    const previous = base(resolver, policy, capture.owner);
    if (!previous || typeof raw !== 'string' || !boundedUtf8(raw, 65536))
      return { status: 'blocked', reason: 'unavailable' };
    const input: unknown = JSON.parse(raw);
    if (
      !exactLocalFields(input, [
        'selectedInboxes',
        'removeTagIndices',
        'removeExtraFields',
        'createdAt'
      ]) ||
      !Array.isArray(input.selectedInboxes) ||
      input.selectedInboxes.length < 1 ||
      input.selectedInboxes.length > 3 ||
      !Array.isArray(input.removeTagIndices) ||
      !Array.isArray(input.removeExtraFields)
    )
      return { status: 'blocked', reason: 'invalid_selection' };
    const time = safeUnsignedInteger(input.createdAt),
      now = safeUnsignedInteger(observeNow());
    if (
      time === undefined ||
      now !== time ||
      (previous.createdAt !== undefined && time <= previous.createdAt)
    )
      return { status: 'blocked', reason: 'clock_conflict' };
    const selected: string[] = [];
    function appendSelected(value: string) {
      selected.push(value);
    }
    for (const value of input.selectedInboxes as unknown[]) {
      if (
        typeof value !== 'string' ||
        selected.includes(value) ||
        !inboxRelayTargets(policy, [value], 'read').includes(value) ||
        !inboxRelayTargets(policy, [value], 'write').includes(value)
      )
        return { status: 'blocked', reason: 'invalid_selection' };
      appendSelected(value);
    }
    const old =
      previous.wire === null
        ? undefined
        : (JSON.parse(previous.wire) as { tags: string[][]; content: string });
    const tags = old?.tags ?? [];
    const removeTags: number[] = [],
      removeFields: string[] = [];
    function appendTagIndex(value: number) {
      removeTags.push(value);
    }
    function appendField(value: string) {
      removeFields.push(value);
    }
    if (
      input.removeTagIndices.length > tags.length ||
      input.removeExtraFields.length > 64
    )
      return { status: 'blocked', reason: 'invalid_selection' };
    for (const index of input.removeTagIndices as unknown[]) {
      if (
        typeof index !== 'number' ||
        safeUnsignedInteger(index) === undefined ||
        index >= tags.length ||
        removeTags.includes(index)
      )
        return { status: 'blocked', reason: 'invalid_selection' };
      appendTagIndex(index);
    }
    const standard = [
      'id',
      'sig',
      'pubkey',
      'kind',
      'created_at',
      'tags',
      'content'
    ];
    const extras: string[] = [];
    function appendExtra(value: string) {
      extras.push(value);
    }
    if (old)
      for (const key in old) if (!standard.includes(key)) appendExtra(key);
    for (const key of input.removeExtraFields as unknown[]) {
      if (
        typeof key !== 'string' ||
        !extras.includes(key) ||
        removeFields.includes(key)
      )
        return { status: 'blocked', reason: 'invalid_selection' };
      appendField(key);
    }
    // Unsigned extra fields cannot survive the existing exact five-field signer.
    // Block until the owner chooses every removal; retaining only the old source
    // would not constitute preservation in the outgoing replacement.
    if (extras.some((key) => !removeFields.includes(key)))
      return { status: 'blocked', reason: 'unsupported_extra_fields' };
    let nextTags = tags.filter((_tag, index) => !removeTags.includes(index));
    for (const origin of selected)
      if (
        !nextTags.some(
          (tag) => tag.length === 2 && tag[0] === 'relay' && tag[1] === origin
        )
      )
        nextTags = nextTags.concat([['relay', origin]]);
    // Reuse the public preference grammar without signing a synthetic event:
    // every retained relay tag must remain a canonical advertised origin. Other
    // tags/content/order/duplicates survive unchanged, including >3 advertisements.
    if (
      nextTags.some(
        (tag) =>
          tag[0] === 'relay' &&
          (tag.length !== 2 || !canonicalRelayOrigin(tag[1]))
      )
    )
      return { status: 'blocked', reason: 'unsupported_profile' };
    const wire = JSON.stringify({
      pubkey: capture.owner,
      kind: 10050,
      created_at: time,
      tags: nextTags,
      content: old?.content ?? ''
    });
    const template = inspectLocalTemplate(wire, capture.owner, 10050),
      destinations = publicRelayTargets(policy, 'write');
    if (!template || destinations.length === 0)
      return { status: 'blocked', reason: 'unsupported_profile' };
    const digest = await crypto.subtle.digest(
      'SHA-256',
      new TextEncoder().encode(JSON.stringify(manifest))
    );
    const policyFingerprint = Array.from(new Uint8Array(digest), (value) =>
      value.toString(16).padStart(2, '0')
    ).join('');
    if (!capture.current() || !inboxResolverCurrentAfterSample(resolver))
      return { status: 'blocked', reason: 'unavailable' };
    const preview: Preview = {
      author: capture.owner,
      originalWire: previous.wire,
      baseId: previous.id,
      wire,
      hash: template.hash,
      policyFingerprint,
      selectedInboxes: selected,
      destinations: destinations.slice(),
      removedTagIndices: removeTags,
      removedExtraFields: removeFields,
      globalEffect
    };
    const review = Object.freeze({}) as InboxSetupReview;
    reviews.set(review, {
      identity,
      session: capture.session,
      policy,
      previewWire: JSON.stringify(preview)
    });
    return { status: 'review', review };
  } catch {
    return { status: 'blocked', reason: 'unavailable' };
  }
}
export function inboxSetupPreview(
  review: InboxSetupReview
): Preview | undefined {
  const saved = reviews.get(review);
  return saved ? (JSON.parse(saved.previewWire) as Preview) : undefined;
}
export type InboxSetupCapture =
  | Readonly<{ status: 'captured'; record: PublicRecordHandle; owner: string }>
  | Readonly<{ status: 'invalid' | 'conflict' | 'unavailable' }>;
// No effect permission: this exact reviewed record must be durably admitted
// before later signing/publication. Fresh lookup is bounded knowledge, never CAS.
export function captureInboxSetup(
  review: InboxSetupReview,
  resolver: InboxResolver,
  command: unknown,
  consent: unknown
): InboxSetupCapture {
  try {
    const saved = reviews.get(review),
      id = canonicalLocalId(command);
    if (!saved || !id || consent !== 'reviewed_global_inbox_replacement')
      return { status: 'invalid' };
    const preview = JSON.parse(saved.previewWire) as Preview,
      capture = identityMessagingOwnership(saved.identity);
    if (
      !capture ||
      capture.owner !== preview.author ||
      capture.session !== saved.session
    )
      return { status: 'unavailable' };
    const current = base(resolver, saved.policy, preview.author);
    if (!current) return { status: 'unavailable' };
    if (current.id !== preview.baseId || current.wire !== preview.originalWire)
      return { status: 'conflict' };
    const record = decodePublicRecord(
      JSON.stringify({
        schema: 1,
        family: 'preference_operation',
        owner: preview.author,
        id,
        revision: 0,
        source: { type: 'inbox_head', wire: preview.originalWire },
        consent: 'explicit_review',
        capture: {
          kind: 10050,
          wire: preview.wire,
          hash: preview.hash,
          targets: preview.destinations,
          policyFingerprint: preview.policyFingerprint
        },
        artifact: null,
        receipts: []
      }),
      preview.author,
      id
    );
    if (
      !record.ok ||
      !capture.current() ||
      !inboxResolverCurrentAfterSample(resolver)
    )
      return { status: 'unavailable' };
    return { status: 'captured', record: record.value, owner: preview.author };
  } catch {
    return { status: 'unavailable' };
  }
}
