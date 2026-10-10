// HC_TEST_ONLY_QUALIFICATION_PORT: original production owners and actual native
// IndexedDB. Controlled installed provider is not extension/operator/live Q.
import { mount, unmount, type ComponentProps } from 'svelte';
import ListingEditor from '../../../src/lib/components/ListingEditor.svelte';
import { makeFixture } from './approved-signing.ts';
import { createFixture as createOperationFixture } from './listing-operation-view.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  disconnectIdentityView,
  closeIdentityView,
  identityViewSnapshot,
  IDENTITY_VIEW_CONTEXT
} from '../../../src/lib/runtime/view-context.ts';
import {
  openListingEditor,
  listingEditorSnapshot,
  editListingEditor,
  flushListingEditor,
  closeListingEditor,
  subscribeListingEditor,
  type ListingEditor as Editor,
  type ListingEditorRoute
} from '../../../src/lib/publishing/listing-editor.ts';
import {
  browserDatabaseName,
  browserSchemaVersion
} from '../../../src/lib/persistence/schema.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository
} from '../../../src/lib/persistence/public-publications.ts';
import {
  readPublicDraft,
  capturePublicDraftSave,
  commitPublicDraftWrite
} from '../../../src/lib/persistence/drafts.ts';
import { inspectLocalTemplate } from '../../../src/lib/nostr/local-template.ts';
import { projectFoodParts } from '../../../src/lib/contracts/food-availability-v1/read.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import type { PublicDraftForm } from '../../../src/lib/contracts/local-records.ts';
export const reference = (
  identifier: string,
  owner = makeFixture(30402).owner
) => encodeProductReference({ kind: 30402, pubkey: owner, identifier });
export const blank = (): PublicDraftForm => ({
  title: '',
  description: '',
  location: '',
  amount: '',
  currency: '',
  unit: '',
  quantity: '',
  contactType: '',
  contactValue: ''
});
export function installProvider() {
  const signer = makeFixture(30402);
  let signs = 0,
    keys = 0;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => {
        keys++;
        return Promise.resolve(signer.owner);
      },
      signEvent: (value: Parameters<typeof signer.sign>[0]) => {
        signs++;
        return Promise.resolve(signer.sign(value));
      }
    }
  });
  return { owner: signer.owner, counts: () => ({ signs, keys }) };
}
export async function createFixture(guest = false) {
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr'),
    provider = installProvider(),
    context = createIdentityViewContext();
  let activeOwner = provider.owner;
  mountIdentityView(context);
  if (!guest) await connectIdentityView(context);
  let editor: Editor | undefined,
    mounted: ReturnType<typeof mount> | undefined,
    target: HTMLElement | undefined;
  let operation: Awaited<ReturnType<typeof createOperationFixture>> | undefined;
  const cancel = new AbortController();
  async function database() {
    const opened = await openBrowserDatabase();
    if (opened.state !== 'ready') throw Error(opened.reason);
    return opened.owner;
  }
  async function inspect() {
    const db = await database();
    try {
      const transaction = browserDatabaseTransaction(
        db,
        ['public_drafts', 'public_operations'],
        'readonly'
      );
      const result = await new Promise<{
        drafts: unknown[];
        operations: unknown[];
      }>((resolve, reject) => {
        const a = transaction.objectStore('public_drafts').getAll(),
          b = transaction.objectStore('public_operations').getAll();
        transaction.addEventListener('complete', () =>
          resolve({ drafts: a.result, operations: b.result })
        );
        transaction.addEventListener('abort', () =>
          reject(Error('native inspection aborted'))
        );
      });
      return result;
    } finally {
      closeBrowserDatabase(db);
    }
  }
  return {
    get owner() {
      return activeOwner;
    },
    counts: () => (operation ? operation.counts() : provider.counts()),
    identity: () => identityViewSnapshot(context),
    inspect,
    async open(route: ListingEditorRoute, forged = false) {
      const result = await openListingEditor(
        forged ? { ...context } : context,
        route,
        cancel.signal
      );
      if (result.status === 'ready') editor = result.editor;
      return result.status === 'ready'
        ? { status: 'ready', snapshot: listingEditorSnapshot(result.editor) }
        : result;
    },
    snapshot: () => editor && listingEditorSnapshot(editor),
    async edit(form: PublicDraftForm, copied = false) {
      return editor
        ? await editListingEditor(copied ? { ...editor } : editor, form)
        : false;
    },
    async flush(copied = false) {
      return editor
        ? await flushListingEditor(copied ? { ...editor } : editor)
        : false;
    },
    copiedClose() {
      if (editor) closeListingEditor({ ...editor });
    },
    copiedSubscribe() {
      let calls = 0;
      const off = editor
        ? subscribeListingEditor({ ...editor }, () => {
            calls++;
          })
        : () => {};
      off();
      return calls;
    },
    cancel() {
      cancel.abort();
    },
    disconnect() {
      disconnectIdentityView(context);
    },
    async render(route: ListingEditorRoute) {
      if (mounted) await unmount(mounted);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp115-editor';
      document.body.append(target);
      mounted = mount(ListingEditor, {
        target,
        props: { route } as ComponentProps<typeof ListingEditor>,
        context: new Map([[IDENTITY_VIEW_CONTEXT, context]])
      });
    },
    async unmount() {
      if (mounted) await unmount(mounted);
      mounted = undefined;
      target?.remove();
      target = undefined;
    },
    async prototype() {
      operation = await createOperationFixture();
      if ((await operation.signed()) !== 'prepared')
        throw Error('genuine original signature did not settle');
      const row = await operation.row();
      disconnectIdentityView(context);
      await connectIdentityView(context);
      const observed = identityViewSnapshot(context).identity;
      if (
        observed.state === 'guest' ||
        observed.state === 'pending' ||
        observed.publicKey !== row.owner
      )
        throw Error('actual original prototype author admission failed');
      if (row.source.type !== 'draft')
        throw Error('original source unavailable');
      const template = inspectLocalTemplate(row.capture.wire, row.owner, 30402),
        projection = template && projectFoodParts(template);
      if (!projection || projection.outcome !== 'focused')
        throw Error('actual prototype unavailable');
      activeOwner = row.owner;
      const naddr = encodeProductReference({
        kind: 30402,
        pubkey: row.owner,
        identifier: projection.projection.identifier
      });
      if (!naddr) throw Error('actual coordinate unavailable');
      return {
        naddr,
        draftId: row.source.id,
        sourceRevision: row.source.revision,
        eventId: row.artifact?.eventId
      };
    },
    async advanceDraft(id: string) {
      const db = await database();
      try {
        const publications = createPublicPublicationRepository(db, activeOwner),
          repository =
            publications && publicPublicationDraftRepository(publications);
        if (!repository) throw Error('original draft repo unavailable');
        const read = await readPublicDraft(repository, id);
        if (!read.ok) throw Error(read.reason);
        const captured = capturePublicDraftSave(
          repository,
          id,
          read.value.revision,
          {
            ...read.value.form,
            description: read.value.form.description + ' changed'
          }
        );
        if (!captured.ok) throw Error(captured.reason);
        const saved = await commitPublicDraftWrite(repository, captured.value);
        if (!saved.ok) throw Error(saved.reason);
        return saved.value;
      } finally {
        closeBrowserDatabase(db);
      }
    },
    async deleteDraft(id: string) {
      const db = await database();
      try {
        const transaction = browserDatabaseTransaction(
          db,
          ['public_drafts'],
          'readwrite'
        );
        const done = new Promise<void>((resolve, reject) => {
          transaction.addEventListener('complete', () => resolve());
          transaction.addEventListener('abort', () =>
            reject(Error('controlled native deletion aborted'))
          );
        });
        transaction.objectStore('public_drafts').delete([activeOwner, id]);
        await done;
      } finally {
        closeBrowserDatabase(db);
      }
    },
    async damageSource(
      mode: 'advanced' | 'deleted' | 'mismatched' | 'ambiguous' | 'external',
      id: string
    ) {
      if (mode === 'advanced') return await this.advanceDraft(id);
      if (mode === 'deleted') return await this.deleteDraft(id);
      if (!operation) throw Error('actual original prototype missing');
      if (mode === 'external') {
        await operation.externalSource();
        return;
      }
      const db = await database();
      try {
        if (mode === 'ambiguous') {
          const row = await operation.row(),
            id = crypto.randomUUID();
          const transaction = browserDatabaseTransaction(
            db,
            ['public_operations'],
            'readwrite'
          );
          const done = new Promise<void>((resolve, reject) => {
            transaction.addEventListener('complete', () => resolve());
            transaction.addEventListener('abort', () =>
              reject(Error('controlled ambiguity abort'))
            );
          });
          transaction.objectStore('public_operations').put({
            owner: row.owner,
            id,
            wire: JSON.stringify({ ...row, id })
          });
          await done;
          return;
        }
        const publications = createPublicPublicationRepository(db, activeOwner),
          repository =
            publications && publicPublicationDraftRepository(publications);
        if (!repository) throw Error('actual repository missing');
        const read = await readPublicDraft(repository, id);
        if (!read.ok) throw Error(read.reason);
        const row = {
          ...read.value,
          form: {
            ...read.value.form,
            title: 'Different actual bounded draft value'
          }
        };
        const transaction = browserDatabaseTransaction(
          db,
          ['public_drafts'],
          'readwrite'
        );
        const done = new Promise<void>((resolve, reject) => {
          transaction.addEventListener('complete', () => resolve());
          transaction.addEventListener('abort', () =>
            reject(Error('controlled original source mismatch abort'))
          );
        });
        transaction
          .objectStore('public_drafts')
          .put({ owner: activeOwner, id, wire: JSON.stringify(row) });
        await done;
      } finally {
        closeBrowserDatabase(db);
      }
    },
    async close() {
      cancel.abort();
      if (editor) closeListingEditor(editor);
      if (mounted) await unmount(mounted);
      target?.remove();
      closeIdentityView(context);
      operation?.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else delete window.nostr;
    }
  };
}
// Observe actual native calls. Holds defer original application listeners only;
// native completion remains genuine. No synthetic request/transaction/result.
export function nativeObservation() {
  const requestDescriptor = Object.getOwnPropertyDescriptor(
      IDBRequest.prototype,
      'addEventListener'
    ),
    transactionDescriptor = Object.getOwnPropertyDescriptor(
      IDBTransaction.prototype,
      'addEventListener'
    );
  const get = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'get')!
    .value as IDBObjectStore['get'];
  const put = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'put')!
    .value as IDBObjectStore['put'];
  const open = Object.getOwnPropertyDescriptor(IDBFactory.prototype, 'open')!
    .value as IDBFactory['open'];
  const close = Object.getOwnPropertyDescriptor(IDBDatabase.prototype, 'close')!
    .value as IDBDatabase['close'];
  const inherited = Object.getOwnPropertyDescriptor(
    EventTarget.prototype,
    'addEventListener'
  );
  if (!inherited) throw Error('actual native registration unavailable');
  const requestAdd = (requestDescriptor ?? inherited)
    .value as IDBRequest['addEventListener'];
  const transactionAdd = (transactionDescriptor ?? inherited)
    .value as IDBTransaction['addEventListener'];
  let opens = 0,
    closes = 0,
    completions = 0,
    gets = 0,
    puts = 0,
    mode: 'none' | 'open-hold' | 'draft-hold' | 'save-hold' | 'open-after' =
      'none',
    held: (() => void) | undefined,
    after: (() => void) | undefined;
  IDBObjectStore.prototype.get = function (...args: Parameters<typeof get>) {
    if (this.name === 'public_drafts' || this.name === 'public_operations')
      gets++;
    return get.call(this, ...args);
  };
  IDBObjectStore.prototype.put = function (...args: Parameters<typeof put>) {
    if (this.name === 'public_drafts' || this.name === 'public_operations')
      puts++;
    return put.call(this, ...args);
  };
  IDBFactory.prototype.open = function (...args: Parameters<typeof open>) {
    opens++;
    return open.call(this, ...args);
  };
  IDBDatabase.prototype.close = function () {
    closes++;
    return close.call(this);
  };
  IDBRequest.prototype.addEventListener = function (
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions
  ) {
    if (!listener) return;
    if (
      this instanceof IDBOpenDBRequest &&
      type === 'success' &&
      listener &&
      (mode === 'open-hold' || mode === 'open-after')
    ) {
      const chosen = mode;
      mode = 'none';
      return requestAdd.call(
        this,
        type,
        (event: Event) => {
          const original = event.currentTarget;
          if (!(original instanceof EventTarget))
            throw Error('actual native completion target unavailable');
          completions++;
          const invoke = () =>
            typeof listener === 'function'
              ? listener.call(original, event)
              : listener.handleEvent(event);
          if (chosen === 'open-hold') held = invoke;
          else {
            invoke();
            after?.();
          }
        },
        options
      );
    }
    return requestAdd.call(this, type, listener, options);
  };
  IDBTransaction.prototype.addEventListener = function (
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions
  ) {
    if (!listener) return;
    if (
      type === 'complete' &&
      listener &&
      this.objectStoreNames.contains('public_drafts') &&
      ((mode === 'draft-hold' && this.mode === 'readonly') ||
        (mode === 'save-hold' && this.mode === 'readwrite'))
    ) {
      mode = 'none';
      return transactionAdd.call(
        this,
        type,
        (event: Event) => {
          const original = event.currentTarget;
          if (!(original instanceof EventTarget))
            throw Error('actual native completion target unavailable');
          completions++;
          held = () =>
            typeof listener === 'function'
              ? listener.call(original, event)
              : listener.handleEvent(event);
        },
        options
      );
    }
    return transactionAdd.call(this, type, listener, options);
  };
  return {
    holdOpen() {
      mode = 'open-hold';
    },
    holdDraft() {
      mode = 'draft-hold';
    },
    holdSave() {
      mode = 'save-hold';
    },
    afterOpen(callback: () => void) {
      mode = 'open-after';
      after = callback;
    },
    state: () => ({ opens, closes, completions, gets, puts, held: !!held }),
    release() {
      const callback = held;
      held = undefined;
      callback?.();
    },
    restore() {
      IDBObjectStore.prototype.get = get;
      IDBObjectStore.prototype.put = put;
      IDBFactory.prototype.open = open;
      IDBDatabase.prototype.close = close;
      if (requestDescriptor)
        Object.defineProperty(
          IDBRequest.prototype,
          'addEventListener',
          requestDescriptor
        );
      else Reflect.deleteProperty(IDBRequest.prototype, 'addEventListener');
      if (transactionDescriptor)
        Object.defineProperty(
          IDBTransaction.prototype,
          'addEventListener',
          transactionDescriptor
        );
      else Reflect.deleteProperty(IDBTransaction.prototype, 'addEventListener');
    }
  };
}

export async function inspectNative() {
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  try {
    const transaction = browserDatabaseTransaction(
      opened.owner,
      ['public_drafts', 'public_operations'],
      'readonly'
    );
    return await new Promise<{ drafts: unknown[]; operations: unknown[] }>(
      (resolve, reject) => {
        const drafts = transaction.objectStore('public_drafts').getAll(),
          operations = transaction.objectStore('public_operations').getAll();
        transaction.addEventListener('complete', () =>
          resolve({ drafts: drafts.result, operations: operations.result })
        );
        transaction.addEventListener('abort', () =>
          reject(Error('actual native inspection aborted'))
        );
      }
    );
  } finally {
    closeBrowserDatabase(opened.owner);
  }
}

export async function nativeVersionChange() {
  const request = indexedDB.open(browserDatabaseName, browserSchemaVersion + 1);
  return await new Promise<{ drafts: unknown[] }>((resolve, reject) => {
    request.addEventListener('success', () => {
      const database = request.result,
        transaction = database.transaction(['public_drafts'], 'readonly'),
        drafts = transaction.objectStore('public_drafts').getAll();
      transaction.addEventListener('complete', () => {
        database.close();
        resolve({ drafts: drafts.result });
      });
      transaction.addEventListener('abort', () => {
        database.close();
        reject(Error('controlled upgraded read aborted'));
      });
    });
    request.addEventListener('error', () =>
      reject(request.error ?? Error('native upgrade failed'))
    );
  });
}
