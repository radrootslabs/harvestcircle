// HC_TEST_ONLY_RELAY: actual owner/IndexedDB/public SDK over loopback, no live Q.
import { mount, unmount } from 'svelte';
import Panel from './public-hides-panel.svelte';
import { makeFixture } from './approved-signing.ts';
import { createFixture as publicationFixture } from './listing-operation-view.ts';
import {
  openSellingView,
  sellingViewSnapshot,
  closeSellingView
} from '../../../src/lib/publishing/selling-view.ts';
import { fixturePolicy } from './inbox-setup.ts';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  closeIdentityView
} from '../../../src/lib/runtime/view-context.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  bindPublicRuntimeHides,
  createPublicView,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  createLocalPublicHides,
  mountLocalPublicHides,
  refreshLocalPublicHides,
  localPublicHidesSnapshot,
  hideLocalPublicPublisher,
  undoLocalPublicPublisher,
  closeLocalPublicHides,
  LOCAL_PUBLIC_HIDES_CONTEXT
} from '../../../src/lib/persistence/local-public-hides.ts';
import {
  createProductView,
  productViewSnapshot,
  productViewPublisherProof,
  closeProductView,
  type ProductView
} from '../../../src/lib/catalog/product-view.ts';
import {
  createSearchView,
  startSearchView,
  searchViewSnapshot,
  closeSearchView,
  type SearchView
} from '../../../src/lib/catalog/search-view.ts';
import { buildFoodTemplate } from '../../../src/lib/contracts/food-availability-v1/write.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import {
  verifyEnvelope,
  type VerifiedEnvelope
} from '../../../src/lib/nostr/verified-envelope.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';

export async function createFixture(denied = false, retained = false) {
  const signer = makeFixture(),
    second = makeFixture();
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  const original = retained ? await publicationFixture() : undefined;
  if (original && (await original.signed()) !== 'prepared')
    throw Error('original native signed operation missing');
  const retainedRow = await original?.row();
  const owner = original?.owner ?? signer.owner;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: { getPublicKey: () => Promise.resolve(owner) }
  });
  const context = createIdentityViewContext();
  mountIdentityView(context);
  await connectIdentityView(context);
  const hides = createLocalPublicHides(context);
  await mountLocalPublicHides(hides);
  const publicContext = createPublicRuntimeContext();
  const manifest = readRelayPolicy(fixturePolicy());
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...manifest,
      operatorDenylist: denied ? [signer.owner] : []
    })
  );
  if (!policy) throw Error('fixture policy missing');
  const runtime = mountPublicRuntime(publicContext, policy);
  if (!runtime) throw Error('original public runtime missing');
  bindPublicRuntimeHides(runtime, hides);
  const now = Math.floor(Date.now() / 1000);
  const built = buildFoodTemplate({
    identifier: 'visible-carrots',
    title: 'Visible carrots',
    description: 'Public plain text only',
    location: 'Farm',
    amount: '3',
    currency: 'CAD',
    unit: 'each',
    created_at: now,
    published_at: now,
    status: 'active'
  });
  if (!built.ok) throw Error('fixture food invalid');
  const event = retainedRow?.artifact
    ? (JSON.parse(retainedRow.artifact.wire) as ReturnType<typeof signer.sign>)
    : signer.sign({
        kind: 30402,
        created_at: now,
        tags: built.wire_parts.tags.map((tag) => [...tag]),
        content: built.wire_parts.content
      });
  const reference = encodeProductReference({
    kind: 30402,
    pubkey: event.pubkey,
    identifier: event.tags.find((tag) => tag[0] === 'd')?.[1] ?? ''
  });
  if (!reference) throw Error('fixture reference missing');
  const proof = verifyEnvelope(JSON.stringify(event));
  if (!proof.ok) throw Error('actual verified publisher missing');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner;
  const listeners = new Map<symbol, () => void>();
  let product: ProductView | undefined, search: SearchView | undefined;
  let panel: ReturnType<typeof mount> | undefined,
    target: HTMLElement | undefined;
  const notify = () => {
    for (const listener of listeners.values()) listener();
  };
  const fixture = {
    events: [event],
    view: () => product,
    publicSnapshot: () => product && productViewSnapshot(product),
    searchSnapshot: () => search && searchViewSnapshot(search),
    hidesSnapshot: () => localPublicHidesSnapshot(hides),
    subscribe(listener: () => void) {
      const id = Symbol();
      listeners.set(id, listener);
      return () => {
        listeners.delete(id);
      };
    },
    read() {
      if (product) closeProductView(product);
      product = createProductView(
        createPublicView(runtime),
        reference,
        { nowSeconds: () => Math.floor(Date.now() / 1000) },
        notify
      );
      notify();
    },
    search() {
      if (search) closeSearchView(search);
      search = createSearchView(
        createPublicView(runtime),
        { nowSeconds: () => Math.floor(Date.now() / 1000) },
        notify
      );
      startSearchView(search, 'carrots');
    },
    async render() {
      if (!product) fixture.read();
      if (panel) await unmount(panel);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp122-panel';
      document.body.append(target);
      panel = mount(Panel, {
        target,
        props: { fixture },
        context: new Map([[LOCAL_PUBLIC_HIDES_CONTEXT, hides]])
      });
    },
    hide() {
      return hideLocalPublicPublisher(
        hides,
        product
          ? (productViewPublisherProof(product) ?? proof.value)
          : proof.value,
        'reviewed_local_public_hide'
      );
    },
    fake() {
      return hideLocalPublicPublisher(
        hides,
        { pubkey: event.pubkey } as unknown as VerifiedEnvelope,
        'reviewed_local_public_hide'
      );
    },
    refresh() {
      return refreshLocalPublicHides(hides);
    },
    async concurrentPreference(hidden: boolean) {
      const other = createLocalPublicHides(context);
      await mountLocalPublicHides(other);
      try {
        const otherEvent = second.sign({
          kind: event.kind,
          created_at: event.created_at,
          tags: event.tags.map((tag) => [...tag]),
          content: event.content
        });
        const otherProof = verifyEnvelope(JSON.stringify(otherEvent));
        if (!otherProof.ok) throw Error('other genuine publisher missing');
        const unrelated = await hideLocalPublicPublisher(
          other,
          otherProof.value,
          'reviewed_local_public_hide'
        );
        if (unrelated.status !== 'saved') throw Error('other hide missing');
        const changed = hidden
          ? await hideLocalPublicPublisher(
              other,
              proof.value,
              'reviewed_local_public_hide'
            )
          : await undoLocalPublicPublisher(
              other,
              event.pubkey,
              'reviewed_local_public_undo'
            );
        if (changed.status !== 'saved')
          throw Error('concurrent preference missing');
        return {
          snapshot: localPublicHidesSnapshot(other),
          rows: await fixture.rows()
        };
      } finally {
        closeLocalPublicHides(other);
      }
    },
    undo() {
      return undoLocalPublicPublisher(
        hides,
        event.pubkey,
        'reviewed_local_public_undo'
      );
    },
    async rows() {
      const tx = browserDatabaseTransaction(
        database,
        ['local_preferences'],
        'readonly'
      );
      return new Promise<unknown[]>((resolve, reject) => {
        const request = tx
          .objectStore('local_preferences')
          .index('by_owner')
          .getAll(owner);
        tx.addEventListener('complete', () =>
          resolve(request.result as unknown[])
        );
        tx.addEventListener('abort', () =>
          reject(Error('native read aborted'))
        );
      });
    },
    async corrupt() {
      const tx = browserDatabaseTransaction(
        database,
        ['local_preferences'],
        'readwrite'
      );
      tx.objectStore('local_preferences').put({
        owner: signer.owner,
        id: 'public_hides',
        wire: 'corrupt local metadata'
      });
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('native write aborted'))
        );
      });
    },
    nativeRecord() {
      return original?.row();
    },
    async sellingSnapshot() {
      const view = await openSellingView(context, runtime);
      if (!view) throw Error('original owner selling view missing');
      try {
        const snapshot = sellingViewSnapshot(view);
        return (
          snapshot && {
            ...snapshot,
            attention: snapshot.attention.map((row) => ({
              ...row,
              operations: row.operations.map(() => 'original_operation_view')
            }))
          }
        );
      } finally {
        closeSellingView(view);
      }
    },
    async abortWrite() {
      const native = Reflect.get<IDBDatabase, 'transaction'>(
        IDBDatabase.prototype,
        'transaction'
      );
      let injected = false;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<typeof native>
      ) {
        const tx = native.apply(this, args);
        if (
          !injected &&
          this.name === 'harvestcircle_browser' &&
          args[1] === 'readwrite'
        ) {
          injected = true;
          tx.abort();
        }
        return tx;
      };
      try {
        return await fixture.hide();
      } finally {
        IDBDatabase.prototype.transaction = native;
      }
    },
    async conflictWrite() {
      const native = Reflect.get<IDBDatabase, 'transaction'>(
        IDBDatabase.prototype,
        'transaction'
      );
      let injected = false;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<typeof native>
      ) {
        if (
          !injected &&
          this.name === 'harvestcircle_browser' &&
          args[1] === 'readwrite'
        ) {
          injected = true;
          const concurrent = native.call(
            this,
            ['local_preferences'],
            'readwrite'
          );
          concurrent.objectStore('local_preferences').put({
            owner: signer.owner,
            id: 'public_hides',
            wire: JSON.stringify({
              schema: 1,
              family: 'public_hides',
              owner: signer.owner,
              id: 'public_hides',
              revision: 1,
              publishers: [second.owner]
            })
          });
        }
        return native.apply(this, args);
      };
      try {
        return await fixture.hide();
      } finally {
        IDBDatabase.prototype.transaction = native;
      }
    },
    async failReadback() {
      const native = Reflect.get<IDBDatabase, 'transaction'>(
        IDBDatabase.prototype,
        'transaction'
      );
      let reads = 0;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<typeof native>
      ) {
        if (
          this.name === 'harvestcircle_browser' &&
          args[1] === 'readonly' &&
          ++reads === 2
        )
          throw new DOMException(
            'controlled readback failure',
            'SecurityError'
          );
        return native.apply(this, args);
      };
      try {
        return await fixture.hide();
      } finally {
        IDBDatabase.prototype.transaction = native;
      }
    },
    async denyStorage() {
      const native = Reflect.get<IDBFactory, 'open'>(
        IDBFactory.prototype,
        'open'
      );
      IDBFactory.prototype.open = () => {
        throw new DOMException('controlled denied open', 'SecurityError');
      };
      try {
        await refreshLocalPublicHides(hides);
        return localPublicHidesSnapshot(hides);
      } finally {
        IDBFactory.prototype.open = native;
      }
    },
    async switchOwner() {
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        value: { getPublicKey: () => Promise.resolve(second.owner) }
      });
      await connectIdentityView(context);
    },
    async reconnect() {
      // Delay only the lock callback to exercise a real account-generation
      // transition during an admitted local action, before any IDB mutation.
      const native = navigator.locks.request.bind(navigator.locks);
      const descriptor = Object.getOwnPropertyDescriptor(
        navigator.locks,
        'request'
      );
      let release: (() => void) | undefined;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const wrapper = (
        name: string,
        options: LockOptions,
        callback: LockGrantedCallback<unknown>
      ) =>
        native(name, options, async (lock) => {
          await gate;
          return callback(lock);
        });
      Object.defineProperty(navigator.locks, 'request', {
        configurable: true,
        value: wrapper
      });
      try {
        const pending = undoLocalPublicPublisher(
          hides,
          event.pubkey,
          'reviewed_local_public_undo'
        );
        await connectIdentityView(context);
        release?.();
        const result = await pending;
        await refreshLocalPublicHides(hides);
        return result;
      } finally {
        release?.();
        if (descriptor)
          Object.defineProperty(navigator.locks, 'request', descriptor);
        else Reflect.deleteProperty(navigator.locks, 'request');
      }
    },
    async close() {
      if (panel) await unmount(panel);
      target?.remove();
      if (product) closeProductView(product);
      if (search) closeSearchView(search);
      closeLocalPublicHides(hides);
      closeIdentityView(context);
      closePublicRuntime(publicContext);
      closeBrowserDatabase(database);
      signer.close();
      second.close();
      original?.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
  return fixture;
}
