export const secondOrigin = 'wss://second.example.org';
function retryPolicy(twoTargets = false, changed = false) {
  const manifest = readRelayPolicy(fixturePolicy());
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...manifest,
      public: twoTargets
        ? [
            ...manifest.public,
            {
              origin: changed ? 'wss://changed.example.org' : secondOrigin,
              read: true,
              write: true,
              nip50: false
            }
          ]
        : manifest.public
    })
  );
  if (!policy) throw Error('controlled policy invalid');
  return policy;
}
import { deliverListing } from '../../../src/lib/publishing/deliver-listing.ts';
import {
  getPublicPool,
  closePublicPool
} from '../../../src/lib/nostr/public-pool.ts';
import {
  readRelayPolicy,
  publicRelayTargets,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  beginListingRetry,
  listingRetryResult,
  stopListingRetry,
  type ListingRetry
} from '../../../src/lib/publishing/retry-listing.ts';
import {
  prepareListingResume,
  resumeListing,
  listingResumeSnapshot
} from '../../../src/lib/publishing/resume-listing.ts';
import { makeFixture } from './approved-signing.ts';
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

export async function createFixture(twoTargets = false) {
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
    policy = retryPolicy(twoTargets);
  const preference = signer.sign({
    ...signer.template,
    kind: 10050,
    created_at: 100,
    tags: [['relay', inbox]],
    content: ''
  });
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0,
    keys = 0;
  const observedOwner = signer.owner;
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
          for (const origin of publicRelayTargets(policy, 'read'))
            sink({ type: 'EOSE', from: origin, id: 'fixture' });
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
  const cleanupPool = pool;
  let control: ListingRetry | undefined;
  let closed = false;
  function close() {
    if (closed) return;
    closed = true;
    if (control) stopListingRetry(control);
    closePublicPool(cleanupPool);
    if (controller) closePublicDraftController(controller);
    closeBrowserDatabase(database);
    closeInboxSetupView(setup);
    disconnectIdentity(identity);
    closePublicScheduler(scheduler);
    signer.close();
    if (prior) Object.defineProperty(window, 'nostr', prior);
    else Reflect.deleteProperty(window, 'nostr');
    if (tracePut)
      Object.defineProperty(IDBObjectStore.prototype, 'put', tracePut);
    else Reflect.deleteProperty(IDBObjectStore.prototype, 'put');
    if (traceSend)
      Object.defineProperty(WebSocket.prototype, 'send', traceSend);
    else Reflect.deleteProperty(WebSocket.prototype, 'send');
  }
  return {
    owner: signer.owner,
    command,
    policy,
    async details() {
      const row = await stored();
      return {
        row,
        signs,
        keys,
        trace: trace.slice(),
        wire: listingPublicationSnapshot(publication)?.wire
      };
    },
    start() {
      control = beginListingRetry(
        publication,
        pool,
        policy,
        'retry_stored_listing'
      );
      return !!control;
    },
    async result() {
      if (!control) throw Error('original retry missing');
      return await listingRetryResult(control);
    },
    stop() {
      if (control) stopListingRetry(control);
    },
    copiedStop() {
      if (control) stopListingRetry({ ...control });
    },
    async initial() {
      return await deliverListing(
        publication,
        pool,
        policy,
        'publish_stored_listing'
      );
    },
    close
  };
}
export async function openResumeFixture(
  owner: string,
  command: string,
  twoTargets = false,
  changed = false
) {
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let observedOwner = owner,
    signs = 0,
    keys = 0,
    heldKeys = 0,
    completed = false;
  let keyHold: Promise<string> | undefined, release: (() => void) | undefined;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey() {
        keys++;
        if (keyHold) heldKeys++;
        return keyHold ?? Promise.resolve(observedOwner);
      },
      signEvent() {
        signs++;
        return Promise.reject(
          Error(
            'HC_TEST_ONLY resumed artifact must never request another signature'
          )
        );
      },
      nip44: {
        encrypt: (_peer: string, text: string) =>
          Promise.resolve('fixture:' + text),
        decrypt: (_peer: string, text: string) => Promise.resolve(text.slice(8))
      }
    }
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner,
    repository = createPublicPublicationRepository(database, owner),
    policy = retryPolicy(twoTargets, changed),
    pool = getPublicPool(policy);
  if (!repository || !pool) throw Error('actual reopened custody missing');
  const originalRepository = repository,
    originalPool = pool;
  let heldReadAcknowledgements = 0;
  let releaseRead = () => {};
  let restoreReadListener = () => {};
  function holdNextNativeReadAcknowledgement() {
    const prototype = IDBTransaction.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(
      prototype,
      'addEventListener'
    );
    const nativeDescriptor =
      descriptor ??
      Object.getOwnPropertyDescriptor(
        EventTarget.prototype,
        'addEventListener'
      );
    if (!nativeDescriptor)
      throw Error('native event registration descriptor missing');
    const native = nativeDescriptor.value as IDBTransaction['addEventListener'];
    restoreReadListener = () => {
      if (descriptor)
        Object.defineProperty(prototype, 'addEventListener', descriptor);
      else Reflect.deleteProperty(prototype, 'addEventListener');
    };
    prototype.addEventListener = function (
      type: string,
      listener: EventListenerOrEventListenerObject,
      options?: boolean | AddEventListenerOptions
    ) {
      if (
        type !== 'complete' ||
        !listener ||
        this.mode !== 'readonly' ||
        !this.objectStoreNames.contains('public_operations')
      )
        return native.call(this, type, listener, options);
      restoreReadListener();
      return native.call(
        this,
        type,
        function (event: Event) {
          const transaction = event.currentTarget;
          if (!(transaction instanceof IDBTransaction))
            throw Error('native completion target missing');
          heldReadAcknowledgements++;
          let released = false;
          releaseRead = () => {
            if (released) return;
            released = true;
            if (typeof listener === 'function')
              listener.call(transaction, event);
            else listener.handleEvent(event);
          };
        },
        options
      );
    };
  }
  let control: ListingRetry | undefined;
  async function read() {
    const got = await readPublicListingIntent(originalRepository, command);
    const row =
      got.status === 'found' &&
      publicRecordSnapshot(got.record, owner, command);
    if (!row || row.family !== 'public_operation')
      throw Error('reopened original operation missing');
    return row;
  }
  return {
    async start(
      consent: unknown = 'resume_stored_listing',
      holdAfterPrepare = false,
      holdReadAfterPrepare = false
    ) {
      const prepared = await prepareListingResume(
        originalRepository,
        identity,
        policy,
        command,
        'reviewed_stored_listing_resume'
      );
      if (prepared.status !== 'ready') return prepared.status;
      if (holdReadAfterPrepare) holdNextNativeReadAcknowledgement();
      if (holdAfterPrepare)
        keyHold = new Promise<string>((resolve) => {
          release = () => resolve(observedOwner);
        });
      control = resumeListing(prepared.resume, originalPool, consent);
      if (control) {
        completed = false;
        void listingRetryResult(control).then(() => {
          completed = true;
        });
      }
      return control ? 'started' : 'invalid';
    },
    async preview() {
      const p = await prepareListingResume(
        originalRepository,
        identity,
        policy,
        command,
        'reviewed_stored_listing_resume'
      );
      return p.status === 'ready' ? listingResumeSnapshot(p.resume) : p;
    },
    async result() {
      if (!control) throw Error('original resume missing');
      return await listingRetryResult(control);
    },
    stop() {
      if (control) stopListingRetry(control);
    },
    copiedStop() {
      if (control) stopListingRetry({ ...control });
    },
    holdKey() {
      keyHold = new Promise<string>((resolve) => {
        release = () => resolve(observedOwner);
      });
    },
    releaseKey() {
      release?.();
      keyHold = undefined;
    },
    heldReadCount() {
      return heldReadAcknowledgements;
    },
    releaseRead() {
      releaseRead();
      restoreReadListener();
    },
    changeKey() {
      const other = makeFixture(10050);
      observedOwner = other.owner;
      other.close();
    },
    logout() {
      disconnectIdentity(identity);
    },
    async details() {
      return { row: await read(), signs, keys, heldKeys, completed };
    },
    close() {
      if (control) stopListingRetry(control);
      release?.();
      releaseRead();
      restoreReadListener();
      closePublicPool(originalPool);
      closeBrowserDatabase(database);
      disconnectIdentity(identity);
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
}
