import {
  reviewNewListing,
  listingReviewSnapshot,
  reserveNewListing
} from '../../../src/lib/publishing/prepare-listing.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  readPublicListingIntent
} from '../../../src/lib/persistence/public-publications.ts';
import { captureNewListingReadiness } from '../../../src/lib/publishing/new-listing-readiness.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  stopInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicDraftController,
  editPublicDraft,
  flushPublicDraft,
  publicDraftControllerSnapshot,
  closePublicDraftController
} from '../../../src/lib/publishing/draft-controller.ts';
import { publicRecordSnapshot } from '../../../src/lib/persistence/records.ts';
import { inspectLocalTemplate } from '../../../src/lib/nostr/local-template.ts';
import { makeSetupFixture, fixturePolicy, inbox } from './inbox-setup.ts';
export async function createFixture() {
  const f = await makeSetupFixture([['relay', inbox]]);
  // HC_TEST_ONLY_QUALIFICATION_PORT: genuine original owners with exercised
  // controlled observations, never actual operator/relay qualification.
  const setup = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
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
    f.identity,
    setup,
    'reviewed_new_listing_inbox'
  );
  if (!ready) throw Error('original readiness missing');
  const originalReady = ready;
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner;
  const repository = createPublicPublicationRepository(database, f.owner);
  const drafts = repository && publicPublicationDraftRepository(repository);
  if (!repository || !drafts)
    throw Error('original publication repository missing');
  const originalRepository = repository;
  const form = {
    title: 'Carrots',
    description: 'First complete description',
    location: 'Local farm',
    amount: '2',
    currency: 'USD',
    unit: 'kg',
    quantity: '',
    contactType: '' as const,
    contactValue: ''
  };
  const controller = await createPublicDraftController(drafts, form);
  if (!controller) throw Error('original controller missing');
  const originalController = controller;
  const command = '3d030bf5-901d-45e1-8251-41cbdf805e96';
  async function review() {
    const result = await reviewNewListing(
      originalRepository,
      originalController,
      originalReady,
      command,
      'reviewed_new_listing'
    );
    if (result.status !== 'review') throw Error(result.reason);
    return result.review;
  }
  function operations(): Promise<number> {
    return new Promise((resolve, reject) => {
      const tx = browserDatabaseTransaction(
        database,
        ['public_operations'],
        'readonly'
      );
      const count = tx
        .objectStore('public_operations')
        .index('by_owner')
        .count(IDBKeyRange.only(f.owner));
      tx.addEventListener('complete', () => resolve(count.result));
      tx.addEventListener('abort', () => reject(Error('actual count aborted')));
    });
  }
  return {
    async unknown(mode: 'exact' | 'unavailable' | 'mismatch') {
      const original = await review(),
        preview = listingReviewSnapshot(original);
      if (!preview) throw Error('original preview missing');
      const put = Object.getOwnPropertyDescriptor(
          IDBObjectStore.prototype,
          'put'
        ),
        abort = Object.getOwnPropertyDescriptor(
          IDBTransaction.prototype,
          'abort'
        ),
        get = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'get');
      if (!put || !abort || !get) throw Error('native descriptors missing');
      const nativePut = put.value as IDBObjectStore['put'],
        nativeGet = get.value as IDBObjectStore['get'];
      let nativePuts = 0;
      const different = JSON.parse(preview.templateWire) as {
        pubkey: string;
        kind: number;
        created_at: number;
        tags: string[][];
        content: string;
      };
      different.tags = different.tags.map((tag) =>
        tag[0] === 'd' ? ['d', 'f04a1fd7-e3c7-4e75-8949-4fdc8a6b2246'] : tag
      );
      const wire = JSON.stringify(different),
        template = inspectLocalTemplate(wire, f.owner, 30402);
      if (!template) throw Error('actual changed template invalid');
      const changed = {
        schema: 1,
        family: 'public_operation',
        owner: f.owner,
        id: command,
        revision: 0,
        source: {
          type: 'draft',
          id: preview.draftId,
          revision: preview.draftRevision
        },
        capture: {
          kind: 30402,
          wire,
          hash: template.hash,
          targets: preview.targets,
          policyFingerprint: preview.policyFingerprint
        },
        artifact: null,
        receipts: []
      };
      Object.defineProperty(IDBObjectStore.prototype, 'put', {
        ...put,
        value: function (
          this: IDBObjectStore,
          ...args: Parameters<IDBObjectStore['put']>
        ) {
          const request = nativePut.apply(this, args);
          if (this.name === 'public_operations') nativePuts++;
          if (this.name === 'public_operations' && nativePuts === 1) {
            if (mode === 'mismatch')
              request.addEventListener('success', () => {
                nativePut.call(this, {
                  owner: f.owner,
                  id: command,
                  wire: JSON.stringify(changed)
                });
                nativePuts++;
              });
            throw Error(
              'HC_TEST_ONLY lost acknowledgement after actual native put'
            );
          }
          return request;
        }
      });
      Object.defineProperty(IDBTransaction.prototype, 'abort', {
        ...abort,
        value() {
          throw Error('HC_TEST_ONLY unavailable abort channel');
        }
      });
      if (mode === 'unavailable')
        Object.defineProperty(IDBObjectStore.prototype, 'get', {
          ...get,
          value: function (
            this: IDBObjectStore,
            ...args: Parameters<IDBObjectStore['get']>
          ) {
            if (this.name === 'public_operations')
              throw new DOMException(
                'HC_TEST_ONLY read setup unavailable',
                'InvalidStateError'
              );
            return nativeGet.apply(this, args);
          }
        });
      let result;
      try {
        result = await reserveNewListing(original, 'publish_reviewed_listing');
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', put);
        Object.defineProperty(IDBTransaction.prototype, 'abort', abort);
        Object.defineProperty(IDBObjectStore.prototype, 'get', get);
      }
      const observed = await readPublicListingIntent(
        originalRepository,
        command
      );
      const row =
        observed.status === 'found'
          ? publicRecordSnapshot(observed.record, f.owner, command)
          : undefined;
      Object.defineProperty(IDBObjectStore.prototype, 'put', {
        ...put,
        value: function (
          this: IDBObjectStore,
          ...args: Parameters<IDBObjectStore['put']>
        ) {
          if (this.name === 'public_operations') nativePuts++;
          return nativePut.apply(this, args);
        }
      });
      let retry;
      try {
        retry = await reserveNewListing(original, 'publish_reviewed_listing');
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', put);
      }
      return {
        status: result.status,
        nativePuts,
        originalCapture:
          row?.family === 'public_operation' &&
          row.source.type === 'draft' &&
          row.source.id === preview.draftId &&
          row.source.revision === preview.draftRevision &&
          row.capture.wire === preview.templateWire &&
          row.capture.hash === preview.expectedHash &&
          JSON.stringify(row.capture.targets) ===
            JSON.stringify(preview.targets) &&
          row.capture.policyFingerprint === preview.policyFingerprint &&
          listingReviewSnapshot(original)?.identifier === preview.identifier,
        operations: await operations(),
        retry: retry.status,
        signatures: f.counts().signs
      };
    },
    async failedRead() {
      const descriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'get'
      );
      if (!descriptor) throw Error('native get descriptor missing');
      Object.defineProperty(IDBObjectStore.prototype, 'get', {
        ...descriptor,
        value() {
          throw new DOMException(
            'HC_TEST_ONLY native read setup failure',
            'InvalidStateError'
          );
        }
      });
      let status: string;
      try {
        status = (await readPublicListingIntent(originalRepository, command))
          .status;
      } catch {
        status = 'rejected';
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'get', descriptor);
      }
      return {
        status,
        operations: await operations(),
        signatures: f.counts().signs
      };
    },
    async concurrentReviews() {
      const [first, second] = await Promise.all([review(), review()]);
      const firstPreview = listingReviewSnapshot(first),
        secondPreview = listingReviewSnapshot(second);
      const firstSave = await reserveNewListing(
        first,
        'publish_reviewed_listing'
      );
      const secondSave = await reserveNewListing(
        second,
        'publish_reviewed_listing'
      );
      return {
        sameReview: first === second,
        sameTemplate:
          firstPreview?.templateWire === secondPreview?.templateWire,
        first: firstSave.status,
        second: secondSave.status,
        operations: await operations(),
        signatures: f.counts().signs
      };
    },
    async doubleSubmit() {
      const original = await review(),
        preview = listingReviewSnapshot(original);
      if (!preview) throw Error('preview missing');
      const first = reserveNewListing(original, 'publish_reviewed_listing'),
        second = reserveNewListing(original, 'publish_reviewed_listing');
      const samePromise = first === second;
      const results = await Promise.all([first, second]);
      if (results[0].status !== 'saved' || results[1].status !== 'saved')
        throw Error('original double submit did not save');
      const repeatedReview = await review();
      const repeatedPreview = listingReviewSnapshot(repeatedReview);
      const repeated = await reserveNewListing(
        repeatedReview,
        'publish_reviewed_listing'
      );
      editPublicDraft(controller, {
        ...form,
        description: 'Changed future description'
      });
      await flushPublicDraft(controller);
      const retry = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      const stored =
        retry.status === 'existing'
          ? publicRecordSnapshot(retry.record, f.owner, command)
          : undefined;
      return {
        same: samePromise && results[0].record === results[1].record,
        repeatReview: repeated.status,
        repeatCaptureUnchanged:
          repeatedPreview?.identifier === preview.identifier &&
          repeatedPreview?.createdAtSeconds === preview.createdAtSeconds &&
          repeatedPreview?.expectedHash === preview.expectedHash,
        operations: await operations(),
        futureCaptureUnchanged:
          stored?.family === 'public_operation' &&
          stored.capture.wire === preview.templateWire &&
          listingReviewSnapshot(original)?.templateWire ===
            preview.templateWire,
        retry: retry.status,
        signatures: f.counts().signs
      };
    },
    async commandConflict() {
      const original = await review(),
        before = listingReviewSnapshot(original);
      if (!before) throw Error('preview missing');
      const saved = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      if (saved.status !== 'saved') throw Error('original save missing');
      editPublicDraft(controller, { ...form, title: 'Different carrots' });
      const other = await reviewNewListing(
        originalRepository,
        originalController,
        originalReady,
        command,
        'reviewed_new_listing'
      );
      const conflict =
        other.status === 'blocked'
          ? { status: other.reason }
          : await reserveNewListing(other.review, 'publish_reviewed_listing');
      return {
        conflict: conflict.status,
        identifierRandom:
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
            before.identifier
          ) && !before.identifier.includes(form.title),
        identifierStable:
          listingReviewSnapshot(original)?.identifier === before.identifier,
        operations: await operations(),
        signatures: f.counts().signs
      };
    },
    async exactRevision() {
      await flushPublicDraft(controller);
      editPublicDraft(controller, {
        ...form,
        description: 'Second complete description'
      });
      const original = await review(),
        preview = listingReviewSnapshot(original),
        savedDraft = publicDraftControllerSnapshot(controller)?.draft;
      if (!preview || !savedDraft) throw Error('exact draft missing');
      const targets = preview.targets.slice();
      (preview.targets as string[]).push('wss://unapproved.foodtrace.net');
      const result = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      if (result.status !== 'saved') throw Error('exact reservation failed');
      const row = publicRecordSnapshot(result.record, f.owner, command);
      return {
        exact:
          row?.family === 'public_operation' &&
          row.source.type === 'draft' &&
          row.source.id === savedDraft.id &&
          row.source.revision === savedDraft.revision &&
          savedDraft.revision === 1 &&
          row.capture.hash === preview.expectedHash &&
          row.owner === f.owner &&
          row.capture.wire === preview.templateWire,
        description: savedDraft.form.description,
        targetsFrozen:
          row?.family === 'public_operation' &&
          JSON.stringify(row.capture.targets) === JSON.stringify(targets),
        signatures: f.counts().signs
      };
    },
    async stale() {
      const original = await review(),
        saved = publicDraftControllerSnapshot(controller)?.draft;
      if (!saved) throw Error('saved draft missing');
      const other = await createPublicDraftController(drafts, form, saved.id);
      if (!other) throw Error('other original controller missing');
      editPublicDraft(other, {
        ...form,
        description: 'Actual other tab revision'
      });
      await flushPublicDraft(other);
      const changedDraft = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      closePublicDraftController(other);
      stopInboxSetupView(setup);
      const retired = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      return {
        changedDraft: changedDraft.status,
        retired: retired.status,
        operations: await operations(),
        signatures: f.counts().signs
      };
    },
    async abort() {
      const original = await review(),
        preview = listingReviewSnapshot(original);
      const descriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      );
      if (!descriptor) throw Error('native put descriptor missing');
      const nativePut = descriptor.value as IDBObjectStore['put'];
      let armed = true;
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<typeof nativePut>
      ) {
        const request = nativePut.apply(this, args);
        if (armed && this.name === 'public_operations') {
          armed = false;
          request.addEventListener('success', () => this.transaction.abort());
        }
        return request;
      };
      let result;
      try {
        result = await reserveNewListing(original, 'publish_reviewed_listing');
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', descriptor);
      }
      const beforeRetry = await operations();
      const retry = await reserveNewListing(
        original,
        'publish_reviewed_listing'
      );
      const row =
        retry.status === 'saved'
          ? publicRecordSnapshot(retry.record, f.owner, command)
          : undefined;
      return {
        aborted: result.status,
        beforeRetry,
        retry: retry.status,
        operations: await operations(),
        originalCapture:
          row?.family === 'public_operation' &&
          row.capture.wire === preview?.templateWire &&
          row.capture.hash === preview?.expectedHash,
        signatures: f.counts().signs
      };
    },
    close() {
      closePublicDraftController(controller);
      closeInboxSetupView(setup);
      closeBrowserDatabase(database);
      f.close();
    }
  };
}
