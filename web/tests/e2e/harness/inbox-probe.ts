import { makeControllerFixture } from './inbox-access-controller.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import { createPrivateStorageRepository } from '../../../src/lib/persistence/private-storage.ts';
import {
  validateInboxOperatorQualification,
  type InboxOperatorQualification
} from '../../../src/lib/config/inbox-operators.ts';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession,
  type PrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  reserveInboxProbeIdentity,
  inboxProbeSnapshot,
  inboxProbeRumorWire,
  type InboxProbeIdentity
} from '../../../src/lib/nostr/inbox-access.ts';
import {
  captureInboxProbeSealOperation,
  buildInboxProbeSeal,
  type InboxProbeSealOperation,
  type InboxProbeRecipientSeal
} from '../../../src/lib/nostr/seal-builder.ts';
import {
  buildInboxProbeGiftwrap,
  inboxProbeGiftwrapSnapshot,
  type InboxProbeGiftwrap
} from '../../../src/lib/nostr/giftwrap-builder.ts';
import {
  getEventHash,
  generateSecretKey,
  getPublicKey,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import { nip44 } from 'applesauce-core/helpers/encryption';
export * from '../../../src/lib/nostr/inbox-access.ts';
export * from '../../../src/lib/nostr/seal-builder.ts';
export * from '../../../src/lib/nostr/giftwrap-builder.ts';
export * from '../../../src/lib/nostr/verify-outbound-envelope.ts';
export * from '../../../src/lib/messaging/inbox-access-controller.ts';
export * from '../../../src/lib/messaging/send-identity.ts';
export * from '../../../src/lib/runtime/private-session.ts';
export type Mode =
  | 'normal'
  | 'hold_key'
  | 'hold_encrypt'
  | 'hold_decrypt'
  | 'hold_sign'
  | 'wrong_key'
  | 'refuse_encrypt'
  | 'refuse_decrypt'
  | 'refuse_sign'
  | 'wrong_encrypt'
  | 'wrong_decrypt'
  | 'wrong_sign_author'
  | 'wrong_sign_time'
  | 'wrong_sign_tags'
  | 'cached_signature'
  | 'stop_encrypt_getter'
  | 'stop_decrypt_getter';
type Provider = {
  getPublicKey(): Promise<string>;
  signEvent(input: UnsignedEvent): Promise<Record<string, unknown>>;
  nip44: {
    encrypt(target: string, wire: string): Promise<string>;
    decrypt(target: string, wire: string): Promise<string>;
  };
};
// Actual source owners/SDK/Chromium storage and random keys, controlled provider
// and anonymous discovery transport. No installed extension/operator/Q evidence.
export async function makeProbeFixture() {
  const base = await makeControllerFixture(true),
    opened = await openBrowserDatabase();
  if (opened.state !== 'ready')
    throw Error('native fixture database unavailable');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, base.owner);
  if (!repository) throw Error('actual private namespace');
  const provider = Reflect.get(window, 'nostr') as Provider;
  const key = provider.getPublicKey.bind(provider),
    sign = provider.signEvent.bind(provider),
    encrypt = provider.nip44.encrypt.bind(provider.nip44),
    decrypt = provider.nip44.decrypt.bind(provider.nip44);
  let mode: Mode = 'normal',
    release: (() => void) | undefined,
    waiting = '',
    keys = 0,
    encrypts = 0,
    decrypts = 0,
    signs = 0;
  let controller: Awaited<ReturnType<typeof base.capture>> | undefined,
    identity: InboxProbeIdentity | undefined,
    operation: InboxProbeSealOperation | undefined,
    seal: InboxProbeRecipientSeal | undefined,
    wrap: InboxProbeGiftwrap | undefined;
  async function pause(stage: string) {
    if (mode !== 'hold_' + stage) return;
    waiting = stage;
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    waiting = '';
    release = undefined;
  }
  provider.getPublicKey = async () => {
    keys++;
    await pause('key');
    return mode === 'wrong_key' ? 'f'.repeat(64) : key();
  };
  provider.signEvent = async (input) => {
    signs++;
    await pause('sign');
    if (mode === 'refuse_sign') throw Error('controlled refusal');
    const changed =
      mode === 'wrong_sign_time'
        ? { ...input, created_at: input.created_at + 1 }
        : mode === 'wrong_sign_tags'
          ? { ...input, tags: [['p', base.owner]] }
          : input;
    if (mode === 'wrong_sign_author') base.mode('author');
    let response: Record<string, unknown>;
    try {
      response = await sign(changed);
    } finally {
      base.mode('exact');
    }
    return mode === 'cached_signature'
      ? { ...response, sig: '0'.repeat(128) }
      : response;
  };
  Object.defineProperty(provider, 'nip44', {
    configurable: true,
    value: {
      get encrypt() {
        if (mode === 'stop_encrypt_getter' && controller)
          closeInboxAccessController(controller);
        return async (target: string, wire: string) => {
          encrypts++;
          await pause('encrypt');
          if (mode === 'refuse_encrypt') throw Error('controlled refusal');
          return encrypt(
            target,
            mode === 'wrong_encrypt' ? 'substituted rumor' : wire
          );
        };
      },
      get decrypt() {
        if (mode === 'stop_decrypt_getter' && controller)
          closeInboxAccessController(controller);
        return async (target: string, wire: string) => {
          decrypts++;
          await pause('decrypt');
          if (mode === 'refuse_decrypt') throw Error('controlled refusal');
          return mode === 'wrong_decrypt'
            ? 'substituted plaintext'
            : decrypt(target, wire);
        };
      }
    }
  });
  const operator = (unsupported = false, unconfigured = false) => {
    const value = validateInboxOperatorQualification(
      JSON.stringify({
        schemaVersion: 1,
        revision: 1,
        authority: unconfigured ? 'unconfigured' : 'source_fixture',
        policyWire: JSON.stringify(readRelayPolicy(base.policy)),
        evidenceSha256: unconfigured ? null : 'a'.repeat(64),
        relays: unconfigured
          ? []
          : readRelayPolicy(base.policy).inbox.map(({ origin }) => ({
              origin,
              auth: 'not_required',
              recipientRead: 'supported',
              namedArchiveAck: 'supported',
              exactReadback: unsupported ? 'unsupported' : 'supported',
              retention: 'operator_defined'
            }))
      })
    );
    if (!value) throw Error('source-labelled operator fixture');
    return value;
  };
  const qualification = operator();
  let otherIdentity: ReturnType<typeof createIdentitySession> | undefined,
    otherSession: PrivateSession | undefined;
  async function stored() {
    const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readonly'
      ),
      req = tx.objectStore('private_sends').getAll();
    return new Promise<{ owner: string; id: string; wire: string }[]>(
      (resolve, reject) => {
        tx.addEventListener('complete', () =>
          resolve(req.result as { owner: string; id: string; wire: string }[])
        );
        tx.addEventListener('abort', () => reject(Error('inspection aborted')));
      }
    );
  }
  return {
    ...base,
    repository,
    qualification,
    operator,
    foreignRepository() {
      const secret = generateSecretKey(),
        owner = getPublicKey(secret);
      secret.fill(0);
      const result = createPrivateStorageRepository(database, owner);
      if (!result) throw Error('actual foreign namespace');
      return result;
    },
    copiedPolicy() {
      const result = validateRelayPolicy(
        JSON.stringify(readRelayPolicy(base.policy))
      );
      if (!result) throw Error('genuine alternate policy token');
      return result;
    },
    async foreignSession() {
      otherIdentity = createIdentitySession();
      await connectIdentity(otherIdentity);
      await probeIdentityMessaging(otherIdentity, 'reviewed_self_copy');
      otherSession = await createPrivateSession(
        otherIdentity,
        'reviewed_private_session'
      );
      if (!otherSession) throw Error('genuine same-owner other session');
      return otherSession;
    },
    async capture() {
      controller = await base.capture();
      return controller;
    },
    controller: () => controller,
    probeIdentity: () => identity,
    operation: () => operation,
    seal: () => seal,
    wrap: () => wrap,
    mode(next: Mode) {
      mode = next;
    },
    waiting: () => waiting,
    settle: () => release?.(),
    counts: () => ({ keys, encrypts, decrypts, signs }),
    stored,
    async fillCapacity() {
      const secret = generateSecretKey(),
        peer = getPublicKey(secret);
      secret.fill(0);
      const tx = browserDatabaseTransaction(
          database,
          ['private_sends'],
          'readwrite'
        ),
        store = tx.objectStore('private_sends');
      for (let i = 0; i < 100; i++) {
        const id = crypto.randomUUID();
        store.put({
          owner: base.owner,
          id,
          wire: JSON.stringify({
            schema: 1,
            family: 'private_send_reservation',
            owner: base.owner,
            id,
            revision: 0,
            peer,
            rumorHash: 'a'.repeat(64),
            createdAt: 1
          })
        });
      }
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('fixture capacity abort'))
        );
      });
    },
    installNativeAbort() {
      const original = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      )?.value as IDBObjectStore['put'];
      let fired = false;
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        const request = original.apply(this, args);
        if (!fired && this.name === 'private_sends') {
          fired = true;
          const transaction = this.transaction;
          request.addEventListener('success', () => transaction.abort());
        }
        return request;
      };
      return () => {
        IDBObjectStore.prototype.put = original;
      };
    },
    installChangedReadback() {
      const original = Object.getOwnPropertyDescriptor(
        IDBDatabase.prototype,
        'transaction'
      )?.value as IDBDatabase['transaction'];
      let fired = false;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        const tx = original.apply(this, args);
        if (
          !fired &&
          args[1] === 'readwrite' &&
          tx.objectStoreNames.contains('private_sends')
        ) {
          fired = true;
          tx.addEventListener('complete', () => {
            const next = original.call(this, ['private_sends'], 'readwrite'),
              store = next.objectStore('private_sends'),
              rows = store.getAll();
            rows.addEventListener('success', () => {
              for (const row of rows.result as {
                owner: string;
                id: string;
                wire: string;
              }[]) {
                if (row.owner !== base.owner) continue;
                const parsed = JSON.parse(row.wire) as { createdAt: number };
                parsed.createdAt++;
                store.put({ ...row, wire: JSON.stringify(parsed) });
              }
            });
          });
        }
        return tx;
      };
      return () => {
        IDBDatabase.prototype.transaction = original;
      };
    },
    async reserve(
      selected: InboxOperatorQualification = qualification,
      review = 'reviewed_inbox_probe_reservation'
    ) {
      if (!controller) throw Error('missing genuine controller');
      const result = await reserveInboxProbeIdentity(
        repository,
        base.session,
        base.policy,
        controller,
        selected,
        review
      );
      if ('identity' in result) identity = result.identity;
      return {
        status: result.status,
        disposition: result.disposition,
        snapshot: identity && inboxProbeSnapshot(identity)
      };
    },
    async prepareSeal() {
      if (!identity) throw Error('missing genuine reserved probe');
      operation = captureInboxProbeSealOperation(
        base.session,
        identity,
        'reviewed_inbox_probe_seal'
      );
      if (!operation) return 'unavailable';
      const result = await buildInboxProbeSeal(operation);
      if (result.status === 'sealed') {
        seal = result.seal;
        return 'sealed';
      }
      return result.status;
    },
    wrapOriginal() {
      if (!seal) throw Error('missing genuine original seal');
      wrap = buildInboxProbeGiftwrap(seal, 'reviewed_inbox_probe_wrap');
      return wrap ? 'wrapped' : 'wrap_failed';
    },
    async prepare() {
      const result = await this.prepareSeal();
      return result === 'sealed' ? this.wrapOriginal() : result;
    },
    installWrapFault() {
      const original = Object.getOwnPropertyDescriptor(nip44.v2, 'encrypt')
        ?.value as typeof nip44.v2.encrypt;
      let failing = true,
        calls = 0;
      nip44.v2.encrypt = (...args: Parameters<typeof nip44.v2.encrypt>) => {
        calls++;
        if (failing) throw Error('controlled SDK wrapper failure');
        return original.apply(nip44.v2, args);
      };
      return {
        allow: () => {
          failing = false;
        },
        calls: () => calls,
        restore: () => {
          nip44.v2.encrypt = original;
        }
      };
    },
    async decryptOriginal() {
      if (!identity || !wrap) throw Error('missing genuine artifact');
      const snapshot = inboxProbeSnapshot(identity),
        artifact = inboxProbeGiftwrapSnapshot(wrap),
        original = inboxProbeRumorWire(identity, base.session);
      if (!snapshot || !artifact || !original)
        throw Error('stale actual artifact');
      const outer = JSON.parse(artifact.wire) as {
          pubkey: string;
          content: string;
        },
        sealWire = await decrypt(outer.pubkey, outer.content),
        signed = verifyEnvelope(sealWire),
        nested = JSON.parse(sealWire) as { pubkey: string; content: string },
        rumorWire = await decrypt(nested.pubkey, nested.content),
        rumor = JSON.parse(rumorWire) as UnsignedEvent & { id: string };
      return {
        outerVerified: verifyEnvelope(artifact.wire).ok,
        sealVerified: signed.ok,
        exactRumor: rumorWire === original,
        hash:
          getEventHash(rumor) === snapshot.rumorHash &&
          rumor.id === snapshot.rumorHash,
        owner: rumor.pubkey === base.owner,
        recipient:
          JSON.stringify(rumor.tags) === JSON.stringify([['p', base.owner]]),
        createdAt: rumor.created_at === snapshot.createdAt,
        kind: rumor.kind,
        diagnostic:
          /^Harvestcircle private inbox access check v1: [0-9a-f]{64}$/.test(
            rumor.content
          )
      };
    },
    close() {
      release?.();
      if (otherSession) closePrivateSession(otherSession);
      if (otherIdentity) disconnectIdentity(otherIdentity);
      base.close();
      closeBrowserDatabase(database);
    }
  };
}
import { closeInboxAccessController } from '../../../src/lib/messaging/inbox-access-controller.ts';
