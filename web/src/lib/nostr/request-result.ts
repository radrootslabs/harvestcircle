import {
  PUBLIC_INGRESS_BUDGETS,
  PUBLIC_QUERY_BUDGETS,
  PUBLIC_NIP50_BUDGETS
} from '../config/budgets.ts';
import { requireNip50Source } from './search-sources.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { publicRelayTargets, type RelayPolicy } from '../config/relays.ts';
import {
  createObservationContext,
  recordPublicObservation,
  observationSourceOrigins,
  type ObservationJournal,
  type ObservationContext
} from '../catalog/observations.ts';
import {
  admitPublicEvent,
  admitInboxEvent,
  publicIngressStats,
  type PublicIngress
} from './ingress.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';
import { isPublicEnvelopeKind } from './public-store.ts';
import type { PublicPoolMessage } from './exports.ts';
declare const requestBrand: unique symbol;
export type PublicRequestResult = Readonly<{ readonly [requestBrand]: true }>;
export type SourceState =
  'pending' | 'eose' | 'closed' | 'error' | 'limit' | 'cancelled';
export type SourceOutcome = Readonly<{
  source: string;
  state: SourceState;
  candidates: number;
  accepted: number;
  duplicates: number;
  rejected: number;
}>;
export type RequestSnapshot = Readonly<{
  context: ObservationContext;
  sources: readonly SourceOutcome[];
  coverage: 'bounded-eose' | 'partial';
  definitiveAbsence: false;
  inactive: boolean;
  inbox?: Readonly<{ author: string; heads: readonly InboxSourceHead[] }>;
}>;
export type InboxSourceHead = Readonly<{
  source: string;
  id: string;
  createdAt: number;
}>;
export type RequestMessageResult =
  | Readonly<{ status: 'accepted' | 'duplicate'; value: VerifiedEnvelope }>
  | Readonly<{
      status:
        | 'rejected'
        | 'limit'
        | 'not_public'
        | 'source_unknown'
        | 'inactive'
        | 'control';
    }>;
interface Owner {
  readonly handle: (message: PublicPoolMessage) => RequestMessageResult;
  readonly snapshot: () => RequestSnapshot;
  readonly dispose: () => void;
}
const owners = new WeakMap<PublicRequestResult, Owner>();
const runIngress = new WeakMap<ObservationJournal, PublicIngress>();
// Scalar state remains inside its own closure; unknown Map-returned records
// are never treated as mutable DOM/model receivers by the source guard.
function sourceOwner(source: string) {
  let state: SourceState = 'pending';
  let candidates = 0;
  let accepted = 0;
  let duplicates = 0;
  let rejected = 0;
  return {
    state: () => state,
    candidate() {
      candidates = Math.min(PUBLIC_INGRESS_BUDGETS.deliveries, candidates + 1);
    },
    reject() {
      rejected = Math.min(PUBLIC_INGRESS_BUDGETS.deliveries, rejected + 1);
    },
    admit(duplicate: boolean) {
      if (duplicate) duplicates++;
      else accepted++;
    },
    control(next: 'eose' | 'error' | 'closed' | 'limit') {
      if (next === 'limit') state = 'limit';
      else if (next === 'eose') {
        if (state === 'pending') state = 'eose';
      } else if (state !== 'limit' && state !== 'cancelled') state = next;
    },
    cancel() {
      if (state === 'pending') state = 'cancelled';
    },
    snapshot(): SourceOutcome {
      return {
        source: source,
        state: state,
        candidates: candidates,
        accepted: accepted,
        duplicates: duplicates,
        rejected: rejected
      };
    }
  };
}

// Pure decoded-SDK adapter. Network callbacks bind this opaque logical request;
// HCP032 owns actual deadline/concurrency/generation and browser composition.
export function createPublicRequestResult(
  policy: RelayPolicy,
  ingress: PublicIngress,
  journal: ObservationJournal,
  sampleSource?: string
): PublicRequestResult {
  return createResult(policy, ingress, journal, sampleSource);
}
export function createInboxRequestResult(
  policy: RelayPolicy,
  ingress: PublicIngress,
  journal: ObservationJournal,
  author: unknown
): PublicRequestResult {
  const key = canonicalPublicKey(author);
  if (!key) throw new Error('inbox_author_invalid');
  return createResult(policy, ingress, journal, undefined, key);
}
function createResult(
  policy: RelayPolicy,
  ingress: PublicIngress,
  journal: ObservationJournal,
  sampleSource?: string,
  inboxAuthor?: string
): PublicRequestResult {
  publicIngressStats(ingress);
  const allOrigins = publicRelayTargets(policy, 'read');
  const expected = observationSourceOrigins(journal);
  if (
    allOrigins.length !== expected.length ||
    !allOrigins.every((origin, index) => origin === expected[index])
  )
    throw new Error('request_observation_policy_changed');
  const origins =
    sampleSource === undefined
      ? allOrigins
      : [requireNip50Source(policy, sampleSource)];
  const priorIngress = runIngress.get(journal);
  if (priorIngress && priorIngress !== ingress)
    throw new Error('public_request_ingress_changed');
  const context = createObservationContext(journal);
  runIngress.set(journal, ingress);
  const sources = new Map<string, ReturnType<typeof sourceOwner>>();
  const heads = new Map<string, InboxSourceHead>();
  for (const source of origins) sources.set(source, sourceOwner(source));
  let inactive = false;
  let unknownDelivery = false;
  const owner: Owner = {
    handle(message) {
      if (inactive) return { status: 'inactive' };
      // Pinned SDK normalized relay URLs may carry the origin's sole trailing
      // slash. Match only the two exact spellings of an already approved origin.
      const origin = origins.find(
        (value) => message.from === value || message.from === value + '/'
      );
      const source = origin === undefined ? undefined : sources.get(origin);
      if (message.type === 'EVENT') {
        const admitted =
          inboxAuthor === undefined
            ? admitPublicEvent(ingress, message.event)
            : admitInboxEvent(ingress, message.event);
        if (!source || origin === undefined) {
          unknownDelivery = true;
          return { status: 'source_unknown' };
        }
        const pending = source.state() === 'pending';
        source.candidate();
        // Preserve the inclusive100th admitted delivery, then settle this sample
        // alone. Global ingress overflow retains its existing run-wide semantics.
        if (
          sampleSource !== undefined &&
          source.snapshot().candidates >=
            PUBLIC_NIP50_BUDGETS.candidatesPerSource
        )
          source.control('limit');
        if (
          inboxAuthor !== undefined &&
          source.snapshot().candidates >= PUBLIC_QUERY_BUDGETS.requestedPerRelay
        )
          source.control('limit');
        if (admitted.status === 'limit') {
          source.control('limit');
          return { status: 'limit' };
        }
        if (admitted.status === 'rejected') {
          source.reject();
          return { status: 'rejected' };
        }
        if (!pending) {
          source.reject();
          return { status: 'inactive' };
        }
        if (!('value' in admitted)) return { status: 'rejected' };
        const event = verifiedEnvelopeSnapshot(admitted.value);
        if (
          !event ||
          (inboxAuthor === undefined
            ? !isPublicEnvelopeKind(event.kind)
            : event.kind !== 10050 || event.pubkey !== inboxAuthor)
        ) {
          source.reject();
          return { status: 'not_public' };
        }
        if (
          inboxAuthor === undefined &&
          !recordPublicObservation(journal, context, origin, admitted.value)
        ) {
          source.control('limit');
          return { status: 'limit' };
        }
        if (inboxAuthor !== undefined) {
          const previous = heads.get(origin);
          if (
            !previous ||
            event.created_at > previous.createdAt ||
            (event.created_at === previous.createdAt && event.id < previous.id)
          )
            heads.set(origin, {
              source: origin,
              id: event.id,
              createdAt: event.created_at
            });
        }
        source.admit(admitted.status === 'duplicate');
        // Record exact source evidence BEFORE any caller inserts/deduplicates into
        // EventStore. Only verified proof leaves this boundary, never SDK metadata.
        return admitted;
      }
      if (!source) return { status: 'source_unknown' };
      if (message.type === 'EOSE') source.control('eose');
      else if (message.type === 'ERROR') source.control('error');
      else if (message.type === 'CLOSED') source.control('closed');
      // Deliberately do not read message.reason, message.error, filters or IDs.
      return { status: 'control' };
    },
    snapshot() {
      const outcomes = Array.from(sources.values(), (row) => row.snapshot());
      const complete =
        outcomes.length > 0 &&
        (inboxAuthor === undefined || !unknownDelivery) &&
        outcomes.every((row) => row.state === 'eose' && row.rejected === 0);
      return {
        context: context,
        sources: outcomes,
        coverage: complete ? 'bounded-eose' : 'partial',
        definitiveAbsence: false,
        inactive: inactive,
        ...(inboxAuthor === undefined
          ? {}
          : {
              inbox: {
                author: inboxAuthor,
                heads: Array.from(heads.values(), (row) => ({ ...row }))
              }
            })
      };
    },
    dispose() {
      if (inactive) return;
      inactive = true;
      for (const row of sources.values()) row.cancel();
    }
  };
  const token = Object.freeze({}) as PublicRequestResult;
  owners.set(token, owner);
  return token;
}
function ownerOf(token: PublicRequestResult): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('public_request_result_invalid');
  return owner;
}
export function handlePublicRequestMessage(
  owner: PublicRequestResult,
  message: PublicPoolMessage
): RequestMessageResult {
  return ownerOf(owner).handle(message);
}
export function publicRequestSnapshot(
  owner: PublicRequestResult
): RequestSnapshot {
  return ownerOf(owner).snapshot();
}
export function disposePublicRequestResult(owner: PublicRequestResult): void {
  ownerOf(owner).dispose();
}
