import { makeFixture as makePairedFixture } from './paired-delivery.ts';
import { reservedSendRumorWire } from '../../../src/lib/messaging/send-identity.ts';
import { disconnectIdentity } from '../../../src/lib/runtime/identity-session.ts';
import { captureEnquiryContext } from '../../../src/lib/messaging/enquiry-context.ts';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import type { EventTemplate, finalizeEvent } from 'applesauce-core/helpers';
import {
  createPrivateComposer,
  updatePrivateComposerText,
  privateComposerText,
  privateComposerSnapshot,
  bindPrivateComposerPreparation,
  closePrivateComposer
} from '../../../src/lib/messaging/composer-state.ts';
import {
  captureDirtyNavigation,
  requestPrivateNavigation,
  confirmDiscardNavigation,
  keepPrivateEditing,
  dirtyNavigationSnapshot,
  privateNavigationBeforeUnload,
  type DirtyNavigation
} from '../../../src/lib/runtime/dirty-navigation.ts';
import {
  prepareSelfRecovery,
  preparePairedDelivery,
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  captureSelfRecoveryPreparation,
  stopSelfRecoveryPreparation
} from '../../../src/lib/messaging/prepare-send.ts';
export {
  updatePrivateComposerText,
  privateComposerText,
  privateComposerSnapshot,
  bindPrivateComposerPreparation,
  closePrivateComposer,
  requestPrivateNavigation,
  confirmDiscardNavigation,
  keepPrivateEditing,
  dirtyNavigationSnapshot,
  prepareSelfRecovery,
  preparePairedDelivery,
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  captureSelfRecoveryPreparation,
  stopSelfRecoveryPreparation,
  disconnectIdentity
};
export function attachPrivateBeforeUnload(token: DirtyNavigation) {
  const handler = (event: BeforeUnloadEvent) => {
    if (privateNavigationBeforeUnload(token, event)) event.returnValue = '';
  };
  window.addEventListener('beforeunload', handler);
  return () => window.removeEventListener('beforeunload', handler);
}
export async function makeFixture() {
  const f = await makePairedFixture(),
    wire = reservedSendRumorWire(f.reserved),
    composerInput = createPrivateComposer(
      f.identity,
      'reviewed_private_composer'
    );
  if (!wire || !composerInput) throw Error('missing private fixture');
  const composer = composerInput;
  const original = (JSON.parse(wire) as { content: string }).content;
  if (!updatePrivateComposerText(composer, original, 'reviewed_private_text'))
    throw Error('private text refused');
  if (
    !bindPrivateComposerPreparation(
      composer,
      f.reserved,
      f.preparation,
      undefined,
      'reviewed_composer_preparation'
    )
  )
    throw Error('private intent binding refused');
  const navigationInput = captureDirtyNavigation(composer);
  if (!navigationInput) throw Error('dirty contract unavailable');
  const navigation = navigationInput;
  async function enquiryContext(title = 'Carrots') {
    const provider = window.nostr as Readonly<{
      signEvent(
        template: EventTemplate
      ): Promise<ReturnType<typeof finalizeEvent>>;
    }>;
    f.mode('wrong_author');
    try {
      const signed = await provider.signEvent({
        kind: 30402,
        created_at: 1700000060,
        content: 'Public carrots',
        tags: [
          ['d', 'carrots'],
          ['title', title],
          ['summary', 'Fresh carrots'],
          ['published_at', '1700000000'],
          ['location', 'Victoria'],
          ['price', '3.5', 'CAD'],
          ['radroots:price_unit', 'lb'],
          ['status', 'active']
        ]
      });
      const proof = verifyEnvelope(
          JSON.stringify({
            id: signed.id,
            pubkey: signed.pubkey,
            sig: signed.sig,
            kind: signed.kind,
            created_at: signed.created_at,
            tags: signed.tags,
            content: signed.content
          })
        ),
        context = proof.ok && captureEnquiryContext(proof.value);
      if (!context) throw Error('missing genuine enquiry context');
      return context;
    } finally {
      f.mode('normal');
    }
  }
  // The fixture exercises the shared contract with real browser controls. It
  // does not add a production page or qualify a manual accessibility review.
  const panel = document.createElement('section'),
    textarea = document.createElement('textarea'),
    warning = document.createElement('p'),
    keep = document.createElement('button'),
    discard = document.createElement('button');
  panel.setAttribute('aria-label', 'Private source fixture');
  textarea.setAttribute('aria-label', 'Private text');
  textarea.value = original;
  warning.setAttribute('role', 'status');
  keep.textContent = 'Keep editing';
  discard.textContent = 'Discard draft';
  function render() {
    warning.textContent = dirtyNavigationSnapshot(navigation)?.copy ?? '';
    textarea.value = privateComposerText(composer) ?? '';
  }
  keep.addEventListener('click', () => {
    keepPrivateEditing(navigation);
    render();
    textarea.focus();
  });
  discard.addEventListener('click', () => {
    confirmDiscardNavigation(navigation, 'reviewed_discard_private_text');
    render();
  });
  panel.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      keepPrivateEditing(navigation);
      render();
      textarea.focus();
    }
  });
  textarea.addEventListener('input', () => {
    updatePrivateComposerText(
      composer,
      textarea.value,
      'reviewed_private_text'
    );
    render();
  });
  panel.append(textarea, warning, keep, discard);
  document.body.append(panel);
  let restoreReadback = () => {};
  let readbackFailures = 0;
  function loseReadback() {
    restoreReadback();
    const put = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      ),
      openCursor = Reflect.get<IDBIndex, 'openCursor'>(
        IDBIndex.prototype,
        'openCursor'
      );
    let committed = false;
    readbackFailures = 0;
    IDBObjectStore.prototype.put = function (
      value: unknown,
      key?: IDBValidKey
    ) {
      const request =
        key === undefined ? put.call(this, value) : put.call(this, value, key);
      if (this.name === 'private_sends') committed = true;
      return request;
    };
    IDBIndex.prototype.openCursor = function (
      query?: IDBValidKey | IDBKeyRange | null,
      direction?: IDBCursorDirection
    ) {
      if (this.objectStore.name === 'private_sends' && committed) {
        readbackFailures++;
        throw Error('controlled readback unavailable');
      }
      return openCursor.call(this, query, direction);
    };
    restoreReadback = () => {
      IDBObjectStore.prototype.put = put;
      IDBIndex.prototype.openCursor = openCursor;
    };
    return restoreReadback;
  }
  return {
    ...f,
    composer,
    navigation,
    original,
    enquiryContext,
    loseReadback,
    readbackFailures: () => readbackFailures,
    requestClose() {
      const result = requestPrivateNavigation(navigation);
      render();
      keep.focus();
      return result;
    },
    close() {
      restoreReadback();
      panel.remove();
      closePrivateComposer(composer);
      f.close();
    }
  };
}
