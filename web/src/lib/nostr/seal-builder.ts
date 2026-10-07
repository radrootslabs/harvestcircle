import { ExtensionSigner } from 'applesauce-signers';
import type { UnsignedEvent } from 'applesauce-core/helpers';
import {
  reservedSendSnapshot,
  reservedSendRumorWire,
  type ReservedSendIdentity
} from '../messaging/send-identity.ts';
import {
  identityMessagingOwnership,
  disconnectIdentity,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  browserExtensionScheduler,
  runExtensionAction,
  callExtension,
  stopExtensionAction,
  markExtensionWaitExpired,
  type ExtensionAction
} from './extension-scheduler.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
import {
  sealCiphertextV2,
  captureSealTemplate,
  bindSealResponse
} from './seal-template.ts';
declare const operationBrand: unique symbol;
declare const sealBrand: unique symbol;
export type PrivateSealOperation = Readonly<{ [operationBrand]: true }>;
export type PrivateRecipientSeal = Readonly<{ [sealBrand]: true }>;
export type PrivateSealResult =
  | Readonly<{ status: 'sealed'; seal: PrivateRecipientSeal }>
  | Readonly<{
      status:
        'invalid' | 'busy' | 'unavailable' | 'refused' | 'stopped' | 'mismatch';
    }>;
type State = {
  run(): Promise<PrivateSealResult>;
  stop(expired: boolean): void;
};
type SavedSeal = Readonly<{
  owner: string;
  peer: string;
  destination: string;
  role: 'peer' | 'self';
  command: string;
  rumorHash: string;
  wire: string;
  current(): boolean;
}>;
const operations = new WeakMap<PrivateSealOperation, State>();
const seals = new WeakMap<PrivateRecipientSeal, SavedSeal>();
// Explicit lower-level crypto preparation, not inbox readiness, durable CAS,
// saved-encrypted evidence, a Send controller or a public/private publisher.
export function capturePrivateSealOperation(
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  role: unknown,
  review: unknown
): PrivateSealOperation | undefined {
  if (
    review !== 'reviewed_private_seal' ||
    (role !== 'peer' && role !== 'self')
  )
    return undefined;
  const inputRecord = reservedSendSnapshot(reserved),
    inputOwnership = identityMessagingOwnership(session);
  if (
    !inputRecord ||
    !inputOwnership ||
    inputRecord.owner !== inputOwnership.owner ||
    !inputOwnership.current()
  )
    return undefined;
  const record = inputRecord,
    ownership = inputOwnership,
    capturedRole = role;
  const target = capturedRole === 'self' ? record.owner : record.peer;
  let stopped = false,
    started = false,
    active: ExtensionAction | undefined;
  const current = () => {
    const observed = reservedSendSnapshot(reserved);
    return (
      !stopped &&
      ownership.current() &&
      observed?.owner === record.owner &&
      observed.id === record.id &&
      observed.peer === record.peer &&
      observed.rumorHash === record.rumorHash
    );
  };
  function stop(expired: boolean) {
    stopped = true;
    if (active) {
      if (expired) markExtensionWaitExpired(active);
      else stopExtensionAction(active);
    }
  }
  function cipherPort() {
    try {
      if (!current()) return undefined;
      const cipher = new ExtensionSigner().nip44;
      if (!current() || !cipher) return undefined;
      const encryptMethod = cipher.encrypt;
      if (!current()) return undefined;
      const decryptMethod = cipher.decrypt;
      if (
        !current() ||
        typeof encryptMethod !== 'function' ||
        typeof decryptMethod !== 'function'
      )
        return undefined;
      const encrypt = encryptMethod.bind(cipher);
      if (!current()) return undefined;
      const decrypt = decryptMethod.bind(cipher);
      if (!current()) return undefined;
      // Owned plain port: the provider's getters are never re-read inside a
      // scheduled invocation with the captured private rumor.
      return { encrypt, decrypt };
    } catch {
      return undefined;
    }
  }
  async function fresh(action: ExtensionAction): Promise<boolean> {
    if (!current()) return false;
    const result = await callExtension(action, 'key', () =>
      new ExtensionSigner().getPublicKey()
    );
    if (!current()) return false;
    if (
      result.status !== 'settled' ||
      !result.current ||
      result.value !== record.owner
    ) {
      disconnectIdentity(session);
      return false;
    }
    const cipher = cipherPort();
    if (!current()) return false;
    if (!cipher) {
      disconnectIdentity(session);
      return false;
    }
    return current();
  }
  async function work(action: ExtensionAction): Promise<PrivateSealResult> {
    active = action;
    const initial = cipherPort();
    if (!current()) return { status: 'stopped' };
    if (!initial) {
      disconnectIdentity(session);
      return { status: 'unavailable' };
    }
    if (!(await fresh(action))) return { status: 'stopped' };
    const wire = reservedSendRumorWire(reserved),
      cipher = cipherPort();
    if (!cipher && current()) disconnectIdentity(session);
    if (!wire || !cipher || !current()) return { status: 'stopped' };
    const encrypted = await callExtension(action, 'encrypt', () =>
      cipher.encrypt(target, wire)
    );
    if (!current()) return { status: 'stopped' };
    if (encrypted.status !== 'settled' || !encrypted.current)
      return { status: encrypted.status === 'denied' ? 'refused' : 'stopped' };
    if (!sealCiphertextV2(encrypted.value)) return { status: 'mismatch' };
    if (!(await fresh(action))) return { status: 'stopped' };
    const milliseconds = safeUnsignedInteger(Date.now());
    if (milliseconds === undefined || !current()) return { status: 'stopped' };
    // Public randomized timing metadata only. Never change the reserved inner
    // timestamp. No identity key, crypto implementation or future time created.
    const random = crypto.getRandomValues(new Uint32Array(1))[0],
      now = Math.floor(milliseconds / 1000),
      time = Math.max(0, now - (random % 3600));
    const captured = captureSealTemplate(record.owner, encrypted.value, time);
    if (!captured || !current()) return { status: 'mismatch' };
    const response = await callExtension(action, 'sign', () =>
      new ExtensionSigner().signEvent(JSON.parse(captured) as UnsignedEvent)
    );
    if (!current()) return { status: 'stopped' };
    if (response.status !== 'settled' || !response.current)
      return { status: response.status === 'denied' ? 'refused' : 'stopped' };
    const signed = bindSealResponse(captured, response.value);
    if (!signed) return { status: 'mismatch' };
    if (!(await fresh(action))) return { status: 'stopped' };
    if (!current()) return { status: 'stopped' };
    const seal = Object.freeze({}) as PrivateRecipientSeal;
    seals.set(seal, {
      owner: record.owner,
      peer: record.peer,
      destination: target,
      role: capturedRole,
      command: record.id,
      rumorHash: record.rumorHash,
      wire: signed,
      current
    });
    return { status: 'sealed', seal };
  }
  async function run(): Promise<PrivateSealResult> {
    if (started || !current()) return { status: 'stopped' };
    started = true;
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return { status: 'unavailable' };
    try {
      return await navigator.locks.request(
        'harvestcircle:owner:' + record.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock): Promise<PrivateSealResult> => {
          if (!lock) return { status: 'busy' };
          if (!current()) return { status: 'stopped' };
          const scheduler = browserExtensionScheduler();
          if (!scheduler) return { status: 'unavailable' };
          const result = await runExtensionAction(
            scheduler,
            {
              owner: record.owner,
              session: ownership.session,
              operation: Symbol()
            },
            current,
            work
          );
          if (!current()) return { status: 'stopped' };
          if (result.status === 'busy') return { status: 'busy' };
          return result.status === 'completed' &&
            'value' in result &&
            result.value
            ? result.value
            : { status: result.status === 'denied' ? 'refused' : 'stopped' };
        }
      );
    } catch {
      return { status: current() ? 'refused' : 'stopped' };
    } finally {
      active = undefined;
    }
  }
  const operation = Object.freeze({}) as PrivateSealOperation;
  operations.set(operation, { run, stop });
  return operation;
}
export function buildPrivateSeal(
  operation: PrivateSealOperation
): Promise<PrivateSealResult> {
  return (
    operations.get(operation)?.run() ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopPrivateSeal(operation: PrivateSealOperation): void {
  operations.get(operation)?.stop(false);
}
export function expirePrivateSealWait(operation: PrivateSealOperation): void {
  operations.get(operation)?.stop(true);
}
// Detached signed ciphertext only; a cast/copy is not a private seal token.
// Future wrapper/nested owners must retain exact original operation binding.
export function privateSealSnapshot(
  seal: PrivateRecipientSeal
): Readonly<Omit<SavedSeal, 'current'>> | undefined {
  const saved = seals.get(seal);
  return saved?.current()
    ? {
        owner: saved.owner,
        peer: saved.peer,
        destination: saved.destination,
        role: saved.role,
        command: saved.command,
        rumorHash: saved.rumorHash,
        wire: saved.wire
      }
    : undefined;
}
