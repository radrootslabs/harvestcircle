import { getPublicKey } from 'applesauce-core/helpers';
import { inboxSetupViewOwnership } from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  captureCitedProduct,
  citedProductSnapshot,
  closeCitedProduct
} from '../../../src/lib/messaging/cited-product.ts';
import {
  createReplyController,
  updateReplyText,
  replySnapshot,
  replyText,
  sendReply,
  closeReplyController
} from '../../../src/lib/messaging/reply-controller.ts';
// HC_TEST_ONLY_RELAY: original identity, signed nested crypto and native IDB.
import { mount, unmount } from 'svelte';
import Panel from './private-blocks-panel.svelte';
export { makeFixture as createQueueFixture } from './decryption-queue.ts';
import { renderFixture, type Scenario } from './conversation-thread.ts';
import {
  privateBlocksForIdentity,
  createIdentityLocalPrivateBlocksView,
  mountLocalPrivateBlocksView,
  localPrivateBlocksSnapshot,
  refreshLocalPrivateBlocks,
  blockLocalPrivatePeer,
  closeLocalPrivateBlocksView,
  closeLocalPrivateBlocks,
  mountLocalPrivateBlocks,
  LOCAL_PRIVATE_BLOCKS_CONTEXT
} from '../../../src/lib/persistence/local-private-blocks.ts';
import {
  blockInboxConversation,
  inboxListSnapshot,
  inboxReplyOwnership,
  inboxMessageSnapshot,
  markInboxConversationDisplayed
} from '../../../src/lib/messaging/inbox-view.ts';
import {
  openBrowserDatabase,
  browserDatabaseTransaction,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import type { AdmittedConversation } from '../../../src/lib/messaging/admit-conversation.ts';
import type { UnlockedSession } from '../../../src/lib/messaging/unlocked-session.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
export async function createFixture(kind: Scenario = 'inbound') {
  const thread = await renderFixture(kind);
  const blocks = privateBlocksForIdentity(thread.identity);
  if (!blocks) throw Error('original block owner unavailable');
  const token = blocks;
  const view = createIdentityLocalPrivateBlocksView(thread.identity);
  await mountLocalPrivateBlocksView(view);
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready')
    throw Error('native fixture database unavailable');
  const database = opened.owner;
  const target = document.createElement('div');
  document.body.append(target);
  const panel = mount(Panel, {
    target,
    context: new Map([[LOCAL_PRIVATE_BLOCKS_CONTEXT, view]])
  });
  async function rows() {
    const tx = browserDatabaseTransaction(
      database,
      ['local_preferences'],
      'readonly'
    );
    const request = tx.objectStore('local_preferences').getAll();
    return await new Promise<unknown[]>((resolve, reject) => {
      tx.addEventListener('complete', () =>
        resolve(
          (request.result as { owner: string; id: string }[]).filter(
            (row) => row.owner === thread.owner && row.id === 'private_blocks'
          )
        )
      );
      tx.addEventListener('abort', () =>
        reject(Error('native preference scan aborted'))
      );
    });
  }
  async function corrupt() {
    const tx = browserDatabaseTransaction(
      database,
      ['local_preferences'],
      'readwrite'
    );
    tx.objectStore('local_preferences').put({
      owner: thread.owner,
      id: 'private_blocks',
      wire: 'corrupt'
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('native corrupt fixture aborted'))
      );
    });
    await refreshLocalPrivateBlocks(token);
  }
  const block = () =>
    blockInboxConversation(
      thread.controller,
      thread.conversationId,
      'reviewed_local_private_block'
    );
  async function foreignGeneration() {
    const proof = inboxReplyOwnership(
      thread.controller,
      thread.conversationId,
      thread.firstRumor
    );
    if (!proof?.current()) throw Error('original genuine proof unavailable');
    const other = createIdentitySession();
    await connectIdentity(other);
    await probeIdentityMessaging(other, 'reviewed_self_copy');
    const foreign = privateBlocksForIdentity(other);
    if (!foreign) throw Error('second original identity unavailable');
    try {
      await mountLocalPrivateBlocks(foreign);
      return await blockLocalPrivatePeer(
        foreign,
        proof.room,
        proof.unlocked,
        'reviewed_local_private_block'
      );
    } finally {
      closeLocalPrivateBlocks(foreign);
      disconnectIdentity(other);
    }
  }
  async function disposeSibling() {
    const sibling = createIdentityLocalPrivateBlocksView(thread.identity);
    await mountLocalPrivateBlocksView(sibling);
    closeLocalPrivateBlocksView(sibling);
    return localPrivateBlocksSnapshot(token);
  }
  let releaseInventory: (() => void) | undefined;
  function holdInventory() {
    const native = Reflect.get<IDBTransaction, 'addEventListener'>(
      IDBTransaction.prototype,
      'addEventListener'
    );
    let held = false;
    IDBTransaction.prototype.addEventListener = function (
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions
    ) {
      if (
        !held &&
        type === 'complete' &&
        this.mode === 'readonly' &&
        this.objectStoreNames.length === 1 &&
        this.objectStoreNames.contains('received_envelopes') &&
        listener
      ) {
        held = true;
        native.call(
          this,
          type,
          (event: Event) => {
            releaseInventory = () => {
              releaseInventory = undefined;
              if (typeof listener === 'function') listener.call(this, event);
              else listener.handleEvent(event);
            };
          },
          options
        );
        IDBTransaction.prototype.addEventListener = native;
      } else if (listener) native.call(this, type, listener, options);
    };
  }
  async function abortWrite() {
    const prototype = IDBObjectStore.prototype,
      original = Reflect.get<IDBObjectStore, 'put'>(prototype, 'put');
    prototype.put = function (value: unknown, key?: IDBValidKey) {
      const request = original.call(this, value, key);
      if (this.name === 'local_preferences') this.transaction.abort();
      return request;
    };
    try {
      return await block();
    } finally {
      prototype.put = original;
    }
  }
  async function failReadback() {
    const prototype = IDBDatabase.prototype,
      original = Reflect.get<IDBDatabase, 'transaction'>(
        prototype,
        'transaction'
      );
    let written = false;
    prototype.transaction = function (
      storeNames: string | string[],
      mode?: IDBTransactionMode,
      options?: IDBTransactionOptions
    ) {
      const names = typeof storeNames === 'string' ? [storeNames] : storeNames;
      if (names.includes('local_preferences') && mode === 'readonly' && written)
        throw Error('actual readback unavailable');
      const tx = original.call(this, storeNames, mode, options);
      if (names.includes('local_preferences') && mode === 'readwrite')
        tx.addEventListener('complete', () => {
          written = true;
        });
      return tx;
    };
    try {
      return await block();
    } finally {
      prototype.transaction = original;
    }
  }
  async function writePreferences(peers: readonly string[], revision: number) {
    const wire = JSON.stringify({
      schema: 1,
      family: 'private_blocks',
      owner: thread.owner,
      id: 'private_blocks',
      revision,
      peers: peers.slice().sort()
    });
    const tx = browserDatabaseTransaction(
      database,
      ['local_preferences'],
      'readwrite'
    );
    tx.objectStore('local_preferences').put({
      owner: thread.owner,
      id: 'private_blocks',
      wire
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('actual competing preference write aborted'))
      );
    });
    return wire;
  }
  const unrelatedPeer = getPublicKey(new Uint8Array(32).fill(88));
  async function matchingNativeRow() {
    const wire = await writePreferences([thread.peer, unrelatedPeer], 17);
    const before = await rows();
    const prototype = IDBDatabase.prototype;
    const native = Reflect.get<IDBDatabase, 'transaction'>(
      prototype,
      'transaction'
    );
    let writes = 0;
    prototype.transaction = function (
      names: string | string[],
      mode?: IDBTransactionMode,
      options?: IDBTransactionOptions
    ) {
      const stores = typeof names === 'string' ? [names] : names;
      if (mode === 'readwrite' && stores.includes('local_preferences'))
        writes++;
      return native.call(this, names, mode, options);
    };
    try {
      const result = await block();
      return {
        wire,
        before,
        after: await rows(),
        result,
        snapshot: localPrivateBlocksSnapshot(token),
        unrelatedPeer,
        writes
      };
    } finally {
      prototype.transaction = native;
    }
  }
  async function competingNativeRow() {
    const prototype = IDBDatabase.prototype;
    const native = Reflect.get<IDBDatabase, 'transaction'>(
      prototype,
      'transaction'
    );
    let intercepted = false,
      completion: Promise<string> | undefined;
    prototype.transaction = function (
      names: string | string[],
      mode?: IDBTransactionMode,
      options?: IDBTransactionOptions
    ) {
      const stores = typeof names === 'string' ? [names] : names;
      if (
        !intercepted &&
        mode === 'readwrite' &&
        stores.includes('local_preferences')
      ) {
        intercepted = true;
        // Queue a genuine independent native write before the producer's CAS
        // transaction. The actual IndexedDB scheduler orders both transactions.
        completion = writePreferences([unrelatedPeer], 19);
      }
      return native.call(this, names, mode, options);
    };
    try {
      const result = await block();
      await completion;
      if (!intercepted) throw Error('actual CAS seam was not reached');
      return {
        result,
        rows: await rows(),
        snapshot: localPrivateBlocksSnapshot(token),
        unrelatedPeer
      };
    } finally {
      prototype.transaction = native;
    }
  }
  async function reloadOwners() {
    thread.logout();
    const snapshots = [];
    for (const changed of [false, true, false]) {
      thread.mode(changed ? 'changed_key' : 'normal');
      const identity = createIdentitySession();
      await connectIdentity(identity);
      const fresh = privateBlocksForIdentity(identity);
      if (!fresh) throw Error('new original observed owner unavailable');
      const before = thread.delta();
      try {
        await mountLocalPrivateBlocks(fresh);
        snapshots.push({
          snapshot: localPrivateBlocksSnapshot(fresh),
          before,
          after: thread.delta()
        });
      } finally {
        closeLocalPrivateBlocks(fresh);
        disconnectIdentity(identity);
      }
    }
    thread.mode('normal');
    return {
      snapshots,
      old: localPrivateBlocksSnapshot(token),
      rows: await rows()
    };
  }
  let captured:
      | (() => Promise<{
          current: boolean;
          citation: unknown;
          message: unknown;
          reply: unknown;
          text: unknown;
          send: Awaited<ReturnType<typeof sendReply>>;
          displayed: boolean;
        }>)
      | undefined,
    closeCaptured = () => {};
  function captureActions() {
    if (captured) throw Error('original handles already captured');
    const message = thread
      .thread()
      .messages.find((row) =>
        row.citations.some((ref) => ref.naddr === thread.carrots)
      );
    if (!message) throw Error('actual authenticated cited message unavailable');
    const proof = inboxReplyOwnership(
      thread.controller,
      thread.conversationId,
      message.data.rumorId
    );
    const setup = proof && inboxSetupViewOwnership(proof.setup);
    if (!proof?.current() || !setup)
      throw Error('original current ownership unavailable');
    const context = createPublicRuntimeContext();
    const runtime = mountPublicRuntime(context, setup.policy);
    if (!runtime) throw Error('original public runtime unavailable');
    const citation = captureCitedProduct(
      thread.controller,
      thread.conversationId,
      message.data.rumorId,
      thread.carrots,
      runtime,
      'reviewed_product_citation'
    );
    const reply = createReplyController(
      thread.controller,
      thread.conversationId,
      message.data.rumorId
    );
    if (!citation || !reply) {
      closePublicRuntime(context);
      throw Error('original actions unavailable');
    }
    if (
      !updateReplyText(
        reply,
        'HCP123_OLD_REPLY_MEMORY_ONLY',
        'reviewed_private_text'
      )
    )
      throw Error('original reply text unavailable');
    closeCaptured = () => {
      closeCitedProduct(citation);
      closeReplyController(reply);
      closePublicRuntime(context);
    };
    captured = async () => ({
      current: proof.current(),
      citation: citedProductSnapshot(citation),
      message: inboxMessageSnapshot(
        thread.controller,
        thread.conversationId,
        message.data.rumorId
      ),
      reply: replySnapshot(reply),
      text: replyText(reply),
      send: await sendReply(reply, 'reviewed_reply_send'),
      displayed: await markInboxConversationDisplayed(
        thread.controller,
        thread.conversationId,
        [message.data.rumorId],
        'reviewed_displayed_conversation'
      )
    });
    return {
      current: proof.current(),
      citation: citedProductSnapshot(citation),
      text: replyText(reply)
    };
  }
  async function failLockCompletion() {
    const locks = navigator.locks;
    const native = Reflect.get<LockManager, 'request'>(locks, 'request');
    const descriptor = Object.getOwnPropertyDescriptor(locks, 'request');
    Object.defineProperty(locks, 'request', {
      configurable: true,
      value: async (...args: unknown[]) => {
        // Execute the actual owner WebLock and original native write/readback;
        // then lose the caller's completion channel, without fabricating a row.
        await Reflect.apply(native, locks, args);
        throw Error('controlled owner-lock completion channel unavailable');
      }
    });
    try {
      return await block();
    } finally {
      if (descriptor) Object.defineProperty(locks, 'request', descriptor);
      else Reflect.deleteProperty(locks, 'request');
    }
  }
  return {
    ...thread,
    rows,
    corrupt,
    abortWrite,
    failReadback,
    block,
    foreignGeneration,
    matchingNativeRow,
    competingNativeRow,
    reloadOwners,
    captureActions,
    failLockCompletion,
    capturedActions: () => {
      if (!captured) throw Error('original handles not captured');
      return captured();
    },
    disposeSibling,
    waitOwner: () =>
      navigator.locks.request('harvestcircle:owner:' + thread.owner, () =>
        Promise.resolve()
      ),
    holdInventory,
    inventoryHeld: () => !!releaseInventory,
    releaseInventory: () => releaseInventory?.(),
    preferences: () => localPrivateBlocksSnapshot(token),
    list: () => inboxListSnapshot(thread.controller),
    refresh: () => refreshLocalPrivateBlocks(token),
    fake: () => {
      const room = inboxReplyOwnership(
        thread.controller,
        thread.conversationId,
        thread.firstRumor
      );
      return blockLocalPrivatePeer(
        token,
        { ...(room?.room ?? {}) } as AdmittedConversation,
        room?.unlocked ?? ({} as UnlockedSession),
        'reviewed_local_private_block'
      );
    },
    async close() {
      closeCaptured();
      releaseInventory?.();
      await unmount(panel);
      target.remove();
      closeLocalPrivateBlocksView(view);
      closeBrowserDatabase(database);
      await thread.close();
    }
  };
}
