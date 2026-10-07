import { publicRelayTargets, type RelayPolicy } from '../config/relays.ts';
import {
  publicRecordSnapshot,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  verifiedEnvelopeWire
} from './verified-envelope.ts';
import {
  publicViewRunCurrent,
  publicViewOwnsRun,
  subscribeInboxPreference,
  type PublicView
} from '../runtime/public-runtime.ts';
import { publicRequestScopeSnapshot, type PublicRun } from './request-scope.ts';
import {
  createInboxResolver,
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  closeInboxResolver,
  type InboxResolver
} from '../messaging/resolve-inbox.ts';
declare const readbackBrand: unique symbol;
export type InboxReadback = Readonly<{ [readbackBrand]: true }>;
type Owner = Readonly<{
  owner: string;
  id: string;
  record: PublicRecordHandle;
  policy: RelayPolicy;
  resolver: InboxResolver;
  wires: Map<string, string>;
  current(): boolean;
  closed(): boolean;
  close(): void;
  use(): boolean;
}>;
const owners = new WeakMap<InboxReadback, Owner>();
const reserved = new WeakMap<PublicRun, true>();
// Read only through the existing bounded anonymous inbox request. No raw SDK,
// arbitrary relay/filter, event publication or inferred seen-relay provenance.
export function resolveInboxReadback(
  view: PublicView,
  run: PublicRun,
  record: PublicRecordHandle,
  owner: string,
  id: string,
  policy: RelayPolicy
): InboxReadback | undefined {
  try {
    if (typeof window === 'undefined' || reserved.has(run)) return undefined;
    reserved.set(run, true);
    const row = publicRecordSnapshot(record, owner, id);
    if (
      row?.family !== 'preference_operation' ||
      !row.artifact ||
      row.revision < 2 ||
      !publicViewRunCurrent(view, run)
    )
      return undefined;
    const verified = verifyEnvelope(row.artifact.wire);
    if (!verified.ok) return undefined;
    const exact = JSON.stringify(verifiedEnvelopeSnapshot(verified.value));
    const sources = publicRelayTargets(policy, 'read'),
      wires = new Map<string, string>();
    let closed = false,
      used = false;
    const resolver = createInboxResolver(
      run,
      owner,
      (next) =>
        subscribeInboxPreference(view, run, owner, (proof, source) => {
          if (closed || !publicViewOwnsRun(view, run)) return;
          // This source comes from admitted request dispatch, bound to this exact
          // proof, not a journal that knows only an ID or a local artifact copy.
          if (
            source !== undefined &&
            sources.includes(source) &&
            JSON.stringify(verifiedEnvelopeSnapshot(proof)) === exact
          ) {
            const wire = verifiedEnvelopeWire(proof);
            if (wire !== undefined) wires.set(source, wire);
          }
          next(proof);
        }),
      () => !closed && publicViewOwnsRun(view, run)
    );
    const token = Object.freeze({}) as InboxReadback;
    owners.set(token, {
      owner,
      id,
      record,
      policy,
      resolver,
      wires,
      current: () =>
        !closed &&
        publicViewOwnsRun(view, run) &&
        inboxResolverCurrentAfterSample(resolver),
      closed: () => closed,
      close() {
        closed = true;
        closeInboxResolver(resolver);
      },
      use() {
        if (closed || used) return false;
        used = true;
        return true;
      }
    });
    return token;
  } catch {
    return undefined;
  }
}
export function inboxReadbackSnapshot(token: InboxReadback) {
  try {
    const saved = owners.get(token);
    if (!saved || saved.closed()) return undefined;
    const row = publicRecordSnapshot(saved.record, saved.owner, saved.id);
    if (row?.family !== 'preference_operation' || !row.artifact)
      return undefined;
    const resolution = inboxResolutionSnapshot(saved.resolver),
      scope = publicRequestScopeSnapshot(resolution.request);
    const sources = publicRelayTargets(saved.policy, 'read');
    const complete =
      resolution.coverage === 'bounded-eose' &&
      scope.inboxSettled === true &&
      resolution.author === saved.owner &&
      sources.length > 0 &&
      sources.length === resolution.sources.length &&
      sources.every((source) =>
        resolution.sources.some(
          (row) => row.source === source && row.state === 'eose'
        )
      );
    const knownHead = resolution.knownBase && {
      id: resolution.knownBase.id,
      createdAt: resolution.knownBase.createdAt
    };
    const current = saved.current();
    return {
      owner: saved.owner,
      id: saved.id,
      artifactId: row.artifact.eventId,
      knownHead,
      status: !current
        ? ('unknown' as const)
        : scope.state === 'active'
          ? ('pending' as const)
          : !complete
            ? ('unknown' as const)
            : knownHead && knownHead.id !== row.artifact.eventId
              ? ('conflict' as const)
              : knownHead?.id === row.artifact.eventId && saved.wires.size > 0
                ? ('readback' as const)
                : ('missing' as const),
      sources: Array.from(saved.wires.keys()),
      complete: complete && current,
      coverage: resolution.coverage,
      definitiveAbsence: false as const
    };
  } catch {
    return undefined;
  }
}
// Internal genuine owner observation. Wire is public, independently verified
// exact signed readback; this confers no signing or network permission.
export function inboxReadbackEvidence(token: InboxReadback) {
  const saved = owners.get(token),
    snapshot = inboxReadbackSnapshot(token);
  if (!saved || !snapshot || !snapshot.complete || !saved.current())
    return undefined;
  return {
    ...snapshot,
    record: saved.record,
    policy: saved.policy,
    resolver: saved.resolver,
    wires: Array.from(saved.wires, ([origin, wire]) => ({ origin, wire })),
    current: saved.current
  };
}
export function useInboxReadback(token: InboxReadback): boolean {
  return owners.get(token)?.use() ?? false;
}
export function closeInboxReadback(token: InboxReadback): void {
  owners.get(token)?.close();
}
