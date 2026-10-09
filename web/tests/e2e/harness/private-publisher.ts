import {
  makeFixture as makePairFixture,
  prepareSelfRecovery,
  preparePairedDelivery,
  preparedPairedDelivery
} from './paired-delivery.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  getPrivatePool,
  publishPrivateGiftWrapAttempt,
  subscribePrivatePage,
  closePrivatePool
} from '../../../src/lib/nostr/private-pool.ts';
import {
  capturePrivatePublication,
  privatePublicationSnapshot,
  type PrivatePublication
} from '../../../src/lib/nostr/private-publisher.ts';
import { deliverPrivateSend } from '../../../src/lib/messaging/deliver-send.ts';
import { browserDatabaseTransaction } from '../../../src/lib/persistence/database.ts';
import {
  reservedSendSnapshot,
  reservedSendRumorWire
} from '../../../src/lib/messaging/send-identity.ts';
import {
  createPrivateComposer,
  updatePrivateComposerText,
  bindPrivateComposerPreparation,
  privateComposerText,
  privateComposerSnapshot,
  closePrivateComposer,
  type PrivateComposer
} from '../../../src/lib/messaging/composer-state.ts';
import { disconnectIdentity } from '../../../src/lib/runtime/identity-session.ts';
export { privatePublicationSnapshot, publishPrivateGiftWrapAttempt };
export async function makeFixture(writeOnly = false) {
  const f = await makePairFixture({ messaging: true, peerRead: !writeOnly });
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('missing genuine private session');
  const peerOrigin = 'wss://peer.example.org',
    archiveOrigin = 'wss://archive.example.org';
  const pool = getPrivatePool(session, f.context.policy, [
    peerOrigin,
    archiveOrigin
  ]);
  if (!pool) throw Error('missing actual private pool');
  const controller = new AbortController();
  let composer: PrivateComposer | undefined;
  function bindComposer(newer = false) {
    const wire = reservedSendRumorWire(f.reserved);
    if (!wire) throw Error('missing genuine captured rumor');
    const content = (JSON.parse(wire) as { content: unknown }).content;
    composer = createPrivateComposer(f.identity, 'reviewed_private_composer');
    if (
      !composer ||
      !updatePrivateComposerText(composer, content, 'reviewed_private_text') ||
      !bindPrivateComposerPreparation(
        composer,
        f.reserved,
        f.preparation,
        undefined,
        'reviewed_composer_preparation'
      )
    )
      throw Error('missing genuine composer binding');
    if (
      newer &&
      !updatePrivateComposerText(
        composer,
        'HCP085_NEWER_PRIVATE_TEXT_MEMORY_ONLY',
        'reviewed_private_text'
      )
    )
      throw Error('newer text update failed');
    return {
      text: privateComposerText(composer),
      snapshot: privateComposerSnapshot(composer)
    };
  }
  function composerState() {
    return composer
      ? {
          text: privateComposerText(composer),
          snapshot: privateComposerSnapshot(composer)
        }
      : undefined;
  }
  async function prepare() {
    const self = await prepareSelfRecovery(
      f.preparation,
      'reviewed_self_recovery'
    );
    if (self.status !== 'saved') throw Error('missing self: ' + self.status);
    const pair = await preparePairedDelivery(
      f.preparation,
      f.context,
      'reviewed_pair_preparation'
    );
    if (pair.status !== 'prepared') throw Error('missing pair: ' + pair.status);
    return f.storedPair();
  }
  function permission(
    role: 'peer' | 'self_archive',
    origin = role === 'peer' ? peerOrigin : archiveOrigin,
    review: unknown = 'reviewed_private_delivery'
  ) {
    const receipt = preparedPairedDelivery(f.preparation);
    return receipt
      ? capturePrivatePublication(
          f.repository,
          session!,
          receipt,
          f.context,
          role,
          origin,
          review
        )
      : undefined;
  }
  async function publish(token: PrivatePublication) {
    return publishPrivateGiftWrapAttempt(pool!, token, controller.signal);
  }
  async function deliver(
    role: 'peer' | 'self_archive',
    origin = role === 'peer' ? peerOrigin : archiveOrigin
  ) {
    const receipt = preparedPairedDelivery(f.preparation);
    return receipt
      ? deliverPrivateSend(
          f.repository,
          session!,
          receipt,
          f.context,
          pool!,
          role,
          origin,
          'reviewed_private_delivery',
          controller.signal
        )
      : { status: 'invalid' as const };
  }
  async function remove() {
    const record = reservedSendSnapshot(f.reserved);
    if (!record) throw Error('missing command');
    const tx = browserDatabaseTransaction(
      f.database,
      ['private_sends'],
      'readwrite'
    );
    tx.objectStore('private_sends').delete([f.owner, record.id]);
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('fixture deletion aborted'))
      );
    });
  }
  let restore = () => {};
  function failReadback() {
    const cursor = Reflect.get<IDBIndex, 'openCursor'>(
      IDBIndex.prototype,
      'openCursor'
    );
    IDBIndex.prototype.openCursor = function (
      ...args: Parameters<IDBIndex['openCursor']>
    ) {
      if (this.objectStore.name === 'private_sends')
        throw Error('controlled actual private readback failure');
      return cursor.apply(this, args);
    };
    restore = () => {
      IDBIndex.prototype.openCursor = cursor;
    };
  }
  function readOrigins() {
    return new Promise<string[]>((resolve, reject) => {
      const seen = new Set<string>();
      try {
        subscribePrivatePage(pool!, 1, (message) => {
          if (message.type === 'candidate') seen.add(message.from);
          else if (message.reason === 'complete') resolve([...seen]);
          else reject(Error(message.reason));
        });
      } catch (error) {
        reject(
          error instanceof Error ? error : Error('fixture private read failed')
        );
      }
    });
  }
  return {
    ...f,
    session,
    pool,
    peerOrigin,
    archiveOrigin,
    prepare,
    prepareSelfOnly: () =>
      prepareSelfRecovery(f.preparation, 'reviewed_self_recovery'),
    permission,
    publish,
    deliver,
    remove,
    failReadback,
    readOrigins,
    bindComposer,
    composerState,
    stop: () => controller.abort(),
    disconnect: () => disconnectIdentity(f.identity),
    close() {
      if (composer) closePrivateComposer(composer);
      restore();
      controller.abort();
      closePrivatePool(pool);
      closePrivateSession(session);
      f.close();
    }
  };
}
