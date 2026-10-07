import { canonicalPublicKey } from '../contracts/public-key.ts';

declare const schedulerBrand: unique symbol;
declare const actionBrand: unique symbol;
export type ExtensionScheduler = Readonly<{ [schedulerBrand]: true }>;
export type ExtensionAction = Readonly<{ [actionBrand]: true }>;
export type ExtensionCallKind = 'key' | 'sign' | 'encrypt' | 'decrypt' | 'auth';
export type ExtensionCapture = Readonly<{
  owner: string | null;
  session: symbol;
  operation: symbol;
}>;
type Phase = 'active' | 'denied' | 'stopped' | 'wait_expired';
export type ExtensionCallResult<T> =
  | Readonly<{
      status: 'settled';
      value: T;
      current: boolean;
      capture: ExtensionCapture;
    }>
  | Readonly<{
      status: 'invalid' | 'busy' | 'denied' | 'stopped' | 'wait_expired';
    }>;
export type ExtensionActionResult<T> =
  | Readonly<{
      status: 'completed' | 'denied' | 'stopped' | 'wait_expired';
      capture: ExtensionCapture;
      value?: T;
    }>
  | Readonly<{ status: 'invalid' | 'busy' }>;
export type ExtensionSchedulerSnapshot =
  | Readonly<{ state: 'idle' | 'unavailable' }>
  | Readonly<{
      state: 'active';
      pending: ExtensionCallKind | null;
      phase: Phase;
    }>;
type ActionController = {
  call<T>(
    kind: ExtensionCallKind,
    invoke: () => Promise<T>
  ): Promise<ExtensionCallResult<T>>;
  stop(): void;
  expire(): void;
};
type SchedulerController = {
  run<T>(
    capture: ExtensionCapture,
    current: () => boolean,
    work: (action: ExtensionAction) => Promise<T>
  ): Promise<ExtensionActionResult<T>>;
  snapshot(): ExtensionSchedulerSnapshot;
};
const schedulers = new WeakMap<ExtensionScheduler, SchedulerController>();
const actions = new WeakMap<ExtensionAction, ActionController>();
// A page-local permission slot, not a server account or cross-tab lock. No
// accessor is called at module load; page destruction ends this realm's state.
let browserScheduler: ExtensionScheduler | undefined;
export function browserExtensionScheduler(): ExtensionScheduler | undefined {
  if (typeof window === 'undefined') return undefined;
  browserScheduler ??= createExtensionScheduler();
  return browserScheduler;
}
export function createExtensionScheduler(): ExtensionScheduler {
  let occupied = false;
  let view: () => ExtensionSchedulerSnapshot = () => ({ state: 'idle' });
  async function run<T>(
    input: ExtensionCapture,
    fresh: () => boolean,
    work: (action: ExtensionAction) => Promise<T>
  ): Promise<ExtensionActionResult<T>> {
    if (occupied) return { status: 'busy' };
    // Reserve before reading an external capture or invoking freshness. Neither
    // a getter nor a reentrant callback can admit a second action.
    occupied = true;
    let phase: Phase = 'active',
      closed = false,
      accepting = true;
    let pending: ExtensionCallKind | null = null;
    let settlement: Promise<unknown> | null = null;
    function outstanding(): Promise<unknown> | null {
      return settlement;
    }
    view = () => ({ state: 'active', pending, phase });
    function current() {
      if (closed || phase !== 'active') return false;
      let valid = false;
      try {
        valid = fresh() === true;
      } catch {
        /* finite stopped state */
      }
      if (!valid && phase === 'active') phase = 'stopped';
      return !closed && phase === 'active' && valid;
    }
    try {
      if (!input || typeof fresh !== 'function' || typeof work !== 'function')
        return { status: 'invalid' };
      const owner = input.owner,
        session = input.session,
        operation = input.operation;
      if (
        (owner !== null &&
          (typeof owner !== 'string' || canonicalPublicKey(owner) !== owner)) ||
        typeof session !== 'symbol' ||
        typeof operation !== 'symbol'
      )
        return { status: 'invalid' };
      // The authoritative capture consists only of immutable lexical primitives.
      // Each outward result is a fresh detached copy, never the retained input.
      const capture = (): ExtensionCapture => ({ owner, session, operation });
      const action = Object.freeze({}) as ExtensionAction;
      async function call<V>(
        kind: ExtensionCallKind,
        invoke: () => Promise<V>
      ): Promise<ExtensionCallResult<V>> {
        if (!accepting) return { status: 'stopped' };
        if (!current())
          return {
            status: closed ? 'stopped' : phase === 'active' ? 'stopped' : phase
          };
        if (pending) return { status: 'busy' };
        if (
          !['key', 'sign', 'encrypt', 'decrypt', 'auth'].includes(kind) ||
          typeof invoke !== 'function'
        )
          return { status: 'invalid' };
        // Mark admission before even the callback's synchronous prefix. The
        // SDK invocation itself may read provider getters and cannot be revoked.
        pending = kind;
        const task = (async (): Promise<ExtensionCallResult<V>> => {
          try {
            const value = await invoke();
            return {
              status: 'settled',
              value,
              current: current(),
              capture: capture()
            };
          } catch {
            if (current()) phase = 'denied';
            return { status: phase === 'active' ? 'denied' : phase };
          } finally {
            pending = null;
          }
        })();
        settlement = task;
        try {
          return await task;
        } finally {
          if (settlement === task) settlement = null;
        }
      }
      actions.set(action, {
        call,
        stop() {
          if (!closed) phase = 'stopped';
        },
        expire() {
          if (!closed && phase === 'active' && pending) phase = 'wait_expired';
        }
      });
      if (!current()) return { status: 'stopped', capture: capture() };
      let value: T | undefined;
      try {
        value = await work(action);
      } catch {
        if (current()) phase = 'denied';
      }
      // Even an unawaited admitted call retains the shared slot. No timeout,
      // disconnect, batch return or rejection can pretend it was cancelled.
      accepting = false;
      const waiting = outstanding();
      if (waiting) await waiting;
      const valid = current();
      return {
        status: valid ? 'completed' : phase === 'active' ? 'stopped' : phase,
        capture: capture(),
        value
      };
    } catch {
      return { status: 'invalid' };
    } finally {
      accepting = false;
      const waiting = outstanding();
      if (waiting) await waiting;
      closed = true;
      occupied = false;
      view = () => ({ state: 'idle' });
    }
  }
  const scheduler = Object.freeze({}) as ExtensionScheduler;
  schedulers.set(scheduler, { run, snapshot: () => view() });
  return scheduler;
}
// Low-level orchestration ports only. No signer, event publisher, permission
// fallback or automatic action is created by the scheduler. Product owners must
// independently validate templates/results before persistence or delivery.
export function runExtensionAction<T>(
  scheduler: ExtensionScheduler,
  capture: ExtensionCapture,
  current: () => boolean,
  work: (action: ExtensionAction) => Promise<T>
): Promise<ExtensionActionResult<T>> {
  return (
    schedulers.get(scheduler)?.run(capture, current, work) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function callExtension<T>(
  action: ExtensionAction,
  kind: ExtensionCallKind,
  invoke: () => Promise<T>
): Promise<ExtensionCallResult<T>> {
  return (
    actions.get(action)?.call(kind, invoke) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function stopExtensionAction(action: ExtensionAction): void {
  actions.get(action)?.stop();
}
// The UI supplies its wait-expiration event. This marks slow/pending, pauses the
// batch, and leaves the real promise reserved; it establishes no cancellation.
export function markExtensionWaitExpired(action: ExtensionAction): void {
  actions.get(action)?.expire();
}
export function extensionSchedulerSnapshot(
  scheduler: ExtensionScheduler
): ExtensionSchedulerSnapshot {
  return schedulers.get(scheduler)?.snapshot() ?? { state: 'unavailable' };
}
