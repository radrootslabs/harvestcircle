import { canonicalPublicKey } from '../contracts/public-key.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  claimPublicOperation,
  publicQuotaOwner,
  type PublicQuotaRepository,
  type PublicQuotaFailure
} from '../persistence/quota.ts';

declare const leaseBrand: unique symbol;
export type PublicEffectLease = Readonly<{ [leaseBrand]: true }>;
export type PublicEffectCapture = Readonly<{
  owner: string;
  session: symbol;
  current(): boolean;
}>;
export type PublicEffectResult<T> =
  | Readonly<{ status: 'completed'; value: T }>
  | Readonly<{ status: 'unknown' | 'retained'; record: PublicRecordHandle }>
  | Readonly<{
      status:
        'invalid' | 'unavailable' | 'busy' | 'stopped' | PublicQuotaFailure;
    }>;
type Authority = Readonly<{
  owner: string;
  id: string;
  recordWire: string;
  session: symbol;
}>;
type State = 'active' | 'unknown' | 'stopped' | 'closed';
type JobResult<T> =
  | Readonly<{ status: 'settled'; value: T; current: boolean }>
  | Readonly<{ status: 'invalid' | 'busy' | 'stopped' | 'unknown' }>;
type Controller = {
  current(authority: Authority): boolean;
  call<T>(authority: Authority, work: () => Promise<T>): Promise<JobResult<T>>;
  mark(): boolean;
  stop(): void;
  snapshot(): Readonly<{
    state: State;
    pending: boolean;
    owner: string;
    id: string;
    hash: string;
    revision: number;
  }>;
};
const leases = new WeakMap<PublicEffectLease, Controller>();

// Lazy browser-only origin/author ownership. It coordinates this client, never
// independent Nostr clients. Only transaction completion acknowledges a plan.
export async function runCapturedPublicEffect<T>(
  repository: PublicQuotaRepository,
  handle: PublicRecordHandle,
  expectedId: unknown,
  input: PublicEffectCapture,
  review: unknown,
  work: (lease: PublicEffectLease) => Promise<T>
): Promise<PublicEffectResult<T>> {
  try {
    const owner = canonicalPublicKey(input?.owner),
      id = canonicalLocalId(expectedId);
    const session = input?.session,
      fresh = input?.current;
    const row =
      owner && id ? publicRecordSnapshot(handle, owner, id) : undefined;
    const recordWire =
      owner && id ? publicRecordWire(handle, owner, id) : undefined;
    if (
      review !== 'reviewed_captured_operation' ||
      !owner ||
      !id ||
      !row ||
      !recordWire ||
      row.family === 'public_draft' ||
      row.revision !== 0 ||
      row.artifact !== null ||
      typeof session !== 'symbol' ||
      typeof fresh !== 'function' ||
      typeof work !== 'function'
    )
      return { status: 'invalid' };
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return { status: 'unavailable' };
    if (publicQuotaOwner(repository) !== owner)
      return { status: 'invalid_scope' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<PublicEffectResult<T>> => {
        if (!lock) return { status: 'busy' };
        if (!fresh()) return { status: 'stopped' };
        const claimed = await claimPublicOperation(repository, id, handle);
        if (!claimed.ok)
          return claimed.reason === 'unknown_completion'
            ? { status: 'unknown', record: handle }
            : { status: claimed.reason };
        const original = claimed.value.record;
        if (claimed.value.state === 'existing') {
          const stored = publicRecordSnapshot(original, owner, id);
          return {
            status:
              stored &&
              stored.family !== 'public_draft' &&
              stored.artifact !== null
                ? 'retained'
                : 'unknown',
            record: original
          };
        }
        let phase: State = 'active',
          closed = false,
          accepting = true,
          started = false;
        let settlement: Promise<void> | null = null;
        function outstanding(): Promise<void> | null {
          return settlement;
        }
        function stop() {
          if (!closed) phase = started ? 'unknown' : 'stopped';
        }
        function current() {
          if (closed || phase !== 'active') return false;
          let valid = false;
          try {
            valid = publicQuotaOwner(repository) === owner && fresh() === true;
          } catch {
            /* finite stale result */
          }
          if (!valid) stop();
          return valid && !closed && phase === 'active';
        }
        function matches(authority: Authority) {
          return (
            authority.owner === owner &&
            authority.id === id &&
            authority.recordWire === recordWire &&
            authority.session === session
          );
        }
        const lease = Object.freeze({}) as PublicEffectLease;
        async function call<V>(
          authority: Authority,
          invoke: () => Promise<V>
        ): Promise<JobResult<V>> {
          if (!matches(authority) || typeof invoke !== 'function')
            return { status: 'invalid' };
          if (!accepting || !current())
            return { status: phase === 'unknown' ? 'unknown' : 'stopped' };
          if (outstanding()) return { status: 'busy' };
          if (started) return { status: 'stopped' };
          // Reserve the whole SDK job before invoking even its synchronous prefix.
          // Callback return cannot release a forgotten/unsettled permission wait.
          let release = () => {};
          const held = new Promise<void>((resolve) => {
            release = resolve;
          });
          settlement = held;
          try {
            const value = await invoke();
            return { status: 'settled', value, current: current() };
          } catch {
            stop();
            return { status: phase === 'unknown' ? 'unknown' : 'stopped' };
          } finally {
            settlement = null;
            release();
          }
        }
        leases.set(lease, {
          current: (authority) => matches(authority) && current(),
          call,
          stop,
          mark() {
            if (started || !outstanding() || !current()) return false;
            started = true;
            return true;
          },
          snapshot: () => ({
            state: phase,
            pending: outstanding() !== null,
            owner,
            id,
            hash: row.capture.hash,
            revision: row.revision
          })
        });
        try {
          if (!current()) return { status: 'stopped' };
          let value: T;
          try {
            value = await work(lease);
          } catch {
            stop();
            return {
              status: 'unknown',
              record: original
            };
          }
          accepting = false;
          const waiting = outstanding();
          if (waiting) {
            // An orphaned job may settle, but cannot acquire more SDK effects
            // or expose an artifact after the operation callback has returned.
            phase = 'unknown';
            await waiting;
          }
          if (!current())
            return phase === 'unknown'
              ? { status: 'unknown', record: original }
              : { status: 'stopped' };
          return { status: 'completed', value };
        } finally {
          accepting = false;
          const waiting = outstanding();
          if (waiting) await waiting;
          closed = true;
          if (phase === 'active') phase = 'closed';
        }
      }
    );
  } catch {
    return { status: 'unavailable' };
  }
}
// These internal mechanical ports expose neither a signer nor a raw template.
// The adapter supplies its actual private session and genuine approval wire.
export function publicEffectLeaseCurrent(
  lease: PublicEffectLease,
  authority: Authority
): boolean {
  return leases.get(lease)?.current(authority) ?? false;
}
export function callOwnedExtension<T>(
  lease: PublicEffectLease,
  authority: Authority,
  work: () => Promise<T>
): Promise<JobResult<T>> {
  return (
    leases.get(lease)?.call(authority, work) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function markOwnedSignaturePending(lease: PublicEffectLease): boolean {
  return leases.get(lease)?.mark() ?? false;
}
export function stopPublicEffect(lease: PublicEffectLease): void {
  leases.get(lease)?.stop();
}
export function publicEffectSnapshot(lease: PublicEffectLease) {
  return leases.get(lease)?.snapshot();
}
