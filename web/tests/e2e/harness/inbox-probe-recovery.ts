import { makeFixture as makeCryptoFixture } from './approved-signing.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxAccessController,
  closeInboxAccessController
} from '../../../src/lib/messaging/inbox-access-controller.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import { createPrivateStorageRepository } from '../../../src/lib/persistence/private-storage.ts';
import {
  captureInboxProbeRecoveryReader,
  recoverInboxProbe,
  recoveredInboxProbeBinding,
  stopInboxProbeRecoveryReader,
  type InboxProbeRecoveryReader,
  type RecoveredInboxProbe
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import {
  reserveInboxProbeIdentity,
  inboxProbeSnapshot
} from '../../../src/lib/nostr/inbox-access.ts';
import {
  captureInboxProbeSealOperation,
  buildInboxProbeSeal
} from '../../../src/lib/nostr/seal-builder.ts';
import { buildInboxProbeGiftwrap } from '../../../src/lib/nostr/giftwrap-builder.ts';
import {
  commitInboxProbe,
  inboxProbeAcknowledgementSnapshot,
  commitInboxProbeArchiveAttempt,
  commitInboxProbeReadbackAttempt,
  type InboxProbeAcknowledgement
} from '../../../src/lib/persistence/inbox-probes.ts';
import {
  getPrivatePool,
  closePrivatePool,
  publishInboxProbeArchiveAttempt,
  readInboxProbeAttempt,
  type PrivatePool,
  type InboxProbeArchiveAttemptResult,
  type InboxProbeReadbackAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
import {
  captureInboxProbePublicationAction,
  captureInboxProbePublication,
  captureInboxProbeReadback,
  type InboxProbePublicationAction
} from '../../../src/lib/nostr/private-publisher.ts';
import {
  captureInboxProbeNonceReader,
  readInboxProbeNonce,
  stopInboxProbeNonceReader,
  type InboxProbeNonceReader
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import type { InboxProbeIdentity } from '../../../src/lib/nostr/inbox-access.ts';
import type { InboxProbeGiftwrap } from '../../../src/lib/nostr/giftwrap-builder.ts';
import {
  readRelayPolicy,
  validateRelayPolicy,
  type RelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  inboxOperatorQualificationWire,
  validateInboxOperatorQualification,
  type InboxOperatorQualification
} from '../../../src/lib/config/inbox-operators.ts';
import type { UnsignedEvent } from 'applesauce-core/helpers';
export * from './inbox-probe-nonce.ts';
export {
  readRelayPolicy,
  validateRelayPolicy,
  inboxOperatorQualificationWire,
  validateInboxOperatorQualification
};

// All actors are actual source factories. The emulated extension may outlive a
// product realm, as a real extension does; no secret leaves its test closure.
function createRecoveryPublicScope(policy: RelayPolicy) {
  const context = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(context, policy);
  if (!runtime) throw Error('actual public discovery scope');
  return { context, runtime };
}
export async function openRecoveryContext(
  owner: string,
  policy: RelayPolicy,
  operator: InboxOperatorQualification,
  localId: string,
  shared?: ReturnType<typeof createRecoveryPublicScope>
) {
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const session = await createPrivateSession(
    identity,
    'reviewed_private_session'
  );
  const opened = await openBrowserDatabase();
  if (!session || opened.state !== 'ready')
    throw Error('fresh actual recovery actors');
  const repository = createPrivateStorageRepository(opened.owner, owner);
  if (!repository) throw Error('actual original native namespace');
  const { context, runtime } = shared ?? createRecoveryPublicScope(policy);
  const view = createPublicView(runtime),
    run = beginPublicViewRun(view),
    resolver = resolveInboxPreference(view, run, owner);
  await settleInboxResolver(resolver);
  const access = createInboxAccessController(session, policy, resolver);
  if (!access) throw Error('fresh actual recovery controller');
  let selected: InboxProbeRecoveryReader | undefined,
    proof: RecoveredInboxProbe | undefined;
  function capture(
    review: unknown = 'reviewed_inbox_probe_recovery',
    id: unknown = localId
  ) {
    return captureInboxProbeRecoveryReader(
      repository!,
      identity,
      session!,
      policy,
      access!,
      operator,
      id,
      review
    );
  }
  return {
    owner,
    identity,
    session,
    repository,
    policy,
    operator,
    access,
    localId,
    capture,
    reader: () => selected,
    proof: () => proof,
    binding: () =>
      proof &&
      recoveredInboxProbeBinding(
        proof,
        repository,
        session,
        access,
        policy,
        operator
      ),
    async recover(reader = (selected ??= capture())) {
      if (!reader) return { status: 'invalid' as const };
      selected ??= reader;
      const result = await recoverInboxProbe(reader);
      if ('proof' in result) proof = result.proof;
      return result;
    },
    churnHead: () => beginPublicViewRun(view),
    close() {
      if (selected) stopInboxProbeRecoveryReader(selected);
      closeInboxAccessController(access);
      closePrivateSession(session);
      disconnectIdentity(identity);
      disposePublicView(view);
      if (!shared) closePublicRuntime(context);
      closeBrowserDatabase(opened.owner);
    }
  };
}
export async function makeRecoveryFixture() {
  const host = createHostedProvider();
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('actual original native database');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, host.owner);
  if (!repository) throw Error('actual original native namespace');
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: true,
          nip50: false
        }
      ],
      inbox: [
        'wss://inbox.example.org',
        'wss://peer.example.org',
        'wss://archive.example.org'
      ].map((origin) => ({ origin, read: true, write: true })),
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    })
  );
  if (!policy) throw Error('actual fixture policy');
  function operator(unsupported = false, unconfigured = false) {
    const result = validateInboxOperatorQualification(
      JSON.stringify({
        schemaVersion: 1,
        revision: 1,
        authority: unconfigured ? 'unconfigured' : 'source_fixture',
        policyWire: JSON.stringify(readRelayPolicy(policy!)),
        evidenceSha256: unconfigured ? null : 'a'.repeat(64),
        relays: unconfigured
          ? []
          : readRelayPolicy(policy!).inbox.map(({ origin }) => ({
              origin,
              auth: 'not_required',
              recipientRead: 'supported',
              namedArchiveAck: 'supported',
              exactReadback: unsupported ? 'unsupported' : 'supported',
              retention: 'operator_defined'
            }))
      })
    );
    if (!result) throw Error('source-labelled supported operator fixture');
    return result;
  }
  const qualification = operator();
  let publicOwner: ReturnType<typeof createRecoveryPublicScope> | undefined,
    original: Awaited<ReturnType<typeof openRecoveryContext>> | undefined,
    recovery: Awaited<ReturnType<typeof openRecoveryContext>> | undefined;
  let originalId: string | undefined;
  let probe: InboxProbeIdentity | undefined,
    wrap: InboxProbeGiftwrap | undefined,
    acknowledgement: InboxProbeAcknowledgement | undefined,
    action: InboxProbePublicationAction | undefined,
    pool: PrivatePool | undefined,
    lastRead: InboxProbeReadbackAttemptResult | undefined,
    nonceReader: InboxProbeNonceReader | undefined;
  function context() {
    if (!original) throw Error('actual original context missing');
    return original;
  }
  function acquire() {
    const r = context();
    pool ??= getPrivatePool(
      r.session,
      policy!,
      ['wss://inbox.example.org'],
      Object.freeze({}),
      'access'
    );
    if (!pool) throw Error('actual original probe pool');
    return pool;
  }
  return {
    ...host,
    repository,
    policy,
    qualification,
    operator,
    get identity() {
      return context().identity;
    },
    get session() {
      return context().session;
    },
    controller: () => original?.access,
    probeIdentity: () => probe,
    async capture() {
      publicOwner ??= createRecoveryPublicScope(policy);
      original ??= await openRecoveryContext(
        host.owner,
        policy,
        qualification,
        crypto.randomUUID(),
        publicOwner
      );
      return original.access;
    },
    async reserve() {
      const r = context(),
        result = await reserveInboxProbeIdentity(
          repository,
          r.session,
          policy,
          r.access,
          qualification,
          'reviewed_inbox_probe_reservation'
        );
      if ('identity' in result) {
        probe = result.identity;
        originalId = inboxProbeSnapshot(probe)?.id;
        if (!originalId)
          throw Error('genuine reserved command metadata missing');
      }
      return result;
    },
    async prepare() {
      const r = context();
      if (!probe) throw Error('actual R0 missing');
      const operation = captureInboxProbeSealOperation(
        r.session,
        probe,
        'reviewed_inbox_probe_seal'
      );
      if (!operation) throw Error('actual seal admission');
      const result = await buildInboxProbeSeal(operation);
      if (!('seal' in result)) return result.status;
      wrap = buildInboxProbeGiftwrap(result.seal, 'reviewed_inbox_probe_wrap');
      return wrap ? 'wrapped' : 'invalid';
    },
    async commitOriginal() {
      if (!wrap) throw Error('actual original wrap missing');
      const result = await commitInboxProbe(
        repository,
        context().session,
        wrap,
        'reviewed_inbox_probe_commit'
      );
      if ('acknowledgement' in result) acknowledgement = result.acknowledgement;
      return result;
    },
    snapshot: () =>
      acknowledgement && inboxProbeAcknowledgementSnapshot(acknowledgement),
    async publish() {
      const r = context();
      if (!acknowledgement) throw Error('actual original ACK missing');
      action ??= captureInboxProbePublicationAction(
        repository,
        r.session,
        policy,
        acknowledgement,
        'reviewed_inbox_probe_network'
      );
      const permission =
        action &&
        captureInboxProbePublication(
          repository,
          r.session,
          policy,
          acknowledgement,
          action,
          'wss://inbox.example.org',
          'reviewed_inbox_probe_publication'
        );
      if (!permission) throw Error('actual original PUB admission');
      return publishInboxProbeArchiveAttempt(
        acquire(),
        permission,
        new AbortController().signal
      );
    },
    append: (result: InboxProbeArchiveAttemptResult) =>
      commitInboxProbeArchiveAttempt(repository, result),
    async read() {
      const r = context();
      if (!acknowledgement || !action)
        throw Error('actual original PUB action missing');
      const permission = captureInboxProbeReadback(
        repository,
        r.session,
        policy,
        acknowledgement,
        action,
        'wss://inbox.example.org',
        'reviewed_inbox_probe_readback'
      );
      if (!permission) throw Error('actual original read admission');
      return (lastRead = await readInboxProbeAttempt(
        acquire(),
        permission,
        new AbortController().signal
      ));
    },
    appendRead: (result: InboxProbeReadbackAttemptResult) =>
      commitInboxProbeReadbackAttempt(repository, result),
    nonceCapture() {
      const r = context();
      return (
        lastRead &&
        captureInboxProbeNonceReader(
          repository,
          r.identity,
          r.session,
          policy,
          lastRead,
          'reviewed_inbox_probe_nonce_decryption'
        )
      );
    },
    async nonce() {
      const r = context();
      nonceReader ??=
        lastRead &&
        captureInboxProbeNonceReader(
          repository,
          r.identity,
          r.session,
          policy,
          lastRead,
          'reviewed_inbox_probe_nonce_decryption'
        );
      return nonceReader
        ? readInboxProbeNonce(nonceReader)
        : { status: 'invalid' as const };
    },
    nonceReader: () => nonceReader,
    foreignRepository() {
      const fixture = makeCryptoFixture();
      const other = createPrivateStorageRepository(database, fixture.owner);
      fixture.close();
      if (!other) throw Error('actual other namespace');
      return other;
    },
    copiedPolicy() {
      const other = validateRelayPolicy(
        JSON.stringify(readRelayPolicy(policy))
      );
      if (!other) throw Error('actual copied policy');
      return other;
    },
    async stored() {
      const tx = browserDatabaseTransaction(
          database,
          ['private_sends'],
          'readonly'
        ),
        request = tx.objectStore('private_sends').getAll();
      return new Promise<{ owner: string; id: string; wire: string }[]>(
        (resolve, reject) => {
          tx.addEventListener('complete', () =>
            resolve(
              request.result as { owner: string; id: string; wire: string }[]
            )
          );
          tx.addEventListener('abort', () =>
            reject(Error('actual fixture read abort'))
          );
        }
      );
    },
    async inject(rows: { owner: string; id: string; wire: string }[]) {
      const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readwrite'
      );
      for (const row of rows) tx.objectStore('private_sends').put(row);
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('actual fixture write abort'))
        );
      });
    },
    async removeOriginal() {
      if (!originalId) throw Error('original command metadata missing');
      const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readwrite'
      );
      tx.objectStore('private_sends').delete([host.owner, originalId]);
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('actual native deletion abort'))
        );
      });
    },
    async fresh() {
      // Recovery explicitly permits expired original memory authority. Only the
      // original genuine reserved ID survives as inert selection metadata.
      if (!originalId) throw Error('original command metadata missing');
      const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readonly'
      );
      const request = tx
        .objectStore('private_sends')
        .get([host.owner, originalId]);
      const row = await new Promise<unknown>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve(request.result));
        tx.addEventListener('abort', () =>
          reject(Error('actual original selection abort'))
        );
      });
      if (!row) throw Error('stored original selection missing');
      if (pool) closePrivatePool(pool);
      original!.close();
      if (inboxProbeSnapshot(probe!)) throw Error('old nonce still visible');
      recovery = await openRecoveryContext(
        host.owner,
        policy,
        qualification,
        originalId,
        publicOwner
      );
      return recovery;
    },
    close() {
      if (nonceReader) stopInboxProbeNonceReader(nonceReader);
      if (pool) closePrivatePool(pool);
      recovery?.close();
      original?.close();
      if (publicOwner) closePublicRuntime(publicOwner.context);
      closeBrowserDatabase(database);
      host.close();
    }
  };
}

// A parent-realm test extension host supports actual child-frame destruction and
// reload without persisting/exposing its random key. It is controlled evidence,
// not installed-extension or operator qualification.
export function createHostedProvider() {
  const cryptoFixture = makeCryptoFixture(10050);
  let keys = 0,
    signs = 0,
    encrypts = 0,
    decrypts = 0,
    mode:
      | 'normal'
      | 'hold_key'
      | 'hold_decrypt'
      | 'wrong_key'
      | 'refuse_decrypt'
      | 'wrong_decrypt' = 'normal',
    waiting = '',
    release: (() => void) | undefined;
  const restorations = new Map<symbol, () => void>();
  function retain(off: () => void) {
    const id = Symbol();
    restorations.set(id, off);
    return () => {
      off();
      restorations.delete(id);
    };
  }
  async function pause(stage: string) {
    if (mode !== 'hold_' + stage) return;
    waiting = stage;
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    waiting = '';
    release = undefined;
  }
  const cipher = {
    encrypt: (target: string, wire: string) => {
      encrypts++;
      return Promise.resolve(cryptoFixture.encrypt(target, wire));
    },
    decrypt: async (target: string, wire: string) => {
      decrypts++;
      await pause('decrypt');
      if (mode === 'refuse_decrypt')
        throw Error('controlled actual provider refusal');
      return mode === 'wrong_decrypt'
        ? 'substituted plaintext'
        : cryptoFixture.decrypt(target, wire);
    }
  };
  const provider = {
    getPublicKey: async () => {
      keys++;
      await pause('key');
      return mode === 'wrong_key' ? 'f'.repeat(64) : cryptoFixture.owner;
    },
    signEvent: (input: UnsignedEvent) => {
      signs++;
      return Promise.resolve(cryptoFixture.sign(input));
    },
    nip44: cipher
  };
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: provider
  });
  return {
    owner: cryptoFixture.owner,
    preferenceWire: (origins: string[]) =>
      JSON.stringify(
        cryptoFixture.sign({
          kind: 10050,
          created_at: Math.floor(Date.now() / 1000),
          tags: origins.map((origin) => ['relay', origin]),
          content: ''
        })
      ),
    head: () =>
      JSON.stringify(
        cryptoFixture.sign({
          kind: 10050,
          created_at: Math.floor(Date.now() / 1000),
          tags: [['relay', 'wss://inbox.example.org']],
          content: ''
        })
      ),
    counts: () => ({ keys, signs, encrypts, decrypts }),
    mode: (next: typeof mode) => {
      mode = next;
    },
    waiting: () => waiting,
    settle: () => release?.(),
    keyCallback(hit: number, callback: () => void | Promise<void>) {
      const original = provider.getPublicKey.bind(provider);
      let n = 0;
      provider.getPublicKey = async () => {
        if (++n === hit) await callback();
        return original();
      };
      return retain(() => {
        provider.getPublicKey = original;
      });
    },
    cipherGetter(callback: () => void) {
      const descriptor = Object.getOwnPropertyDescriptor(provider, 'nip44')!;
      Object.defineProperty(provider, 'nip44', {
        configurable: true,
        get() {
          callback();
          return cipher;
        }
      });
      return retain(() => {
        Object.defineProperty(provider, 'nip44', descriptor);
      });
    },
    decryptCallback(
      hit: number,
      transform: (wire: string) => string | Promise<string>
    ) {
      const original = cipher.decrypt.bind(cipher),
        descriptor = Object.getOwnPropertyDescriptor(cipher, 'decrypt')!;
      let n = 0,
        receivers = 0;
      Object.defineProperty(cipher, 'decrypt', {
        configurable: true,
        value: async function (
          this: typeof cipher,
          target: string,
          wire: string
        ) {
          if (this !== cipher) throw Error('wrong actual SDK cipher receiver');
          receivers++;
          const opened = await original(target, wire);
          return ++n === hit ? transform(opened) : opened;
        }
      });
      return {
        receivers: () => receivers,
        restore: retain(() => {
          Object.defineProperty(cipher, 'decrypt', descriptor);
        })
      };
    },
    signSeal(wire: string, change: Record<string, unknown>) {
      const seal = JSON.parse(wire) as UnsignedEvent;
      return JSON.stringify(
        cryptoFixture.sign({
          kind: seal.kind,
          created_at: seal.created_at,
          tags: seal.tags,
          content: seal.content,
          ...change
        })
      );
    },
    close() {
      for (const [id, off] of restorations) {
        off();
        restorations.delete(id);
      }
      cryptoFixture.close();
    }
  };
}

// Extension messages return data in the requesting document's realm. Direct
// parent objects fail the stock SDK's instanceof-Object event admission. Clone
// only the signed public event response; account key custody stays in the host.
export function installHostedProvider() {
  type Port = {
    getPublicKey(): Promise<string>;
    signEvent(input: UnsignedEvent): Promise<Record<string, unknown>>;
    nip44: {
      encrypt(target: string, wire: string): Promise<string>;
      decrypt(target: string, wire: string): Promise<string>;
    };
  };
  const host = Reflect.get(window.parent, 'nostr') as Port;
  const cipher = host.nip44;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: host.getPublicKey.bind(host),
      signEvent: async (input: UnsignedEvent) => {
        const signed = await host.signEvent(input);
        return JSON.parse(JSON.stringify(signed)) as Record<string, unknown>;
      },
      nip44: {
        encrypt: cipher.encrypt.bind(cipher),
        decrypt: cipher.decrypt.bind(cipher)
      }
    }
  });
}

export async function prepareHostedOriginal(
  context: Awaited<ReturnType<typeof openRecoveryContext>>
) {
  const { repository, session, policy, access, operator } = context;
  const reserved = await reserveInboxProbeIdentity(
    repository,
    session,
    policy,
    access,
    operator,
    'reviewed_inbox_probe_reservation'
  );
  if (!('identity' in reserved)) throw Error('actual hosted R0');
  const operation = captureInboxProbeSealOperation(
    session,
    reserved.identity,
    'reviewed_inbox_probe_seal'
  );
  if (!operation) throw Error('actual hosted seal operation');
  const sealed = await buildInboxProbeSeal(operation);
  if (!('seal' in sealed)) throw Error('actual hosted seal');
  const wrapped = buildInboxProbeGiftwrap(
    sealed.seal,
    'reviewed_inbox_probe_wrap'
  );
  if (!wrapped) throw Error('actual hosted wrap');
  const committed = await commitInboxProbe(
    repository,
    session,
    wrapped,
    'reviewed_inbox_probe_commit'
  );
  if (!('acknowledgement' in committed)) throw Error('actual hosted native R1');
  return inboxProbeAcknowledgementSnapshot(committed.acknowledgement)!.record;
}
