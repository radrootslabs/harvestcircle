import {
  ExtensionSigner,
  ExtensionMissingError
} from 'applesauce-signers/signers/extension-signer';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';
import {
  approvedSigningIdentity,
  disposableApprovedTemplate,
  bindApprovedResponse,
  type ApprovedPublicSigning
} from './approved-signing.ts';
import type { CapturedArtifact } from '../persistence/artifact-records.ts';
import {
  bindLatePublicResponse,
  type LatePublicArtifact
} from '../runtime/late-results.ts';
import {
  callOwnedExtension,
  publicEffectLeaseCurrent,
  publicEffectSnapshot,
  markOwnedSignaturePending,
  type PublicEffectLease,
  type PublicEffectCapture
} from '../runtime/effect-ownership.ts';

export type ApprovedSignResult =
  | Readonly<{ status: 'signed'; artifact: CapturedArtifact }>
  | Readonly<{ status: 'unknown'; late?: LatePublicArtifact }>
  | Readonly<{
      status:
        | 'invalid_approval'
        | 'invalid_ownership'
        | 'unavailable'
        | 'busy'
        | 'stale'
        | 'refused'
        | 'mismatch';
    }>;

import {
  browserExtensionScheduler,
  runExtensionAction,
  callExtension,
  markExtensionWaitExpired,
  type ExtensionAction,
  type ExtensionCallKind
} from './extension-scheduler.ts';

declare const adapterBrand: unique symbol;
export type ExtensionAdapter = Readonly<{ [adapterBrand]: true }>;
export type ExtensionGuestReason =
  | 'disconnected'
  | 'missing'
  | 'refused'
  | 'invalid_key'
  | 'changed_key'
  | 'unavailable';
export type ExtensionSnapshot = (
  | Readonly<{ state: 'guest'; reason: ExtensionGuestReason }>
  | Readonly<{ state: 'pending'; action: 'connect' | 'recheck' | 'probe' }>
  | Readonly<{
      state: 'connected';
      publicKey: string;
      signingCandidate: boolean;
      messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable';
    }>
) &
  Readonly<{ admission?: 'busy' }>;

declare global {
  interface Window {
    nostr?: unknown;
  }
}
type Controller = {
  snapshot(): ExtensionSnapshot;
  disconnect(): void;
  connect(): Promise<ExtensionSnapshot>;
  recheck(): Promise<ExtensionSnapshot>;
  probe(review: unknown): Promise<ExtensionSnapshot>;
  capture(): PublicEffectCapture | undefined;
  captureMessaging(): PublicEffectCapture | undefined;
  subscribeInvalidation(listener: () => void): () => void;
  expire(approval: ApprovedPublicSigning, lease: PublicEffectLease): void;
  sign(
    approval: ApprovedPublicSigning,
    lease: PublicEffectLease
  ): Promise<ApprovedSignResult>;
};
const adapters = new WeakMap<ExtensionAdapter, Controller>();
function present(): Readonly<{ signingCandidate: boolean }> | undefined {
  if (typeof window === 'undefined') return undefined;
  const provider: unknown = window.nostr;
  if (
    (typeof provider !== 'object' && typeof provider !== 'function') ||
    provider === null
  )
    return undefined;
  const value = provider as { getPublicKey?: unknown; signEvent?: unknown };
  return typeof value.getPublicKey === 'function'
    ? { signingCandidate: typeof value.signEvent === 'function' }
    : undefined;
}
async function invokeExtension<T>(
  action: ExtensionAction | undefined,
  kind: ExtensionCallKind,
  work: () => Promise<T>
): Promise<T> {
  if (!action) throw new Error('extension_unavailable');
  let missing = false;
  const result = await callExtension(action, kind, async () => {
    try {
      return await work();
    } catch (error) {
      missing = error instanceof ExtensionMissingError;
      throw error;
    }
  });
  if (result.status === 'settled' && result.current) return result.value;
  if (missing) throw new ExtensionMissingError('extension_missing');
  throw new Error('extension_' + result.status);
}
async function freshKey(
  current: () => boolean,
  action: ExtensionAction | undefined
): Promise<string | undefined> {
  if (!current()) return undefined;
  const signer = new ExtensionSigner();
  // This SDK invocation admits one NIP-07 call. Its internal getters and
  // permission wait cannot universally be interrupted; callers fence the result.
  if (!current()) return undefined;
  return canonicalPublicKey(
    await invokeExtension(action, 'key', () => signer.getPublicKey())
  );
}
// Lexically owned per-adapter state; no mutable opaque receiver, browser handle
// or imported signer escapes through the WeakMap/controller boundary.
export function createExtensionAdapter(): ExtensionAdapter {
  const invalidations = new Map<symbol, () => void>();
  function notifyInvalidation() {
    for (const listener of invalidations.values()) {
      try {
        listener();
      } catch {
        /* Cleanup failure stays owned by its scope. */
      }
    }
  }
  let key: string | null = null,
    signingCandidate = false;
  let messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable' =
    'not_probed';
  let reason: ExtensionGuestReason = 'disconnected';
  let pending: 'connect' | 'recheck' | 'probe' | null = null;
  let signing = false;
  let activeSigning: Readonly<{
    approval: ApprovedPublicSigning;
    lease: PublicEffectLease;
    action: ExtensionAction;
  }> | null = null;
  let generation = Symbol(),
    cancelled = false;
  function snapshot(): ExtensionSnapshot {
    if (pending && !cancelled) return { state: 'pending', action: pending };
    return key
      ? { state: 'connected', publicKey: key, signingCandidate, messaging }
      : { state: 'guest', reason };
  }
  function guest(nextReason: ExtensionGuestReason) {
    // An observed account loss cannot later revive an old capture, including
    // a reconnect to the same key. Rotate before clearing the observed owner.
    if (key !== null) generation = Symbol();
    key = null;
    signingCandidate = false;
    messaging = 'not_probed';
    reason = nextReason;
    notifyInvalidation();
  }
  function disconnect() {
    generation = Symbol();
    cancelled = true;
    guest(
      'disconnected'
    ); /* Retain pending slot until actual promise settlement. */
  }
  async function run(
    action: 'connect' | 'recheck' | 'probe',
    work: (
      current: () => boolean,
      action: ExtensionAction | undefined
    ) => Promise<void>
  ): Promise<ExtensionSnapshot> {
    if (pending || signing) return { ...snapshot(), admission: 'busy' };
    let busy = false;
    const original = generation;
    pending = action;
    cancelled = false;
    const current = () => generation === original && !cancelled;
    try {
      const scheduler = browserExtensionScheduler();
      if (scheduler) {
        // One shared page slot for all sessions and effect kinds. Busy performs
        // no provider work and never schedules an automatic retry.
        const admitted = await runExtensionAction(
          scheduler,
          { owner: key, session: original, operation: Symbol() },
          current,
          async (lease) => {
            try {
              await work(current, lease);
            } catch (error) {
              if (current())
                guest(
                  error instanceof ExtensionMissingError ? 'missing' : 'refused'
                );
              throw error;
            }
          }
        );
        busy = admitted.status === 'busy';
      } else await work(current, undefined);
    } catch (error) {
      if (current())
        guest(error instanceof ExtensionMissingError ? 'missing' : 'refused');
    } finally {
      pending = null;
      notifyInvalidation();
    }
    return busy ? { ...snapshot(), admission: 'busy' } : snapshot();
  }
  function connect(): Promise<ExtensionSnapshot> {
    return run('connect', async (current, action) => {
      if (typeof window === 'undefined') {
        guest('unavailable');
        return;
      }
      const candidate = present();
      if (!current()) return;
      if (!candidate) {
        guest('missing');
        return;
      }
      const owner = await freshKey(current, action);
      if (!current()) return;
      if (!owner) {
        guest('invalid_key');
        return;
      }
      if (key && owner !== key) {
        guest('changed_key');
        return;
      }
      const resolvedCandidate = present();
      if (!current()) return;
      if (!resolvedCandidate) {
        guest('missing');
        return;
      }
      key = owner;
      signingCandidate = resolvedCandidate.signingCandidate;
      messaging = 'not_probed';
    });
  }
  function recheck(): Promise<ExtensionSnapshot> {
    if (!key) return Promise.resolve(snapshot());
    return run('recheck', async (current, action) => {
      const owner = await freshKey(current, action);
      if (!current()) return;
      if (!owner) {
        guest('invalid_key');
        return;
      }
      if (owner !== key) {
        guest('changed_key');
        return;
      }
      const candidate = present();
      if (!current()) return;
      if (!candidate) {
        guest('missing');
        return;
      }
      signingCandidate = candidate.signingCandidate;
      if (messaging === 'capable') {
        const cipher = new ExtensionSigner().nip44;
        if (!current()) return;
        if (!cipher) {
          messaging = 'unsupported';
          return;
        }
        const encrypt = cipher.encrypt;
        if (!current()) return;
        const decrypt = cipher.decrypt;
        if (!current()) return;
        if (typeof encrypt !== 'function' || typeof decrypt !== 'function')
          messaging = 'unsupported';
      }
    });
  }
  function probe(review: unknown): Promise<ExtensionSnapshot> {
    if (!key || review !== 'reviewed_self_copy')
      return Promise.resolve(snapshot());
    return run('probe', async (current, action) => {
      const owner = key;
      if (!owner) return;
      const checked = await freshKey(current, action);
      if (!current()) return;
      if (checked !== owner) {
        guest(checked ? 'changed_key' : 'invalid_key');
        return;
      }
      const signer = new ExtensionSigner(),
        cipher = signer.nip44;
      if (!current()) return;
      if (!cipher) {
        messaging = 'unsupported';
        return;
      }
      const encryptMethod = cipher.encrypt;
      if (!current()) return;
      const decryptMethod = cipher.decrypt;
      if (!current()) return;
      if (
        typeof encryptMethod !== 'function' ||
        typeof decryptMethod !== 'function'
      ) {
        messaging = 'unsupported';
        return;
      }
      const encrypt = encryptMethod.bind(cipher);
      if (!current()) return;
      const decrypt = decryptMethod.bind(cipher);
      if (!current()) return;
      const text = 'HarvestCircle self-copy check:' + crypto.randomUUID();
      let encrypted: unknown, decrypted: unknown;
      try {
        if (!current()) return;
        encrypted = await invokeExtension(action, 'encrypt', () =>
          encrypt(owner, text)
        );
        if (!current()) return;
        if (
          typeof encrypted !== 'string' ||
          !boundedUtf8(encrypted, PUBLIC_INGRESS_BUDGETS.eventBytes)
        ) {
          messaging = 'refused';
          return;
        }
        if (!current()) return;
        decrypted = await invokeExtension(action, 'decrypt', () =>
          decrypt(owner, encrypted as string)
        );
      } catch {
        if (current()) messaging = 'refused';
        return;
      }
      if (!current()) return;
      if (decrypted !== text) {
        messaging = 'refused';
        return;
      }
      const after = await freshKey(current, action);
      if (!current()) return;
      if (after !== owner) {
        guest(after ? 'changed_key' : 'invalid_key');
        return;
      }
      const candidate = present();
      if (!current()) return;
      if (!candidate) {
        guest('missing');
        return;
      }
      signingCandidate = candidate.signingCandidate;
      const finalCipher = new ExtensionSigner().nip44;
      if (!current()) return;
      if (!finalCipher) {
        messaging = 'unsupported';
        return;
      }
      const finalEncrypt = finalCipher.encrypt;
      if (!current()) return;
      const finalDecrypt = finalCipher.decrypt;
      if (!current()) return;
      if (
        typeof finalEncrypt !== 'function' ||
        typeof finalDecrypt !== 'function'
      ) {
        messaging = 'unsupported';
        return;
      }
      messaging = 'capable';
    });
  }
  async function sign(
    approval: ApprovedPublicSigning,
    lease: PublicEffectLease
  ): Promise<ApprovedSignResult> {
    const captured = approvedSigningIdentity(approval);
    if (!captured) return { status: 'invalid_approval' };
    if (pending || signing) return { status: 'busy' };
    if (
      !key ||
      key !== captured.owner ||
      !signingCandidate ||
      typeof window === 'undefined'
    )
      return { status: 'unavailable' };
    const original = generation,
      owner = key;
    const authority = {
      owner,
      id: captured.id,
      recordWire: captured.recordWire,
      session: original
    };
    if (!publicEffectLeaseCurrent(lease, authority))
      return { status: 'invalid_ownership' };
    const current = () =>
      generation === original &&
      !cancelled &&
      key === owner &&
      publicEffectLeaseCurrent(lease, authority);
    signing = true;
    let late: LatePublicArtifact | undefined;
    function lost(): ApprovedSignResult {
      return publicEffectSnapshot(lease)?.state === 'unknown'
        ? { status: 'unknown', ...(late ? { late } : {}) }
        : { status: 'stale' };
    }
    try {
      const owned = await callOwnedExtension(
        lease,
        authority,
        async (): Promise<ApprovedSignResult> => {
          const scheduler = browserExtensionScheduler();
          if (!scheduler) return { status: 'unavailable' };
          const admitted = await runExtensionAction(
            scheduler,
            { owner, session: original, operation: Symbol() },
            current,
            async (action): Promise<ApprovedSignResult> => {
              activeSigning = { approval, lease, action };
              const before = await freshKey(current, action);
              if (!current()) return { status: 'stale' };
              if (before !== owner) {
                guest(before ? 'changed_key' : 'invalid_key');
                return { status: 'stale' };
              }
              const disposable = disposableApprovedTemplate(approval);
              if (!disposable) return { status: 'invalid_approval' };
              const signer = new ExtensionSigner();
              if (!current() || !markOwnedSignaturePending(lease))
                return { status: 'stale' };
              const response: unknown = await invokeExtension(
                action,
                'sign',
                async () => {
                  const raw: unknown = await signer.signEvent(disposable);
                  // Retain only bounded independently verified original public
                  // fields inside the actual admitted SDK callback, even after
                  // its owner/view/wait generation was invalidated. No new call.
                  late = bindLatePublicResponse(approval, raw, original);
                  return raw;
                }
              );
              if (!current()) return { status: 'stale' };
              const artifact = bindApprovedResponse(approval, response);
              if (!artifact) return { status: 'mismatch' };
              const after = await freshKey(current, action);
              if (!current()) return { status: 'stale' };
              if (after !== owner) {
                guest(after ? 'changed_key' : 'invalid_key');
                return { status: 'stale' };
              }
              return { status: 'signed', artifact };
            }
          );
          if (admitted.status === 'busy') return { status: 'busy' };
          if (!current()) return { status: 'stale' };
          return 'value' in admitted && admitted.value
            ? admitted.value
            : { status: admitted.status === 'denied' ? 'refused' : 'stale' };
        }
      );
      if (owned.status === 'settled')
        return owned.current ? owned.value : lost();
      if (owned.status === 'unknown')
        return { status: 'unknown', ...(late ? { late } : {}) };
      return {
        status:
          owned.status === 'invalid'
            ? 'invalid_ownership'
            : owned.status === 'stopped'
              ? 'stale'
              : owned.status
      };
    } catch {
      return current() ? { status: 'refused' } : lost();
    } finally {
      activeSigning = null;
      signing = false;
    }
  }
  const adapter = Object.freeze({}) as ExtensionAdapter;
  adapters.set(adapter, {
    snapshot,
    disconnect,
    connect,
    recheck,
    probe,
    capture() {
      if (!key || cancelled) return undefined;
      const owner = key,
        original = generation;
      return {
        owner,
        session: original,
        current: () => generation === original && !cancelled && key === owner
      };
    },
    captureMessaging() {
      if (
        !key ||
        cancelled ||
        !signingCandidate ||
        messaging !== 'capable' ||
        pending ||
        signing
      )
        return undefined;
      const owner = key,
        original = generation;
      return {
        owner,
        session: original,
        current: () =>
          generation === original &&
          !cancelled &&
          key === owner &&
          signingCandidate &&
          messaging === 'capable'
      };
    },
    subscribeInvalidation(listener) {
      const id = Symbol();
      invalidations.set(id, listener);
      return () => {
        invalidations.delete(id);
      };
    },
    sign,
    expire(approval, lease) {
      if (activeSigning?.approval === approval && activeSigning.lease === lease)
        markExtensionWaitExpired(activeSigning.action);
    }
  });
  return adapter;
}
export function extensionSnapshot(
  adapter: ExtensionAdapter
): ExtensionSnapshot {
  return (
    adapters.get(adapter)?.snapshot() ?? {
      state: 'guest',
      reason: 'unavailable'
    }
  );
}
export function disconnectExtensionAdapter(adapter: ExtensionAdapter): void {
  adapters.get(adapter)?.disconnect();
}
// Explicit action methods only; constructor/import never request an extension.
export function connectExtensionAdapter(
  adapter: ExtensionAdapter
): Promise<ExtensionSnapshot> {
  return (
    adapters.get(adapter)?.connect() ??
    Promise.resolve({ state: 'guest', reason: 'unavailable' })
  );
}
export function recheckExtensionAdapter(
  adapter: ExtensionAdapter
): Promise<ExtensionSnapshot> {
  return (
    adapters.get(adapter)?.recheck() ??
    Promise.resolve({ state: 'guest', reason: 'unavailable' })
  );
}
// Only reviewed disposable self-copy text. No caller body, NIP04 fallback, signer
// object, setup, EVENT or AUTH is exposed. Signing shapes are candidates only.
export function probeExtensionAdapter(
  adapter: ExtensionAdapter,
  review: unknown
): Promise<ExtensionSnapshot> {
  return (
    adapters.get(adapter)?.probe(review) ??
    Promise.resolve({ state: 'guest', reason: 'unavailable' })
  );
}
// Internal operation capability only. UI identity sessions do not expose this
// adapter or an unrestricted raw-event signing/publishing function.
export function signApprovedExtensionAdapter(
  adapter: ExtensionAdapter,
  approval: ApprovedPublicSigning,
  lease: PublicEffectLease
): Promise<ApprovedSignResult> {
  return (
    adapters.get(adapter)?.sign(approval, lease) ??
    Promise.resolve({ status: 'unavailable' })
  );
}
export function extensionOwnershipCapture(
  adapter: ExtensionAdapter
): PublicEffectCapture | undefined {
  return adapters.get(adapter)?.capture();
}
export function extensionMessagingOwnershipCapture(
  adapter: ExtensionAdapter
): PublicEffectCapture | undefined {
  return adapters.get(adapter)?.captureMessaging();
}
export function subscribeExtensionInvalidation(
  adapter: ExtensionAdapter,
  listener: () => void
): () => void {
  return adapters.get(adapter)?.subscribeInvalidation(listener) ?? (() => {});
}
// Internal exact original operation port. Expiry pauses the real scheduler;
// neither this method nor a UI timeout pretends to cancel the SDK promise.
export function markApprovedSigningWaitExpired(
  adapter: ExtensionAdapter,
  approval: ApprovedPublicSigning,
  lease: PublicEffectLease
): void {
  adapters.get(adapter)?.expire(approval, lease);
}
