import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';
import { publicRelayTargets, type RelayPolicy } from '../config/relays.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import { isPublicEnvelopeKind } from '../nostr/public-store.ts';
declare const journalBrand: unique symbol;
declare const contextBrand: unique symbol;
export type ObservationJournal = Readonly<{ readonly [journalBrand]: true }>;
export type ObservationContext = Readonly<{ readonly [contextBrand]: true }>;
export type PublicObservation = Readonly<{
  eventId: string;
  source: string;
  kind: number;
}>;
interface Owner {
  readonly origins: () => readonly string[];
  readonly context: () => ObservationContext;
  readonly record: (
    context: ObservationContext,
    source: string,
    event: VerifiedEnvelope
  ) => boolean;
  readonly exact: (
    context: ObservationContext,
    eventId: string,
    source: string
  ) => boolean;
  readonly rows: (context: ObservationContext) => readonly PublicObservation[];
  readonly stats: () => Readonly<{ rows: number; closed: boolean }>;
  readonly close: () => void;
}
const owners = new WeakMap<ObservationJournal, Owner>();
// One journal belongs to a public run; request tokens are local identities,
// never relay-supplied IDs. No SDK seen-relay metadata or receipt inference.
export function createObservationJournal(
  policy: RelayPolicy
): ObservationJournal {
  const origins = publicRelayTargets(policy, 'read');
  let contexts = new WeakMap<ObservationContext, Map<string, string>>();
  let rows = 0;
  let closed = false;
  function contextRows(context: ObservationContext): Map<string, string> {
    const result = contexts.get(context);
    if (!result) throw new Error('observation_context_invalid');
    return result;
  }
  const owner: Owner = {
    origins: () => [...origins],
    context() {
      if (closed) throw new Error('observations_closed');
      const token = Object.freeze({}) as ObservationContext;
      contexts.set(token, new Map());
      return token;
    },
    record(context, source, event) {
      if (closed) return false;
      const records = contextRows(context);
      if (!origins.includes(source)) return false;
      const snapshot = verifiedEnvelopeSnapshot(event);
      if (!snapshot || !isPublicEnvelopeKind(snapshot.kind)) return false;
      const key = source + '\0' + snapshot.id;
      if (records.has(key)) return true;
      if (rows >= PUBLIC_INGRESS_BUDGETS.deliveries) return false;
      // Retain only exact public ID/kind/source. Private/decrypted metadata and
      // full payloads never enter this bounded observation cache.
      records.set(
        key,
        JSON.stringify({
          eventId: snapshot.id,
          source: source,
          kind: snapshot.kind
        })
      );
      rows++;
      return true;
    },
    exact(context, eventId, source) {
      if (closed) return false;
      const records = contextRows(context);
      return (
        typeof eventId === 'string' &&
        /^[0-9a-f]{64}$/u.test(eventId) &&
        origins.includes(source) &&
        records.has(source + '\0' + eventId)
      );
    },
    rows(context) {
      if (closed) return [];
      return Array.from(
        contextRows(context).values(),
        (raw) => JSON.parse(raw) as PublicObservation
      );
    },
    stats() {
      return Object.freeze({ rows: rows, closed: closed });
    },
    close() {
      closed = true;
      contexts = new WeakMap();
      rows = 0;
    }
  };
  const token = Object.freeze({}) as ObservationJournal;
  owners.set(token, owner);
  return token;
}
function ownerOf(journal: ObservationJournal): Owner {
  const owner = owners.get(journal);
  if (!owner) throw new Error('observation_journal_invalid');
  return owner;
}
export function createObservationContext(
  journal: ObservationJournal
): ObservationContext {
  return ownerOf(journal).context();
}
export function recordPublicObservation(
  journal: ObservationJournal,
  context: ObservationContext,
  source: string,
  event: VerifiedEnvelope
): boolean {
  return ownerOf(journal).record(context, source, event);
}
export function exactPublicObservation(
  journal: ObservationJournal,
  context: ObservationContext,
  eventId: string,
  source: string
): boolean {
  return ownerOf(journal).exact(context, eventId, source);
}
export function publicObservations(
  journal: ObservationJournal,
  context: ObservationContext
): readonly PublicObservation[] {
  return ownerOf(journal).rows(context);
}
export function observationJournalStats(
  journal: ObservationJournal
): Readonly<{ rows: number; closed: boolean }> {
  return ownerOf(journal).stats();
}
export function closeObservationJournal(journal: ObservationJournal): void {
  ownerOf(journal).close();
}

export function observationSourceOrigins(
  journal: ObservationJournal
): readonly string[] {
  return ownerOf(journal).origins();
}
