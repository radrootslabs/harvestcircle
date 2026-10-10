// HC_TEST_ONLY_QUALIFICATION_PORT: actual browser owners/native acknowledgments.
import * as editorApi from '../../../src/lib/publishing/listing-editor.ts';
import {
  createFixture as baseFixture,
  blank,
  inspectNative,
  nativeObservation
} from './listing-editor.ts';
import { createFixture as operationFixture } from './listing-operation-view.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  disconnectIdentityView,
  closeIdentityView
} from '../../../src/lib/runtime/view-context.ts';
import type { PublicDraftForm } from '../../../src/lib/contracts/local-records.ts';
export { blank, inspectNative, nativeObservation };
export async function createFixture() {
  const base = await baseFixture(),
    context = createIdentityViewContext();
  mountIdentityView(context);
  await connectIdentityView(context);
  let editor: editorApi.ListingEditor | undefined;
  let operation: Awaited<ReturnType<typeof operationFixture>> | undefined;
  return {
    inspect: base.inspect,
    counts: base.counts,
    async open(route: editorApi.ListingEditorRoute) {
      const result = await editorApi.openListingEditor(context, route);
      if (result.status === 'ready') editor = result.editor;
      return result.status === 'ready'
        ? {
            status: result.status,
            snapshot: editorApi.listingEditorSnapshot(result.editor)
          }
        : result;
    },
    snapshot: () => editor && editorApi.listingEditorSnapshot(editor),
    edit: (form: PublicDraftForm) =>
      editor
        ? editorApi.editListingEditor(editor, form)
        : Promise.resolve(false),
    async saveClose(copied = false) {
      const action: unknown = Reflect.get(
        editorApi,
        'saveAndCloseListingEditor'
      );
      if (typeof action !== 'function')
        throw Error(
          'HCP116 save-close producer absent; no passing result inferred'
        );
      return editor
        ? await (
            action as (value: editorApi.ListingEditor) => Promise<boolean>
          )(copied ? { ...editor } : editor)
        : false;
    },
    disconnect() {
      disconnectIdentityView(context);
    },
    async captured(unknown = false) {
      operation = await operationFixture();
      if (unknown) {
        await operation.holdSigning();
        await operation.stopSigning();
      } else if ((await operation.signed()) !== 'prepared')
        throw Error('actual signature failed');
      const row = await operation.row();
      if (row.source.type !== 'draft')
        throw Error('actual source draft missing');
      disconnectIdentityView(context);
      await connectIdentityView(context);
      return {
        draftId: row.source.id,
        command: row.id,
        row,
        signs: operation.counts()
      };
    },
    async close() {
      if (editor) editorApi.closeListingEditor(editor);
      closeIdentityView(context);
      operation?.close();
      await base.close();
    }
  };
}
// Abort actual native transactions after a genuine successful put; no fake event/result.
export function abortDraftWrites() {
  const descriptor = Object.getOwnPropertyDescriptor(
    IDBObjectStore.prototype,
    'put'
  )!;
  const original = descriptor.value as IDBObjectStore['put'];
  let enabled = true,
    aborts = 0;
  const ids: string[] = [];
  Object.defineProperty(IDBObjectStore.prototype, 'put', {
    ...descriptor,
    value: function (this: IDBObjectStore, value: unknown, key?: IDBValidKey) {
      const request = original.call(this, value, key),
        transaction = this.transaction;
      if (
        enabled &&
        this.name === 'public_drafts' &&
        transaction.mode === 'readwrite'
      ) {
        if (
          value &&
          typeof value === 'object' &&
          'id' in value &&
          typeof value.id === 'string'
        )
          ids.push(value.id);
        request.addEventListener('success', () => {
          if (enabled) {
            aborts++;
            transaction.abort();
          }
        });
      }
      return request;
    }
  });
  return {
    state: () => ({ aborts, ids: ids.slice() }),
    restore() {
      enabled = false;
      Object.defineProperty(IDBObjectStore.prototype, 'put', descriptor);
    }
  };
}
// Lose only original application request ACK callbacks after an actual committed
// put and its genuine readback. Native completion events/results are untouched.
export function unknownDraftAcknowledgment() {
  const putDescriptor = Object.getOwnPropertyDescriptor(
    IDBObjectStore.prototype,
    'put'
  )!;
  const originalPut = putDescriptor.value as IDBObjectStore['put'];
  const ownListener = Object.getOwnPropertyDescriptor(
    IDBRequest.prototype,
    'addEventListener'
  );
  const listenerDescriptor =
    ownListener ??
    Object.getOwnPropertyDescriptor(EventTarget.prototype, 'addEventListener')!;
  const originalListener =
    listenerDescriptor.value as IDBRequest['addEventListener'];
  const held: {
    target: IDBRequest;
    event: Event;
    listener: EventListenerOrEventListenerObject;
  }[] = [];
  let enabled = true,
    putSubmitted = false,
    puts = 0;
  Object.defineProperty(IDBObjectStore.prototype, 'put', {
    ...putDescriptor,
    value: function (this: IDBObjectStore, value: unknown, key?: IDBValidKey) {
      if (
        this.name === 'public_drafts' &&
        this.transaction.mode === 'readwrite'
      ) {
        putSubmitted = true;
        puts++;
      }
      return originalPut.call(this, value, key);
    }
  });
  Object.defineProperty(IDBRequest.prototype, 'addEventListener', {
    configurable: true,
    writable: true,
    value: function (
      this: IDBRequest,
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: AddEventListenerOptions | boolean
    ) {
      if (!listener) return;
      return originalListener.call(
        this,
        type,
        function (event: Event) {
          const target = event.currentTarget as IDBRequest,
            source = target.source;
          if (
            enabled &&
            putSubmitted &&
            type === 'success' &&
            source instanceof IDBObjectStore &&
            source.name === 'public_drafts'
          )
            held.push({ target, event, listener });
          else if (typeof listener === 'function') listener.call(target, event);
          else listener.handleEvent(event);
        },
        options
      );
    }
  });
  return {
    state: () => ({ puts, held: held.length }),
    release() {
      enabled = false;
      Object.defineProperty(IDBObjectStore.prototype, 'put', putDescriptor);
      if (ownListener)
        Object.defineProperty(
          IDBRequest.prototype,
          'addEventListener',
          ownListener
        );
      else Reflect.deleteProperty(IDBRequest.prototype, 'addEventListener');
      for (const row of held.splice(0))
        if (typeof row.listener === 'function')
          row.listener.call(row.target, row.event);
        else row.listener.handleEvent(row.event);
    }
  };
}
