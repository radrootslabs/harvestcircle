import {
  createIdentitySession,
  identitySessionSnapshot,
  connectIdentity,
  disconnectIdentity,
  probeIdentityMessaging,
  invalidateIdentityOperations,
  type IdentitySession,
  type IdentitySnapshot
} from './identity-session.ts';
declare const viewBrand: unique symbol;
export type IdentityViewContext = Readonly<{ [viewBrand]: true }>;
export type IdentityViewSnapshot = Readonly<{
  mounted: boolean;
  identity: IdentitySnapshot;
}>;
export type IdentityViewActionResult = IdentityViewSnapshot &
  Readonly<{ current(): boolean }>;
export const IDENTITY_VIEW_CONTEXT = 'harvestcircle_identity_view';
type Listener = (state: IdentityViewSnapshot) => void;
type Controller = {
  session: IdentitySession;
  snapshot(): IdentityViewSnapshot;
  mount(): boolean;
  connect(): Promise<IdentityViewActionResult>;
  probe(review: unknown): Promise<IdentityViewActionResult>;
  disconnect(): void;
  close(): void;
  subscribe(listener: Listener): () => void;
  invalidate(): void;
};
const contexts = new WeakMap<IdentityViewContext, Controller>();
const unavailable = (): IdentityViewSnapshot => ({
  mounted: false,
  identity: { state: 'guest', reason: 'unavailable' }
});
// Per-client context construction is SSR-pure. Observed key/capability never
// authorizes private content, installed storage, inbox readiness or publication.
export function createIdentityViewContext(): IdentityViewContext {
  const token = Object.freeze({}) as IdentityViewContext;
  const session = createIdentitySession();
  const listeners = new Map<symbol, Listener>();
  let mounted = false,
    closed = false,
    busy = false,
    generation = Symbol(),
    viewGeneration = Symbol(),
    actionGeneration = Symbol();
  const snapshot = (): IdentityViewSnapshot =>
    closed
      ? unavailable()
      : {
          mounted,
          identity: busy
            ? { ...identitySessionSnapshot(session), admission: 'busy' }
            : identitySessionSnapshot(session)
        };
  function notify() {
    for (const listener of listeners.values()) {
      try {
        listener(snapshot());
      } catch {
        /* A presentation observer cannot interrupt owned SDK settlement. */
      }
    }
  }
  async function action(work: () => Promise<IdentitySnapshot>) {
    if (!mounted || closed) return { ...snapshot(), current: () => false };
    busy = false;
    actionGeneration = Symbol();
    const originalAction = actionGeneration,
      original = generation,
      originalView = viewGeneration,
      pending = work();
    notify();
    const result = await pending;
    if (!closed && original === generation) {
      busy = result.admission === 'busy';
      notify();
    }
    return {
      ...snapshot(),
      current: () =>
        !closed &&
        mounted &&
        actionGeneration === originalAction &&
        generation === original &&
        viewGeneration === originalView
    };
  }
  contexts.set(token, {
    session,
    snapshot,
    invalidate() {
      viewGeneration = Symbol();
      invalidateIdentityOperations(session);
    },
    mount() {
      if (closed || typeof window === 'undefined') return false;
      mounted = true;
      notify();
      return true;
    },
    connect: () => action(() => connectIdentity(session)),
    probe: (review) => action(() => probeIdentityMessaging(session, review)),
    disconnect() {
      if (closed) return;
      generation = Symbol();
      busy = false;
      disconnectIdentity(session);
      notify();
    },
    close() {
      if (closed) return;
      generation = Symbol();
      busy = false;
      disconnectIdentity(session);
      closed = true;
      mounted = false;
      notify();
      listeners.clear();
    },
    subscribe(listener) {
      if (closed) return () => {};
      const subscription = Symbol();
      listeners.set(subscription, listener);
      try {
        listener(snapshot());
      } catch {
        /* Isolate presentation observers. */
      }
      return () => {
        listeners.delete(subscription);
      };
    }
  });
  return token;
}
export function identityViewSnapshot(
  context: IdentityViewContext
): IdentityViewSnapshot {
  return contexts.get(context)?.snapshot() ?? unavailable();
}
export function mountIdentityView(context: IdentityViewContext): boolean {
  return contexts.get(context)?.mount() ?? false;
}
export function connectIdentityView(
  context: IdentityViewContext
): Promise<IdentityViewActionResult> {
  return (
    contexts.get(context)?.connect() ??
    Promise.resolve({ ...unavailable(), current: () => false })
  );
}
export function probeIdentityViewMessaging(
  context: IdentityViewContext,
  review: unknown
): Promise<IdentityViewActionResult> {
  return (
    contexts.get(context)?.probe(review) ??
    Promise.resolve({ ...unavailable(), current: () => false })
  );
}
export function disconnectIdentityView(context: IdentityViewContext): void {
  contexts.get(context)?.disconnect();
}
export function closeIdentityView(context: IdentityViewContext): void {
  contexts.get(context)?.close();
}
export function subscribeIdentityView(
  context: IdentityViewContext,
  listener: Listener
): () => void {
  return contexts.get(context)?.subscribe(listener) ?? (() => {});
}
export function invalidateIdentityView(context: IdentityViewContext): void {
  const owned = contexts.get(context);
  owned?.invalidate();
}
// Trusted client orchestration receives only the original opaque session. This
// is not effect permission; each workflow still rechecks its genuine owner.
export function identityViewSession(
  context: IdentityViewContext
): IdentitySession | undefined {
  const owner = contexts.get(context);
  return owner?.snapshot().mounted ? owner.session : undefined;
}
// Mechanical observed-owner comparison only, never private authorization.
export function identityViewMatchesOwner(
  context: IdentityViewContext,
  owner: unknown
): boolean {
  const state = contexts.get(context)?.snapshot();
  return (
    state?.mounted === true &&
    state.identity.state !== 'guest' &&
    state.identity.state !== 'pending' &&
    state.identity.publicKey === owner
  );
}
