// HC_TEST_ONLY_RELAY: original production UI/storage with a controlled SDK.
// No loopback or controlled provider result represents live extension Q.
import { mount, unmount } from 'svelte';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import { captureEnquiryContext } from '../../../src/lib/messaging/enquiry-context.ts';
import {
  captureEnquiryRumor,
  stopRumorPlan
} from '../../../src/lib/messaging/rumor-plan.ts';
import {
  reserveSendIdentity,
  reservedSendSnapshot
} from '../../../src/lib/messaging/send-identity.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  preparePairedDelivery,
  preparedPairedDelivery,
  stopSelfRecoveryPreparation
} from '../../../src/lib/messaging/prepare-send.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import { createInboxRoutePlan } from '../../../src/lib/messaging/inbox-routing.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';

import Panel from '../../../src/lib/components/LocalDataReview.svelte';
import { makeFixture } from './approved-signing.ts';
import { finalizeEvent, getEventHash } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  probeIdentityViewMessaging,
  identityViewSession,
  disconnectIdentityView,
  closeIdentityView
} from '../../../src/lib/runtime/view-context.ts';
import {
  createLocalDataReview,
  reviewLocalData,
  localDataReviewSnapshot,
  previewLocalDataRemoval,
  confirmLocalDataRemoval,
  closeLocalDataReview
} from '../../../src/lib/persistence/local-data-review.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicDraftRepository,
  createPublicDraft,
  savePublicDraft
} from '../../../src/lib/persistence/drafts.ts';
import {
  createPublicQuotaRepository,
  admitPublicOperation
} from '../../../src/lib/persistence/quota.ts';
import { identityLocalOwnership } from '../../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  withIdlePrivateLocalStorage
} from '../../../src/lib/runtime/private-session.ts';
import {
  createPrivateSendReservationRepository,
  reservePrivateSendReservation
} from '../../../src/lib/persistence/private-send-reservations.ts';
import {
  createPrivateStorageRepository,
  commitPrivateRecord,
  listPrivateReceivedRecords
} from '../../../src/lib/persistence/private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordIdentity,
  privateRecordSnapshot,
  type PrivateSendOperation
} from '../../../src/lib/persistence/private-records.ts';

export async function createFixture(mode = 'normal') {
  const signer = makeFixture(),
    foreign = makeFixture(),
    owner = signer.owner;
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let changedKey = false;
  let counts = { reads: 0, signs: 0, encrypts: 0, decrypts: 0 };
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey() {
        counts.reads++;
        return Promise.resolve(changedKey ? foreign.owner : owner);
      },
      signEvent(template: Parameters<typeof signer.sign>[0]) {
        counts.signs++;
        return Promise.resolve(signer.sign(template));
      },
      nip44: {
        encrypt(peer: string, text: string) {
          counts.encrypts++;
          return Promise.resolve(signer.encrypt(peer, text));
        },
        decrypt(peer: string, text: string) {
          counts.decrypts++;
          return Promise.resolve(signer.decrypt(peer, text));
        }
      }
    }
  });
  const identity = createIdentityViewContext();
  mountIdentityView(identity);
  await connectIdentityView(identity);
  if (mode === 'messaging' || mode === 'outbox')
    await probeIdentityViewMessaging(identity, 'reviewed_self_copy');
  if (!identityViewSession(identity))
    throw Error('original connected identity absent');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('native database absent');
  const database = opened.owner,
    drafts = createPublicDraftRepository(database, owner),
    foreignDrafts = createPublicDraftRepository(database, foreign.owner),
    quota = createPublicQuotaRepository(database, owner),
    priv = createPrivateStorageRepository(database, owner),
    foreignPriv = createPrivateStorageRepository(database, foreign.owner);
  if (!drafts || !foreignDrafts || !quota || !priv || !foreignPriv)
    throw Error('original namespaces absent');
  const form = {
    title: 'PUBLIC DRAFT BODY SENTINEL',
    description: '',
    location: '',
    amount: '',
    currency: '',
    unit: '',
    quantity: '',
    contactType: '' as const,
    contactValue: ''
  };
  const first = await createPublicDraft(drafts, form),
    second = await createPublicDraft(drafts, form),
    other = await createPublicDraft(foreignDrafts, {
      ...form,
      title: 'PRIVATE OWNER B SENTINEL'
    });
  if (
    !first.ok ||
    !second.ok ||
    !other.ok ||
    !(await admitPublicOperation(quota, signer.id, signer.record)).ok
  )
    throw Error('original public data absent');
  const outerSecret = new Uint8Array(32).fill(69),
    ids: string[] = [];
  try {
    for (const [target, repository, offset] of [
      [owner, priv, 0],
      [owner, priv, 1],
      [foreign.owner, foreignPriv, 2]
    ] as const) {
      const key = nip44.v2.utils.getConversationKey(outerSecret, target);
      let content: string;
      try {
        content = nip44.v2.encrypt('controlled opaque seal bytes', key);
      } finally {
        key.fill(0);
      }
      const outer = finalizeEvent(
        {
          kind: 1059,
          created_at: 1700000000 + offset,
          tags: [['p', target]],
          content
        },
        outerSecret
      );
      const row = decodePrivateRecord(
        JSON.stringify({
          schema: 1,
          family: 'received_envelope',
          owner: target,
          id: outer.id,
          revision: 0,
          outer: JSON.stringify(outer),
          observedAtMilliseconds: 100,
          sources: ['wss://archive.example.org'],
          read: null
        }),
        target,
        outer.id
      );
      if (
        !row.ok ||
        !(await commitPrivateRecord(repository, row.value, null)).ok
      )
        throw Error('real encrypted structural copy absent');
      if (target === owner) ids.push(outer.id);
    }
  } finally {
    outerSecret.fill(0);
  }
  const reservationId = crypto.randomUUID();
  const reservationWire = JSON.stringify({
    schema: 1,
    family: 'private_send_reservation',
    owner,
    id: reservationId,
    revision: 0,
    peer: foreign.owner,
    rumorHash: 'b'.repeat(64),
    createdAt: 1700000000
  });
  const reservationRepository = createPrivateSendReservationRepository(
    database,
    owner
  );
  const originalIdentity = identityViewSession(identity);
  const ownership =
    originalIdentity && identityLocalOwnership(originalIdentity);
  if (!reservationRepository || !ownership)
    throw Error('original outbox scope absent');
  if (
    mode !== 'outbox' &&
    !(
      await reservePrivateSendReservation(
        reservationRepository,
        reservationWire,
        1700000000,
        ownership.current
      )
    ).ok
  )
    throw Error('original protected outbox absent');
  const settledIds = Array.from<string>([]);
  function pairedRecord(index: number): PrivateSendOperation {
    const id = crypto.randomUUID(),
      rumorTemplate = {
        pubkey: owner,
        kind: 14,
        created_at: 1700000000 + index,
        tags: [['p', foreign.owner]],
        content: 'CR004_TEST_ONLY_nested_capacity_' + index
      },
      rumor = { ...rumorTemplate, id: getEventHash(rumorTemplate) };
    const artifact = (destination: string) => {
      const seal = signer.sign({
        kind: 13,
        created_at: 1700000000 + index,
        tags: [],
        content: signer.encrypt(destination, JSON.stringify(rumor))
      });
      const wrapper = crypto.getRandomValues(new Uint8Array(32)),
        conversation = nip44.v2.utils.getConversationKey(wrapper, destination);
      try {
        const outer = finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000 + index,
            tags: [['p', destination]],
            content: nip44.v2.encrypt(JSON.stringify(seal), conversation)
          },
          wrapper
        );
        return { eventId: outer.id, wire: JSON.stringify(outer) };
      } finally {
        wrapper.fill(0);
        conversation.fill(0);
      }
    };
    const self = artifact(owner),
      peerArtifact = artifact(foreign.owner);
    const route = (
      role: 'peer' | 'self_archive',
      author: string,
      origin: string
    ) => ({
      role,
      author,
      targets: [origin],
      knownBase: { author, id: 'c'.repeat(64), createdAt: 100 },
      sources: [
        {
          source: 'wss://discovery.example.org',
          state: 'eose' as const,
          head: { id: 'c'.repeat(64), createdAt: 100 }
        }
      ]
    });
    // Seeded local receipt bookkeeping only, not genuine remote ACK/operator Q.
    return {
      schema: 1,
      family: 'private_send_operation',
      owner,
      id,
      revision: 1,
      peer: foreign.owner,
      rumorHash: rumor.id,
      createdAt: rumor.created_at,
      self,
      peerArtifact,
      deliveryPlan: {
        state: 'prepared',
        routes: {
          peer: route('peer', foreign.owner, 'wss://peer.example.org'),
          archive: route('self_archive', owner, 'wss://archive.example.org')
        }
      },
      receipts: [
        {
          actionId: crypto.randomUUID(),
          origin: 'wss://peer.example.org',
          role: 'peer',
          attempt: 1,
          eventId: peerArtifact.eventId,
          status: 'accepted',
          observedAtMilliseconds: 1700000000000,
          readbackWire: null
        },
        {
          actionId: crypto.randomUUID(),
          origin: 'wss://archive.example.org',
          role: 'self_archive',
          attempt: 1,
          eventId: self.eventId,
          status: 'accepted',
          observedAtMilliseconds: 1700000000000,
          readbackWire: null
        }
      ]
    };
  }
  if (mode === 'outbox') {
    const rows = Array.from({ length: 100 }, (_, index) => pairedRecord(index));
    for (const row of rows) {
      if (!decodePrivateRecord(JSON.stringify(row), owner, row.id).ok)
        throw Error('strict nested-pair journal fixture rejected');
      settledIds.push(row.id);
    }
    const tx = browserDatabaseTransaction(
      database,
      ['private_sends'],
      'readwrite'
    );
    for (const row of rows)
      tx.objectStore('private_sends').put({
        owner,
        id: row.id,
        wire: JSON.stringify(row)
      });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () => reject(Error('capacity seed abort')));
    });
  }
  const corruptId = crypto.randomUUID();
  async function transactionRows() {
    const tx = browserDatabaseTransaction(
      database,
      [
        'public_drafts',
        'public_operations',
        'received_envelopes',
        'private_sends'
      ],
      'readonly'
    );
    const requests = [
      'public_drafts',
      'public_operations',
      'received_envelopes',
      'private_sends'
    ].map((name) => tx.objectStore(name).getAll());
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('native observation abort'))
      );
    });
    return requests.map(
      (request) =>
        request.result as { owner: string; id: string; wire: string }[]
    );
  }
  if (mode === 'corrupt') {
    const tx = browserDatabaseTransaction(
      database,
      ['public_operations'],
      'readwrite'
    );
    tx.objectStore('public_operations').put({
      owner,
      id: corruptId,
      wire: '{"schema":99}'
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () => reject(Error('fixture seed abort')));
    });
  }
  counts = { reads: 0, signs: 0, encrypts: 0, decrypts: 0 };
  const review = createLocalDataReview(identity);
  const target = document.createElement('div');
  target.id = 'hcp125-panel';
  document.body.append(target);
  const component = mount(Panel, { target, props: { review } });
  const inspect = () => reviewLocalData(review, 'reviewed_local_inventory');
  const preview = () =>
    previewLocalDataRemoval(
      review,
      'public',
      JSON.stringify(['public_draft:' + first.value.id])
    );
  const confirm = () =>
    confirmLocalDataRemoval(review, 'reviewed_local_removal');
  return {
    owner,
    settledIds: () => settledIds.slice(),
    async reviewAndPreviewOutbox() {
      await inspect();
      return previewLocalDataRemoval(
        review,
        'outbox',
        JSON.stringify([settledIds[0]])
      );
    },
    // Actual local sender factories; controlled discovery/SDK inputs are not
    // remote delivery, an installed extension, or operator qualification.
    async freshSend() {
      const session = identityViewSession(identity);
      if (!session) throw Error('original sender identity absent');
      const food = foreign.sign({
        kind: 30402,
        created_at: Math.floor(Date.now() / 1000),
        content: 'Fresh capacity fixture carrots',
        tags: [
          ['d', 'capacity-carrots'],
          ['title', 'Carrots'],
          ['summary', 'Fresh carrots'],
          ['published_at', '1700000000'],
          ['location', 'Victoria'],
          ['price', '3.5', 'CAD'],
          ['radroots:price_unit', 'lb'],
          ['status', 'active']
        ]
      });
      const verified = verifyEnvelope(JSON.stringify(food)),
        context = verified.ok && captureEnquiryContext(verified.value),
        rumor =
          context &&
          captureEnquiryRumor(
            session,
            context,
            'CR004 fresh factory send',
            Math.floor(Date.now() / 1000)
          );
      if (!rumor) throw Error('genuine fresh rumor absent');
      const schedulers: ReturnType<typeof createPublicScheduler>[] = [];
      let preparation: ReturnType<typeof captureSelfRecoveryPreparation>;
      try {
        const reserved = await reserveSendIdentity(
          reservationRepository,
          session,
          crypto.randomUUID(),
          rumor,
          'reviewed_private_intent'
        );
        if (!('identity' in reserved)) return { reservation: reserved.status };
        const id = reservedSendSnapshot(reserved.identity)?.id;
        preparation = captureSelfRecoveryPreparation(
          priv,
          session,
          reserved.identity,
          'reviewed_self_recovery'
        );
        if (!preparation) throw Error('actual self-first factory absent');
        const self = await prepareSelfRecovery(
          preparation,
          'reviewed_self_recovery'
        );
        const discovery = 'wss://discovery.example.org',
          peerOrigin = 'wss://peer.example.org',
          archiveOrigin = 'wss://archive.example.org';
        const policy = validateRelayPolicy(
          JSON.stringify({
            schemaVersion: 1,
            public: [
              { origin: discovery, read: true, write: false, nip50: false }
            ],
            inbox: [peerOrigin, archiveOrigin].map((origin) => ({
              origin,
              read: true,
              write: true
            })),
            postingEnabled: false,
            messagingEnabled: true,
            operatorDenylist: []
          })
        );
        if (!policy) throw Error('controlled discovery policy absent');
        function resolve(signer: typeof foreign, origin: string) {
          const event = signer.sign({
              kind: 10050,
              created_at: Math.floor(Date.now() / 1000),
              tags: [['relay', origin]],
              content: ''
            }),
            scheduler = createPublicScheduler(),
            run = createPublicRun(scheduler, policy!);
          schedulers.push(scheduler);
          return createInboxResolver(
            run,
            signer.owner,
            (next) =>
              openInboxRequest(
                run,
                signer.owner,
                (sink) => {
                  sink({
                    type: 'EVENT',
                    from: discovery,
                    id: 'CR004_TEST_ONLY',
                    event
                  });
                  sink({
                    type: 'EOSE',
                    from: discovery,
                    id: 'CR004_TEST_ONLY'
                  });
                  return () => {};
                },
                next
              ),
            () => ownership?.current() === true
          );
        }
        const own = resolve(signer, archiveOrigin),
          other = resolve(foreign, peerOrigin),
          plan = createInboxRoutePlan(policy, owner, foreign.owner, own, other);
        if (!plan) throw Error('actual reviewed route plan absent');
        const pair = await preparePairedDelivery(
          preparation,
          { policy, own, other, plan },
          'reviewed_pair_preparation'
        );
        return {
          reservation: reserved.status,
          self: self.status,
          pair: pair.status,
          paired: !!preparedPairedDelivery(preparation),
          id
        };
      } finally {
        if (preparation) stopSelfRecoveryPreparation(preparation);
        stopRumorPlan(rumor);
        for (const scheduler of schedulers) closePublicScheduler(scheduler);
      }
    },
    async reserveNext() {
      const result = await reservePrivateSendReservation(
        reservationRepository,
        JSON.stringify({
          schema: 1,
          family: 'private_send_reservation',
          owner,
          id: crypto.randomUUID(),
          revision: 0,
          peer: foreign.owner,
          rumorHash: '8'.repeat(64),
          createdAt: 1700000000
        }),
        1700000000,
        ownership.current
      );
      return result.ok ? 'reserved' : result.reason;
    },
    async changeOutbox() {
      const stored = (await transactionRows())[3].find(
        (row) => row.id === settledIds[0]
      );
      if (!stored) throw Error('original outbox row absent');
      const decoded = decodePrivateRecord(stored.wire, owner, stored.id),
        record =
          decoded.ok && privateRecordSnapshot(decoded.value, owner, stored.id);
      if (
        !decoded.ok ||
        !record ||
        record.family !== 'private_send_operation' ||
        !record.receipts?.length
      )
        throw Error('original paired record absent');
      const next = decodePrivateRecord(
        JSON.stringify({
          ...record,
          revision: record.revision + 1,
          receipts: [
            ...record.receipts,
            {
              ...record.receipts[0],
              actionId: crypto.randomUUID(),
              status: 'unknown'
            }
          ]
        }),
        owner,
        record.id
      );
      if (
        !next.ok ||
        !(await commitPrivateRecord(priv, next.value, decoded.value)).ok
      )
        throw Error('actual outbox CAS change absent');
    },
    switchKey: () => {
      changedKey = true;
    },
    async confirmWhileSdkBusy() {
      const original = identityViewSession(identity);
      if (!original) throw Error('actual identity absent');
      const session = await createPrivateSession(
        original,
        'reviewed_private_session'
      );
      if (!session) throw Error('actual private session absent');
      let entered = () => {},
        release = () => {};
      const ready = new Promise<void>((resolve) => {
        entered = resolve;
      });
      const held = withIdlePrivateLocalStorage(
        session,
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
            entered();
          })
      );
      await ready;
      try {
        return await confirm();
      } finally {
        release();
        await held;
      }
    },
    async abortOutboxDelete(unknown = false) {
      const descriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'delete'
      );
      if (!descriptor) throw Error('actual delete descriptor absent');
      const nativeDelete = descriptor.value as IDBObjectStore['delete'],
        nativeAbort = Reflect.get<IDBTransaction, 'abort'>(
          IDBTransaction.prototype,
          'abort'
        );
      let deletes = 0,
        aborts = 0;
      let terminal: Promise<'abort' | 'complete'> | undefined;
      IDBObjectStore.prototype.delete = function (...args) {
        const request = Reflect.apply(nativeDelete, this, args);
        if (this.name === 'private_sends') {
          const transaction = this.transaction;
          terminal = new Promise((resolve) => {
            transaction.addEventListener('abort', () => {
              aborts++;
              resolve('abort');
            });
            transaction.addEventListener('complete', () => resolve('complete'));
          });
          if (unknown) {
            this.transaction.abort = () => {
              throw Error('controlled lost abort capability');
            };
            throw Error(
              'controlled lost delete result after actual queued request'
            );
          }
          request.addEventListener(
            'success',
            () => {
              deletes++;
              Reflect.apply(nativeAbort, this.transaction, []);
            },
            { once: true }
          );
        }
        return request;
      };
      try {
        const confirmed = await confirm(),
          nativeTerminal = terminal && (await terminal);
        return { confirmed, deletes, aborts, nativeTerminal };
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'delete', descriptor);
      }
    },
    effects: () => ({ ...counts }),
    snapshot: () => localDataReviewSnapshot(review),
    review: inspect,
    confirm,
    async reviewAndPreview() {
      await inspect();
      if (!(await preview())) throw Error('original public preview absent');
    },
    async reviewAndPreviewReceived() {
      await inspect();
      if (
        !(await previewLocalDataRemoval(
          review,
          'received',
          JSON.stringify([ids[0]])
        ))
      )
        throw Error('original received preview absent');
    },
    changeDraft: () =>
      savePublicDraft(drafts, first.value.id, 0, {
        ...form,
        title: 'new actual revision'
      }),
    async changeReceived() {
      const scan = await listPrivateReceivedRecords(priv);
      if (!scan.ok) throw Error('native scan absent');
      const handle = scan.value.find(
        (row) => privateRecordIdentity(row)?.id === ids[0]
      );
      const row = handle && privateRecordSnapshot(handle, owner, ids[0]);
      if (!handle || row?.family !== 'received_envelope')
        throw Error('native selected copy absent');
      const next = decodePrivateRecord(
        JSON.stringify({
          ...row,
          revision: row.revision + 1,
          sources: [...row.sources, 'wss://other.example.org']
        }),
        owner,
        row.id
      );
      if (!next.ok || !(await commitPrivateRecord(priv, next.value, handle)).ok)
        throw Error('real received CAS change absent');
    },
    async rows() {
      const [publicRows, operations, received, outbox] =
        await transactionRows();
      return {
        publicDrafts: publicRows.filter((row) => row.owner === owner).length,
        foreignDrafts: publicRows.filter((row) => row.owner === foreign.owner)
          .length,
        publicOperations: operations.filter((row) => row.owner === owner)
          .length,
        received: received.filter((row) => row.owner === owner).length,
        foreignReceived: received.filter((row) => row.owner === foreign.owner)
          .length,
        outbox: outbox.filter((row) => row.owner === owner).length
      };
    },
    async rawCorrupt() {
      return (await transactionRows())[1].find((row) => row.id === corruptId)
        ?.wire;
    },
    logout: () => disconnectIdentityView(identity),
    dispose: () => closeLocalDataReview(review),
    async hostileSelections() {
      const key = 'public_draft:' + first.value.id;
      const values = [
        'not JSON',
        '[]',
        JSON.stringify([key, key]),
        JSON.stringify(['public_operation:' + signer.id]),
        JSON.stringify(['public_draft:' + other.value.id])
      ];
      const results: boolean[] = [];
      for (const value of values)
        results.push(await previewLocalDataRemoval(review, 'public', value));
      return results;
    },
    async confirmWhileLocked() {
      let release = () => {},
        entered = () => {};
      const ready = new Promise<void>((resolve) => {
        entered = resolve;
      });
      const held = navigator.locks.request(
        'harvestcircle:owner:' + owner,
        async () => {
          entered();
          await new Promise<void>((resolve) => {
            release = resolve;
          });
        }
      );
      await ready;
      try {
        return await confirm();
      } finally {
        release();
        await held;
      }
    },
    async abortDuringDelete() {
      const descriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'delete'
      );
      if (!descriptor) throw Error('native delete descriptor absent');
      const nativeDelete = descriptor.value as IDBObjectStore['delete'];
      let deleteSuccesses = 0,
        nativeAborts = 0;
      let terminal: Promise<'abort' | 'complete'> | undefined;
      IDBObjectStore.prototype.delete = function (...args) {
        const request = nativeDelete.apply(this, args);
        if (this.name === 'public_drafts') {
          const transaction = this.transaction;
          terminal = new Promise((resolve) => {
            transaction.addEventListener('abort', () => {
              nativeAborts++;
              resolve('abort');
            });
            transaction.addEventListener('complete', () => resolve('complete'));
          });
          request.addEventListener('success', () => {
            deleteSuccesses++;
            disconnectIdentityView(identity);
          });
        }
        return request;
      };
      try {
        const confirmed = await confirm();
        const nativeTerminal = terminal && (await terminal);
        return { confirmed, deleteSuccesses, nativeAborts, nativeTerminal };
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'delete', descriptor);
      }
    },
    async oldConfirmationThenNewPreview() {
      await inspect();
      if (
        !(await previewLocalDataRemoval(
          review,
          'received',
          JSON.stringify([ids[0]])
        ))
      )
        throw Error('old actual selection absent');
      const native = navigator.locks.request.bind(navigator.locks);
      let release = () => {},
        markHeld = () => {},
        held = false;
      const reached = new Promise<void>((resolve) => {
        markHeld = resolve;
      });
      const pause = new Promise<void>((resolve) => {
        release = resolve;
      });
      const originalMethod = Reflect.get<LockManager, 'request'>(
        navigator.locks,
        'request'
      );
      navigator.locks.request = (
        name: string,
        options: LockOptions | LockGrantedCallback<unknown>,
        callback?: LockGrantedCallback<unknown>
      ) => {
        if (typeof options === 'function') return native(name, options);
        if (!callback) throw Error('real callback absent');
        return native(name, options, async (lock) => {
          const result: unknown = await callback(lock);
          if (!held && name === 'harvestcircle:owner:' + owner && lock) {
            held = true;
            markHeld();
            await pause;
          }
          return result;
        });
      };
      let old: Promise<boolean> | undefined;
      try {
        old = confirm();
        await reached;
        disconnectIdentityView(identity);
        await connectIdentityView(identity);
        await probeIdentityViewMessaging(identity, 'reviewed_self_copy');
        await inspect();
        const newer = await previewLocalDataRemoval(
          review,
          'received',
          JSON.stringify([ids[1]])
        );
        release();
        const oldConfirmed = await old;
        const currentPreview = localDataReviewSnapshot(review);
        const newConfirmed = await confirm();
        return { held, newer, oldConfirmed, currentPreview, newConfirmed };
      } finally {
        release();
        if (old) await old;
        navigator.locks.request = originalMethod;
      }
    },
    async close() {
      closeLocalDataReview(review);
      await unmount(component);
      target.remove();
      closeIdentityView(identity);
      closeBrowserDatabase(database);
      signer.close();
      foreign.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
}
