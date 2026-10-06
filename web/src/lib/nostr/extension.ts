import {
  ExtensionSigner,
  ExtensionMissingError
} from 'applesauce-signers/signers/extension-signer';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';

declare const adapterBrand: unique symbol;
export type ExtensionAdapter = Readonly<{ [adapterBrand]: true }>;
export type ExtensionGuestReason =
  | 'disconnected'
  | 'missing'
  | 'refused'
  | 'invalid_key'
  | 'changed_key'
  | 'unavailable';
export type ExtensionSnapshot =
  | Readonly<{ state: 'guest'; reason: ExtensionGuestReason }>
  | Readonly<{ state: 'pending'; action: 'connect' | 'recheck' | 'probe' }>
  | Readonly<{
      state: 'connected';
      publicKey: string;
      signingCandidate: boolean;
      messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable';
    }>;

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
async function freshKey(current: () => boolean): Promise<string | undefined> {
  if (!current()) return undefined;
  const signer = new ExtensionSigner();
  // This SDK invocation admits one NIP-07 call. Its internal getters and
  // permission wait cannot universally be interrupted; callers fence the result.
  if (!current()) return undefined;
  return canonicalPublicKey(await signer.getPublicKey());
}
// Lexically owned per-adapter state; no mutable opaque receiver, browser handle
// or imported signer escapes through the WeakMap/controller boundary.
export function createExtensionAdapter(): ExtensionAdapter {
  let key: string | null = null,
    signingCandidate = false;
  let messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable' =
    'not_probed';
  let reason: ExtensionGuestReason = 'disconnected';
  let pending: 'connect' | 'recheck' | 'probe' | null = null;
  let generation = {},
    cancelled = false;
  function snapshot(): ExtensionSnapshot {
    if (pending && !cancelled) return { state: 'pending', action: pending };
    return key
      ? { state: 'connected', publicKey: key, signingCandidate, messaging }
      : { state: 'guest', reason };
  }
  function guest(nextReason: ExtensionGuestReason) {
    key = null;
    signingCandidate = false;
    messaging = 'not_probed';
    reason = nextReason;
  }
  function disconnect() {
    generation = {};
    cancelled = true;
    guest(
      'disconnected'
    ); /* Retain pending slot until actual promise settlement. */
  }
  async function run(
    action: 'connect' | 'recheck' | 'probe',
    work: (current: () => boolean) => Promise<void>
  ): Promise<ExtensionSnapshot> {
    if (pending) return snapshot();
    const original = generation;
    pending = action;
    cancelled = false;
    const current = () => generation === original && !cancelled;
    try {
      await work(current);
    } catch (error) {
      if (current())
        guest(error instanceof ExtensionMissingError ? 'missing' : 'refused');
    } finally {
      pending = null;
    }
    return snapshot();
  }
  function connect(): Promise<ExtensionSnapshot> {
    return run('connect', async (current) => {
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
      const owner = await freshKey(current);
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
    return run('recheck', async (current) => {
      const owner = await freshKey(current);
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
    return run('probe', async (current) => {
      const owner = key;
      if (!owner) return;
      const checked = await freshKey(current);
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
        encrypted = await encrypt(owner, text);
        if (!current()) return;
        if (
          typeof encrypted !== 'string' ||
          !boundedUtf8(encrypted, PUBLIC_INGRESS_BUDGETS.eventBytes)
        ) {
          messaging = 'refused';
          return;
        }
        if (!current()) return;
        decrypted = await decrypt(owner, encrypted);
      } catch {
        if (current()) messaging = 'refused';
        return;
      }
      if (!current()) return;
      if (decrypted !== text) {
        messaging = 'refused';
        return;
      }
      const after = await freshKey(current);
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
  const adapter = Object.freeze({}) as ExtensionAdapter;
  adapters.set(adapter, { snapshot, disconnect, connect, recheck, probe });
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
