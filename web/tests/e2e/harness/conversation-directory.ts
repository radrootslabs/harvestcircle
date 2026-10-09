import { messageMetadataOwnership } from '../../../src/lib/persistence/message-metadata.ts';
import { privateSessionOwnership } from '../../../src/lib/runtime/private-session.ts';
import { makeFixture as makeMetadataFixture } from './message-metadata.ts';
import {
  captureConversationDirectory,
  resolveLocalConversation,
  rememberAdmittedConversation,
  closeConversationDirectory,
  type ConversationDirectory
} from '../../../src/lib/messaging/conversation-directory.ts';
import type { AdmittedConversation } from '../../../src/lib/messaging/admit-conversation.ts';
export async function makeFixture(initialise = true) {
  const f = await makeMetadataFixture('inbound', initialise);
  let directory: ConversationDirectory | undefined;
  let otherDirectory: ConversationDirectory | undefined;
  return {
    ...f,
    async ready() {
      if ((await f.unlock()) !== 'authenticated')
        throw Error('no genuine nested admission');
      if (
        !f.admit('inbound') ||
        !f.captureCache() ||
        f.cache() !== 'added' ||
        !(await f.captureMetadata())
      )
        throw Error('no genuine original room/cache/metadata');
    },
    capture(review: unknown = 'reviewed_conversation_directory') {
      const { privateSession, metadata } = f.navigationInputs();
      directory =
        metadata &&
        captureConversationDirectory(privateSession, metadata, review);
      return !!directory;
    },
    resolve(id: unknown) {
      return directory
        ? resolveLocalConversation(directory, id)
        : Promise.resolve({ status: 'unavailable' });
    },
    remember(review: unknown = 'reviewed_admitted_conversation_navigation') {
      const { room } = f.navigationInputs();
      return directory && room
        ? rememberAdmittedConversation(directory, room, review)
        : Promise.resolve({ status: 'unavailable' });
    },
    forgedRoom() {
      return directory
        ? rememberAdmittedConversation(
            directory,
            {} as AdmittedConversation,
            'reviewed_admitted_conversation_navigation'
          )
        : Promise.resolve({ status: 'unavailable' });
    },
    copied(id: unknown) {
      return directory
        ? resolveLocalConversation({ ...directory }, id)
        : Promise.resolve({ status: 'unavailable' });
    },
    async otherOwner(id: unknown) {
      const input = await f.otherNavigationInputs();
      if (!input.privateSession || !input.metadata)
        throw Error('no genuine other owner metadata');
      otherDirectory = captureConversationDirectory(
        input.privateSession,
        input.metadata,
        'reviewed_conversation_directory'
      );
      if (!otherDirectory) throw Error('no genuine other directory');
      return await resolveLocalConversation(otherDirectory, id);
    },
    async mutatedGenerationObservation() {
      const input = f.navigationInputs();
      if (!input.metadata || !input.room)
        throw Error('no genuine original metadata/room');
      await f.otherGeneration();
      const foreign = f.foreignNavigationSession();
      if (!foreign) throw Error('no genuine second live same-owner session');
      const local = messageMetadataOwnership(input.metadata),
        peer = privateSessionOwnership(foreign);
      if (!local || !peer || !peer.current() || !local.current())
        throw Error('no genuine current observations');
      const sameOwner = local.owner === peer.owner,
        differentGeneration = local.session !== peer.session;
      Reflect.set(local, 'session', peer.session);
      Reflect.set(local, 'current', () => true);
      const imported = captureConversationDirectory(
        foreign,
        input.metadata,
        'reviewed_conversation_directory'
      );
      const original = captureConversationDirectory(
        input.privateSession,
        input.metadata,
        'reviewed_conversation_directory'
      );
      const remembered = imported
        ? await rememberAdmittedConversation(
            imported,
            input.room,
            'reviewed_admitted_conversation_navigation'
          )
        : { status: 'unavailable' };
      if (imported) closeConversationDirectory(imported);
      if (original) closeConversationDirectory(original);
      return {
        sameOwner,
        differentGeneration,
        foreignAccepted: !!imported,
        originalAccepted: !!original,
        remembered,
        raw: await f.rawMetadata()
      };
    },
    async product(text: string) {
      await f.anotherWrap(true, text);
      if (f.cache() !== 'added') throw Error('no genuine second product rumor');
      return await this.remember();
    },
    close() {
      if (directory) closeConversationDirectory(directory);
      if (otherDirectory) closeConversationDirectory(otherDirectory);
      f.close();
    }
  };
}
