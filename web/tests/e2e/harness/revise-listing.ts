// HC_TEST_ONLY_RELAY/QUALIFICATION_PORT: actual signed retained source,
// original mounted editor and native operation rows. No live/operator Q.
import { createFixture as sourceFixture } from './edit-source.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import type { PublicOperationRecord } from '../../../src/lib/contracts/local-records.ts';
import { nextOwnedRevisionTime } from '../../../src/lib/publishing/revise-listing.ts';
import { mount, unmount } from 'svelte';
import ListingEditor from '../../../src/lib/components/ListingEditor.svelte';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  closeIdentityView,
  IDENTITY_VIEW_CONTEXT
} from '../../../src/lib/runtime/view-context.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  PUBLIC_RUNTIME_CONTEXT
} from '../../../src/lib/runtime/public-runtime.ts';
import { fixturePolicy } from './inbox-setup.ts';
import { makeFixture } from './approved-signing.ts';
export type ControlledSignInput = Parameters<
  ReturnType<typeof makeFixture>['sign']
>[0];

export async function createFixture() {
  const original = await sourceFixture();
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner;
  let release = () => {},
    restore = () => {},
    signRequests = 0;
  return {
    ...original,
    clock: nextOwnedRevisionTime,
    async signOriginal(input: ControlledSignInput) {
      const provider = window.nostr as
        | {
            signEvent(
              input: ControlledSignInput
            ): Promise<ReturnType<ReturnType<typeof makeFixture>['sign']>>;
          }
        | undefined;
      if (!provider) throw Error('original provider missing');
      return await provider.signEvent(input);
    },
    holdNextSignature() {
      const provider = window.nostr as
        | {
            signEvent(
              input: Parameters<ReturnType<typeof makeFixture>['sign']>[0]
            ): Promise<ReturnType<ReturnType<typeof makeFixture>['sign']>>;
          }
        | undefined;
      if (!provider) throw Error('original provider missing');
      const sign = provider.signEvent.bind(provider);
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      provider.signEvent = async (input) => {
        signRequests++;
        await held;
        return sign(input);
      };
      restore = () => {
        provider.signEvent = sign;
      };
    },
    refuseNextSignature() {
      const provider = window.nostr as
        | {
            signEvent(
              input: Parameters<ReturnType<typeof makeFixture>['sign']>[0]
            ): Promise<ReturnType<ReturnType<typeof makeFixture>['sign']>>;
          }
        | undefined;
      if (!provider) throw Error('original provider missing');
      const sign = provider.signEvent.bind(provider);
      let refused = false;
      provider.signEvent = async (input) => {
        signRequests++;
        if (!refused) {
          refused = true;
          throw Error('controlled refusal after actual durable signing marker');
        }
        return sign(input);
      };
      restore = () => {
        provider.signEvent = sign;
      };
    },
    signRequests: () => signRequests,
    releaseSignature() {
      release();
    },
    async operations() {
      const transaction = browserDatabaseTransaction(
        database,
        ['public_operations'],
        'readonly'
      );
      const request = transaction.objectStore('public_operations').getAll();
      return await new Promise<PublicOperationRecord[]>((resolve, reject) => {
        transaction.addEventListener('complete', () =>
          resolve(
            (request.result as { owner: string; wire: string }[])
              .filter((row) => row.owner === original.owner)
              .map((row) => JSON.parse(row.wire) as PublicOperationRecord)
          )
        );
        transaction.addEventListener('abort', () =>
          reject(Error('actual native operation read failed'))
        );
      });
    },
    async close() {
      release();
      restore();
      closeBrowserDatabase(database);
      await original.close();
    }
  };
}

export async function openSecondEditor(owner: string, naddr: string) {
  const context = createIdentityViewContext();
  mountIdentityView(context);
  const connected = await connectIdentityView(context);
  if (
    connected.identity.state === 'guest' ||
    connected.identity.state === 'pending' ||
    connected.identity.publicKey !== owner
  )
    throw Error('actual second-tab owner mismatch');
  const publicContext = createPublicRuntimeContext();
  mountPublicRuntime(publicContext, fixturePolicy());
  const target = document.createElement('div');
  target.id = 'hcp119-second';
  document.body.append(target);
  const editor = mount(ListingEditor, {
    target,
    props: { route: { mode: 'edit', naddr } },
    context: new Map<string, unknown>([
      [IDENTITY_VIEW_CONTEXT, context],
      [PUBLIC_RUNTIME_CONTEXT, publicContext]
    ])
  });
  return {
    async close() {
      await unmount(editor);
      target.remove();
      closePublicRuntime(publicContext);
      closeIdentityView(context);
    }
  };
}
