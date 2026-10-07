import { PUBLIC_REQUEST_BUDGETS } from '../config/budgets.ts';
import type { RelayPolicy } from '../config/relays.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { requireNip50Source } from './search-sources.ts';
import {
  createPublicIngress,
  publicIngressStats,
  type PublicIngress
} from './ingress.ts';
import {
  createObservationJournal,
  closeObservationJournal,
  publicObservations,
  type ObservationJournal
} from '../catalog/observations.ts';
import {
  createPublicRequestResult,
  createInboxRequestResult,
  handlePublicRequestMessage,
  publicRequestSnapshot,
  disposePublicRequestResult,
  type PublicRequestResult
} from './request-result.ts';
import type { PublicPoolMessage } from './exports.ts';
import type { VerifiedEnvelope } from './verified-envelope.ts';
declare const schedulerBrand: unique symbol;
declare const runBrand: unique symbol;
declare const requestBrand: unique symbol;
export type PublicScheduler = Readonly<{ readonly [schedulerBrand]: true }>;
export type PublicRun = Readonly<{ readonly [runBrand]: true }>;
export type PublicRequest = Readonly<{ readonly [requestBrand]: true }>;
export type RequestKind = 'search' | 'head' | 'deletion' | 'profile' | 'inbox';
export type RequestState =
  'active' | 'eose' | 'partial' | 'cancelled' | 'deadline' | 'limit' | 'error';
export interface RequestClock {
  readonly now: () => number;
  readonly schedule: (callback: () => void, delay: number) => () => void;
}
const browserClock: RequestClock = {
  now: () => performance.now(),
  schedule(callback, delay) {
    const timer = setTimeout(() => callback(), delay);
    return () => clearTimeout(timer);
  }
};
interface SchedulerOwner {
  readonly now: () => number;
  readonly sampledNow: () => number;
  readonly schedule: RequestClock['schedule'];
  readonly available: () => void;
  readonly reserve: (token: PublicRequest, close: () => boolean) => void;
  readonly release: (token: PublicRequest) => void;
  readonly register: (token: PublicRun, dispose: () => boolean) => void;
  readonly unregister: (token: PublicRun) => void;
  readonly snapshot: () => Readonly<{
    activeRequests: number;
    closed: boolean;
  }>;
  readonly close: () => void;
}
interface RunOwner {
  readonly scheduler: SchedulerOwner;
  readonly ingress: PublicIngress;
  readonly journal: ObservationJournal;
  readonly deadline: number;
  readonly result: (request: PublicRequest) => PublicRequestResult;
  readonly open: (
    kind: RequestKind,
    open: (next: (message: PublicPoolMessage) => void) => () => void,
    onVerified: (event: VerifiedEnvelope, inboxSource?: string) => void,
    sampleSource?: string,
    inboxAuthor?: string
  ) => PublicRequest;
  readonly active: () => boolean;
  readonly currentAfterSample: () => boolean;
  readonly snapshot: () => Readonly<{
    deadline: number;
    active: boolean;
    activeRequests: number;
    ingress: ReturnType<typeof publicIngressStats>;
  }>;
  readonly cancel: () => boolean;
  readonly dispose: () => boolean;
}
interface RequestOwner {
  readonly run: PublicRun;
  readonly result: PublicRequestResult;
  readonly snapshot: () => Readonly<{
    kind: RequestKind;
    deadline: number;
    state: RequestState;
    inboxSettled?: boolean;
    result: ReturnType<typeof publicRequestSnapshot>;
  }>;
  readonly close: () => boolean;
}
const schedulers = new WeakMap<PublicScheduler, SchedulerOwner>();
const runs = new WeakMap<PublicRun, RunOwner>();
const requests = new WeakMap<PublicRequest, RequestOwner>();
function schedulerOf(token: PublicScheduler): SchedulerOwner {
  const owner = schedulers.get(token);
  if (!owner) throw new Error('public_scheduler_invalid');
  return owner;
}
function runOf(token: PublicRun): RunOwner {
  const owner = runs.get(token);
  if (!owner) throw new Error('public_run_invalid');
  return owner;
}
function requestOf(token: PublicRequest): RequestOwner {
  const owner = requests.get(token);
  if (!owner) throw new Error('public_request_invalid');
  return owner;
}
export function createPublicScheduler(
  clock: RequestClock = browserClock
): PublicScheduler {
  const active = new Map<PublicRequest, () => boolean>(),
    registered = new Map<PublicRun, () => boolean>();
  let closed = false,
    last = 0;
  const owner: SchedulerOwner = {
    now() {
      const value = clock.now();
      if (!Number.isFinite(value) || value < 0)
        throw new Error('public_clock_invalid');
      last = Math.max(last, value);
      return last;
    },
    sampledNow: () => last,
    schedule: (callback, delay) => clock.schedule(callback, delay),
    available() {
      if (closed) throw new Error('public_scheduler_closed');
    },
    reserve(token, close) {
      owner.available();
      if (active.size >= PUBLIC_REQUEST_BUDGETS.parallelScopes)
        throw new Error('public_request_concurrency_limit');
      active.set(token, close);
    },
    release(token) {
      active.delete(token);
    },
    register(token, dispose) {
      owner.available();
      registered.set(token, dispose);
    },
    unregister(token) {
      registered.delete(token);
    },
    snapshot() {
      return { activeRequests: active.size, closed: closed };
    },
    close() {
      closed = true;
      let failed = false;
      for (const dispose of registered.values()) if (!dispose()) failed = true;
      for (const close of active.values()) if (!close()) failed = true;
      if (failed) throw new Error('public_scheduler_close_failed');
    }
  };
  const token = Object.freeze({}) as PublicScheduler;
  schedulers.set(token, owner);
  return token;
}
// One immutable run owns the aggregate event/byte budget and observation journal.
// All request kinds use the same admission owner; no per-auxiliary budget reset.
export function createPublicRun(
  scheduler: PublicScheduler,
  policy: RelayPolicy
): PublicRun {
  const shared = schedulerOf(scheduler);
  shared.available();
  const deadline = shared.now() + PUBLIC_REQUEST_BUDGETS.runMilliseconds;
  const ingress = createPublicIngress(),
    journal = createObservationJournal(policy),
    active = new Map<PublicRequest, () => boolean>();
  let cancelled = false,
    disposed = false;
  const token = Object.freeze({}) as PublicRun;
  function cancel(): boolean {
    cancelled = true;
    let failed = false;
    for (const close of active.values()) if (!close()) failed = true;
    return !failed;
  }
  function dispose(): boolean {
    const settled = cancel();
    disposed = true;
    closeObservationJournal(journal);
    if (settled) shared.unregister(token);
    return settled;
  }
  const owner: RunOwner = {
    scheduler: shared,
    ingress,
    journal,
    deadline,
    result(request) {
      const row = requestOf(request);
      if (row.run !== token) throw new Error('public_request_run_changed');
      return row.result;
    },
    currentAfterSample: () =>
      !cancelled &&
      !disposed &&
      !shared.snapshot().closed &&
      shared.sampledNow() < deadline &&
      !publicIngressStats(ingress).stopped,
    active: () =>
      !cancelled &&
      !disposed &&
      !shared.snapshot().closed &&
      shared.now() < deadline &&
      !publicIngressStats(ingress).stopped,
    snapshot: () => ({
      deadline: deadline,
      active: owner.active(),
      activeRequests: active.size,
      ingress: publicIngressStats(ingress)
    }),
    cancel,
    dispose,
    open(kind, open, onVerified, sampleSource, inboxAuthor) {
      if (kind === 'inbox' && !canonicalPublicKey(inboxAuthor))
        throw new Error('inbox_author_invalid');
      if (kind !== 'inbox' && inboxAuthor !== undefined)
        throw new Error('inbox_request_kind_invalid');
      if (sampleSource !== undefined) {
        if (kind !== 'search') throw new Error('nip50_request_kind');
        requireNip50Source(policy, sampleSource);
      }
      shared.available();
      if (!owner.active()) throw new Error('public_run_inactive');
      if (!['search', 'head', 'deletion', 'profile', 'inbox'].includes(kind))
        throw new Error('public_request_kind_invalid');
      const request = Object.freeze({}) as PublicRequest;
      const expires = Math.min(
        deadline,
        shared.now() + PUBLIC_REQUEST_BUDGETS.requestMilliseconds
      );
      let state: RequestState = 'active',
        opening = true,
        cleanupComplete = false,
        timerCancelled = false,
        stopCompleted = false,
        stop = () => {},
        cancelTimer = () => {};
      // Reserve before acquiring SDK subscriptions or scheduling their callbacks.
      shared.reserve(request, () => finish('cancelled'));
      let result: PublicRequestResult;
      try {
        result =
          inboxAuthor === undefined
            ? createPublicRequestResult(policy, ingress, journal, sampleSource)
            : createInboxRequestResult(policy, ingress, journal, inboxAuthor);
      } catch {
        shared.release(request);
        throw new Error('public_request_prepare_failed');
      }
      function finish(next: Exclude<RequestState, 'active'>): boolean {
        if (cleanupComplete) return true;
        if (state === 'active') state = next;
        disposePublicRequestResult(result);
        let failed = false;
        if (!timerCancelled) {
          try {
            cancelTimer();
            timerCancelled = true;
          } catch {
            failed = true;
          }
        }
        if (!opening) {
          if (!stopCompleted) {
            try {
              stop();
              stopCompleted = true;
            } catch {
              failed = true;
            }
          }
          if (!failed && timerCancelled && stopCompleted) {
            cleanupComplete = true;
            active.delete(request);
            shared.release(request);
          }
        }
        return !failed;
      }
      active.set(request, () => finish('cancelled'));
      requests.set(request, {
        run: token,
        result,
        snapshot: () => ({
          kind: kind,
          deadline: expires,
          state: state,
          ...(kind === 'inbox' ? { inboxSettled: cleanupComplete } : {}),
          result: publicRequestSnapshot(result)
        }),
        close: () => finish('cancelled')
      });
      function next(message: PublicPoolMessage): void {
        if (state !== 'active') return;
        try {
          if (!owner.active()) {
            finish(shared.now() >= deadline ? 'deadline' : 'cancelled');
            return;
          }
          if (shared.now() >= expires) {
            finish('deadline');
            return;
          }
        } catch {
          finish('error');
          return;
        }
        const outcome = handlePublicRequestMessage(result, message);
        let callbackFailed = false;
        if (outcome.status === 'accepted' || outcome.status === 'duplicate') {
          try {
            if (outcome.inboxSource === undefined) onVerified(outcome.value);
            else onVerified(outcome.value, outcome.inboxSource);
          } catch {
            finish('error');
            callbackFailed = true;
          }
        }
        if (publicIngressStats(ingress).stopped || outcome.status === 'limit') {
          finish('limit');
          cancel();
          return;
        }
        if (callbackFailed) return;
        const sources = publicRequestSnapshot(result).sources;
        if (
          sources.length > 0 &&
          sources.every((row) => row.state !== 'pending')
        )
          finish(
            sources.every((row) => row.state === 'eose' && row.rejected === 0)
              ? 'eose'
              : 'partial'
          );
      }
      function expire(): void {
        if (state !== 'active') return;
        try {
          const remaining = expires - shared.now();
          if (remaining > 0) {
            cancelTimer = shared.schedule(() => expire(), remaining);
            timerCancelled = false;
            return;
          }
          finish('deadline');
        } catch {
          finish('error');
        }
      }
      try {
        cancelTimer = shared.schedule(
          () => expire(),
          Math.max(0, expires - shared.now())
        );
        // A trusted test scheduler may invoke synchronously before returning its
        // control. Ensure the returned control is still settled before release.
        timerCancelled = false;
        if (state === 'active') stop = open(next);
        opening = false;
        if (state !== 'active') finish(state);
      } catch {
        opening = false;
        finish('error');
        throw new Error('public_request_open_failed');
      }
      return request;
    }
  };
  runs.set(token, owner);
  shared.register(token, dispose);
  return token;
}
export function openPublicRequest(
  run: PublicRun,
  kind: RequestKind,
  open: (next: (message: PublicPoolMessage) => void) => () => void,
  onVerified: (event: VerifiedEnvelope) => void,
  sampleSource?: string
): PublicRequest {
  return runOf(run).open(kind, open, onVerified, sampleSource);
}
export function closePublicRequest(request: PublicRequest): void {
  if (!requestOf(request).close())
    throw new Error('public_request_close_failed');
}
export function openInboxRequest(
  run: PublicRun,
  author: unknown,
  open: (next: (message: PublicPoolMessage) => void) => () => void,
  onVerified: (event: VerifiedEnvelope, inboxSource?: string) => void
): PublicRequest {
  const key = canonicalPublicKey(author);
  if (!key) throw new Error('inbox_author_invalid');
  return runOf(run).open('inbox', open, onVerified, undefined, key);
}
export function cancelPublicRun(run: PublicRun): void {
  if (!runOf(run).cancel()) throw new Error('public_run_close_failed');
}
export function disposePublicRun(run: PublicRun): void {
  if (!runOf(run).dispose()) throw new Error('public_run_close_failed');
}
export function closePublicScheduler(scheduler: PublicScheduler): void {
  schedulerOf(scheduler).close();
}
export function publicSchedulerSnapshot(scheduler: PublicScheduler) {
  return schedulerOf(scheduler).snapshot();
}
// Final fence after callers sample their relevant clocks. No external clock
// call or fresh wall-time promise: cached monotonic sample plus actual owners.
export function publicRunCurrentAfterSample(run: PublicRun): boolean {
  return runOf(run).currentAfterSample();
}
export function publicRunSnapshot(run: PublicRun) {
  return runOf(run).snapshot();
}
export function publicRequestScopeSnapshot(request: PublicRequest) {
  return requestOf(request).snapshot();
}
export function publicRunObservations(run: PublicRun, request: PublicRequest) {
  const owner = runOf(run);
  const result = owner.result(request);
  return publicObservations(
    owner.journal,
    publicRequestSnapshot(result).context
  );
}
