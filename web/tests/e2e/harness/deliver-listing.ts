import { deliverListing } from '../../../src/lib/publishing/deliver-listing.ts';
import {
  getPublicPool,
  closePublicPool
} from '../../../src/lib/nostr/public-pool.ts';
import {
  authorizeListingAttempt,
  withListingDeliveryOwnership,
  takeListingPublication
} from '../../../src/lib/nostr/listing-publisher.ts';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import {
  getPublicStore,
  insertPublicEnvelope,
  publicStoreEnvelope
} from '../../../src/lib/nostr/public-store.ts';
export { deliverListing, authorizeListingAttempt, takeListingPublication };
import { makeFixture } from './approved-signing.ts';
import { EventStore } from 'applesauce-core';
import { getSeenRelays, type NostrEvent } from 'applesauce-core/helpers';
import { fixturePolicy, discovery, inbox } from './inbox-setup.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import { captureNewListingReadiness } from '../../../src/lib/publishing/new-listing-readiness.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  readPublicListingIntent
} from '../../../src/lib/persistence/public-publications.ts';
import {
  createPublicDraftController,
  closePublicDraftController
} from '../../../src/lib/publishing/draft-controller.ts';
import {
  reviewNewListing,
  reserveNewListing,
  listingReviewSnapshot
} from '../../../src/lib/publishing/prepare-listing.ts';
import { publicRecordSnapshot } from '../../../src/lib/persistence/records.ts';
import {
  beginListingSigning,
  runListingSigning,
  listingPublicationSnapshot
} from '../../../src/lib/publishing/sign-listing.ts';

export async function createFixture() {
  const trace: string[] = [];
  const tracePut = Object.getOwnPropertyDescriptor(
      IDBObjectStore.prototype,
      'put'
    ),
    traceSend = Object.getOwnPropertyDescriptor(WebSocket.prototype, 'send');
  if (!tracePut) throw Error('native trace descriptors missing');
  const nativeTracePut = tracePut.value as IDBObjectStore['put'],
    nativeTraceSend = Reflect.get<WebSocket, 'send'>(
      WebSocket.prototype,
      'send'
    );
  Object.defineProperty(IDBObjectStore.prototype, 'put', {
    ...tracePut,
    value: function (
      this: IDBObjectStore,
      ...args: Parameters<IDBObjectStore['put']>
    ) {
      const value: unknown = args[0];
      if (
        this.name === 'public_operations' &&
        typeof value === 'object' &&
        value &&
        'wire' in value &&
        typeof value.wire === 'string'
      ) {
        const revision = (JSON.parse(value.wire) as { revision: number })
          .revision;
        this.transaction.addEventListener(
          'complete',
          () => trace.push('ack:' + revision),
          { once: true }
        );
      }
      return nativeTracePut.apply(this, args);
    }
  });
  Object.defineProperty(WebSocket.prototype, 'send', {
    configurable: true,
    writable: true,
    ...traceSend,
    value: function (this: WebSocket, ...args: Parameters<WebSocket['send']>) {
      if (
        typeof args[0] === 'string' &&
        (JSON.parse(args[0]) as unknown[])[0] === 'EVENT'
      )
        trace.push('EVENT');
      return nativeTraceSend.apply(this, args);
    }
  });
  const signer = makeFixture(10050),
    identity = createIdentitySession(),
    policy = fixturePolicy();
  const preference = signer.sign({
    ...signer.template,
    kind: 10050,
    created_at: 100,
    tags: [['relay', inbox]],
    content: ''
  });
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0,
    keys = 0,
    observedOwner = signer.owner;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => {
        keys++;
        return Promise.resolve(observedOwner);
      },
      signEvent: (input: Parameters<typeof signer.sign>[0]) => {
        signs++;
        return Promise.resolve(signer.sign(input));
      },
      nip44: {
        encrypt: (_peer: string, text: string) =>
          Promise.resolve('fixture:' + text),
        decrypt: (_peer: string, text: string) => Promise.resolve(text.slice(8))
      }
    }
  });
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const scheduler = createPublicScheduler({
    now: () => 0,
    schedule: () => () => {}
  });
  const run = createPublicRun(scheduler, policy);
  const resolver = createInboxResolver(
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
            id: 'fixture',
            event: preference
          });
          sink({ type: 'EOSE', from: discovery, id: 'fixture' });
          return () => {};
        },
        next
      ),
    () => true
  );
  // HC_TEST_ONLY_QUALIFICATION_PORT: original SDK owners, controlled exercised
  // access observation; no real extension/operator/relay qualification.
  const setup = createInboxSetupView({
    identity,
    policy,
    lookup: () => Promise.resolve(resolver),
    observeAccess: (context) => ({
      ...context,
      readTargets: context.readTargets.slice(),
      archiveTargets: context.archiveTargets.slice(),
      receive: 'qualified_exercised',
      archive: 'qualified_exercised',
      current: () => true
    })
  });
  if (!setup) throw Error('original setup missing');
  await checkInboxSetupView(setup);
  await unlockInboxSetupView(setup);
  const ready = captureNewListingReadiness(
    identity,
    setup,
    'reviewed_new_listing_inbox'
  );
  if (!ready) throw Error('original readiness missing');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner,
    repository = createPublicPublicationRepository(database, signer.owner);
  const drafts = repository && publicPublicationDraftRepository(repository);
  if (!repository || !drafts) throw Error('original repository missing');
  const originalRepository = repository;
  const controller = await createPublicDraftController(drafts, {
    title: 'Carrots',
    description: 'Exact signed description',
    location: 'Local farm',
    amount: '2',
    currency: 'USD',
    unit: 'kg',
    quantity: '',
    contactType: '',
    contactValue: ''
  });
  if (!controller) throw Error('original controller missing');
  const command = crypto.randomUUID();
  const reviewed = await reviewNewListing(
    repository,
    controller,
    ready,
    command,
    'reviewed_new_listing'
  );
  if (reviewed.status !== 'review') throw Error(reviewed.reason);
  const original = reviewed.review,
    preview = listingReviewSnapshot(original);
  if (!preview) throw Error('original preview missing');
  const reserved = await reserveNewListing(
    original,
    'publish_reviewed_listing'
  );
  if (reserved.status !== 'saved') throw Error(reserved.status);
  const signing = beginListingSigning(original, 'sign_reviewed_listing');
  if (!signing) throw Error('original signing missing');
  const originalSigning = signing;
  async function stored() {
    const read = await readPublicListingIntent(originalRepository, command);
    const row =
      read.status === 'found' &&
      publicRecordSnapshot(read.record, signer.owner, command);
    if (!row || row.family !== 'public_operation')
      throw Error('original stored command missing');
    return row;
  }
  function invoke() {
    return runListingSigning(originalSigning, 'sign_reviewed_listing');
  }
  const signed = await invoke();
  if (signed.status !== 'prepared') throw Error(signed.status);
  const publication = signed.publication,
    pool = getPublicPool(policy);
  if (!pool) throw Error('original public pool missing');
  return {
    owner: signer.owner,
    command,
    publication,
    pool,
    policy,
    async run(consent: unknown = 'publish_stored_listing') {
      const a = deliverListing(publication, pool, policy, consent),
        b = deliverListing(publication, pool, policy, consent);
      const result = await a,
        row = await stored();
      return {
        result,
        trace: trace.slice(),
        same: a === b,
        row,
        signs,
        keys,
        owner: signer.owner,
        expectedWire: listingPublicationSnapshot(publication)?.wire,
        originalCapture: row.capture.wire === preview.templateWire
      };
    },
    async read() {
      return await stored();
    },
    copied() {
      return deliverListing(
        { ...listingPublicationSnapshot(publication) } as typeof publication,
        pool,
        policy,
        'publish_stored_listing'
      );
    },
    async foreignTarget() {
      return await withListingDeliveryOwnership(
        publication,
        policy,
        'publish_stored_listing',
        async (lease) =>
          !!(await authorizeListingAttempt(
            lease,
            'wss://other.example.org',
            'publish'
          ))
      );
    },
    retargeted() {
      const manifest = readRelayPolicy(policy),
        changed = validateRelayPolicy(
          JSON.stringify({
            ...manifest,
            public: [
              {
                origin: 'wss://other.example.org',
                read: true,
                write: true,
                nip50: false
              }
            ]
          })
        );
      if (!changed) throw Error('changed fixture policy invalid');
      return deliverListing(
        publication,
        pool,
        changed,
        'publish_stored_listing'
      );
    },
    async cacheArtifact() {
      const row = await stored();
      const sdkStore = new EventStore();
      let seen: string[];
      try {
        sdkStore.add(JSON.parse(row.artifact!.wire) as NostrEvent, discovery);
        sdkStore.add(
          JSON.parse(row.artifact!.wire) as NostrEvent,
          'wss://other.example.org'
        );
        const merged = sdkStore.getEvent(row.artifact!.eventId);
        if (!merged) throw Error('actual SDK merge failed');
        seen = [...(getSeenRelays(merged) ?? [])].sort();
        if (seen.length !== 2)
          throw Error('actual SDK seen metadata not merged');
      } finally {
        sdkStore.dispose();
      }
      const verified = verifyEnvelope(row.artifact!.wire),
        store = getPublicStore();
      if (
        !verified.ok ||
        !store ||
        insertPublicEnvelope(store, verified.value) !== 'accepted' ||
        !publicStoreEnvelope(store, row.artifact!.eventId)
      )
        throw Error('actual shared public cache admission failed');
      return { id: row.artifact!.eventId, seen };
    },
    async receiptFault(unknown: boolean) {
      const put = Object.getOwnPropertyDescriptor(
          IDBObjectStore.prototype,
          'put'
        ),
        abort = Object.getOwnPropertyDescriptor(
          IDBTransaction.prototype,
          'abort'
        );
      if (!put || !abort) throw Error('native descriptors missing');
      const nativePut = put.value as IDBObjectStore['put'];
      let faultPuts = 0;
      Object.defineProperty(IDBObjectStore.prototype, 'put', {
        ...put,
        value: function (
          this: IDBObjectStore,
          ...args: Parameters<IDBObjectStore['put']>
        ) {
          const v: unknown = args[0];
          if (
            this.name === 'public_operations' &&
            typeof v === 'object' &&
            v &&
            'wire' in v &&
            typeof v.wire === 'string' &&
            (JSON.parse(v.wire) as { revision: number }).revision === 3
          ) {
            faultPuts++;
            if (unknown) {
              nativePut.apply(this, args);
              throw Error(
                'HC_TEST_ONLY real receipt put lost JS acknowledgement'
              );
            }
            this.transaction.abort();
            throw Error('HC_TEST_ONLY definite receipt abort');
          }
          return nativePut.apply(this, args);
        }
      });
      if (unknown)
        Object.defineProperty(IDBTransaction.prototype, 'abort', {
          ...abort,
          value() {
            throw Error('HC_TEST_ONLY unavailable abort channel');
          }
        });
      try {
        const result = await deliverListing(
            publication,
            pool,
            policy,
            'publish_stored_listing'
          ),
          row = await stored();
        return { result, row, signs, faultPuts };
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', put);
        Object.defineProperty(IDBTransaction.prototype, 'abort', abort);
      }
    },
    logout() {
      disconnectIdentity(identity);
    },
    stop() {
      closeInboxSetupView(setup);
    },
    changeKey() {
      const other = makeFixture(10050);
      observedOwner = other.owner;
      other.close();
    },
    close() {
      closePublicPool(pool);
      closePublicDraftController(controller);
      closeBrowserDatabase(database);
      closeInboxSetupView(setup);
      disconnectIdentity(identity);
      closePublicScheduler(scheduler);
      signer.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
      Object.defineProperty(IDBObjectStore.prototype, 'put', tracePut);
      if (traceSend)
        Object.defineProperty(WebSocket.prototype, 'send', traceSend);
      else Reflect.deleteProperty(WebSocket.prototype, 'send');
    }
  };
}
