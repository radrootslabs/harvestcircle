// HC_TEST_ONLY_RELAY/QUALIFICATION_PORT: actual original repositories, signed
// events, anonymous SDK resolver and native IndexedDB; no live/operator Q.
import { mount, unmount } from 'svelte';
import ConflictNotice from '../../../src/lib/components/ConflictNotice.svelte';
import ListingEditor from '../../../src/lib/components/ListingEditor.svelte';
import { decodeProductReference } from '../../../src/lib/nostr/references.ts';
import {
  openOwnedEditSource,
  ownedEditSourceSnapshot,
  authorizeOwnedEditSource,
  closeOwnedEditSource,
  type OwnedEditSource
} from '../../../src/lib/publishing/edit-source.ts';
import { createFixture as originalFixture } from './listing-operation-view.ts';
import { makeFixture } from './approved-signing.ts';
import { fixturePolicy } from './inbox-setup.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  closeIdentityView,
  disconnectIdentityView,
  IDENTITY_VIEW_CONTEXT
} from '../../../src/lib/runtime/view-context.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  PUBLIC_RUNTIME_CONTEXT,
  createPublicView,
  beginPublicViewRun,
  subscribePublicView,
  disposePublicView,
  type PublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import { createPublicPublicationRepository } from '../../../src/lib/persistence/public-publications.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import { projectFoodParts } from '../../../src/lib/contracts/food-availability-v1/read.ts';
import type { PublicDraftForm } from '../../../src/lib/contracts/local-records.ts';
export async function createFixture(
  flags = { postingEnabled: true, messagingEnabled: true }
) {
  const original = await originalFixture();
  if ((await original.signed()) !== 'prepared')
    throw Error('actual original signature missing');
  const row = await original.row();
  if (!row.artifact) throw Error('actual artifact missing');
  const event = JSON.parse(row.artifact.wire) as ReturnType<
    ReturnType<typeof makeFixture>['sign']
  >;
  const projected = projectFoodParts(event);
  if (projected.outcome !== 'focused')
    throw Error('actual supported source missing');
  const naddr = encodeProductReference({
    kind: 30402,
    pubkey: row.owner,
    identifier: projected.projection.identifier
  });
  if (!naddr) throw Error('actual coordinate missing');
  const context = createIdentityViewContext();
  mountIdentityView(context);
  await connectIdentityView(context);
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner,
    repository = createPublicPublicationRepository(database, row.owner);
  if (!repository) throw Error('actual repository missing');
  const publicContext = createPublicRuntimeContext();
  const runtime = mountPublicRuntime(publicContext, fixturePolicy(flags));
  if (!runtime) throw Error('actual runtime missing');
  let source: OwnedEditSource | undefined,
    panel: ReturnType<typeof mount> | undefined,
    target: HTMLElement | undefined;
  const cancellation = new AbortController();
  const lookups: PublicView[] = [];
  return {
    event,
    naddr,
    owner: row.owner,
    counts: () => original.counts(),
    async open(network = true, address = naddr) {
      source = await openOwnedEditSource(
        repository,
        context,
        address,
        network ? runtime : undefined,
        cancellation.signal
      );
      return source && ownedEditSourceSnapshot(source);
    },
    snapshot: (copy = false) =>
      source && ownedEditSourceSnapshot(copy ? { ...source } : source),
    authorized: (copy = false) =>
      !!(source && authorizeOwnedEditSource(copy ? { ...source } : source)),
    copiedClose() {
      if (source) closeOwnedEditSource({ ...source });
    },
    async external() {
      await original.externalSource();
    },
    async competing(
      title = 'Actual current carrots',
      status: 'active' | 'sold' = 'active'
    ) {
      const provider = window.nostr as
        | {
            signEvent: (
              input: Parameters<ReturnType<typeof makeFixture>['sign']>[0]
            ) => Promise<ReturnType<ReturnType<typeof makeFixture>['sign']>>;
          }
        | undefined;
      if (!provider) throw Error('original controlled provider missing');
      return await provider.signEvent({
        kind: event.kind,
        content: event.content,
        created_at: event.created_at + 1,
        tags: event.tags.map((tag) =>
          tag[0] === 'title'
            ? ['title', title]
            : tag[0] === 'status'
              ? ['status', status]
              : tag
        )
      });
    },
    foreign() {
      return encodeProductReference({
        kind: 30402,
        pubkey: makeFixture().owner,
        identifier: projected.projection.identifier
      })!;
    },
    async render(proposed: PublicDraftForm) {
      if (!source) throw Error('actual source missing');
      if (panel) await unmount(panel);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp118-conflict';
      document.body.append(target);
      panel = mount(ConflictNotice, { target, props: { source, proposed } });
    },
    async renderEditor() {
      if (panel) await unmount(panel);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp118-conflict';
      document.body.append(target);
      panel = mount(ListingEditor, {
        target,
        props: { route: { mode: 'edit', naddr } },
        context: new Map<string, unknown>([
          [IDENTITY_VIEW_CONTEXT, context],
          [PUBLIC_RUNTIME_CONTEXT, publicContext]
        ])
      });
    },
    lookupAgain() {
      const reference = decodeProductReference(naddr);
      if (!reference) throw Error('actual reference missing');
      const view = createPublicView(runtime);
      lookups.push(view);
      const run = beginPublicViewRun(view);
      subscribePublicView(
        view,
        run,
        'head',
        [
          {
            kinds: [30402],
            authors: [reference.pubkey],
            '#d': [reference.identifier],
            limit: 20
          }
        ],
        () => {}
      );
    },
    async drafts() {
      const transaction = browserDatabaseTransaction(
        database,
        ['public_drafts'],
        'readonly'
      );
      const request = transaction.objectStore('public_drafts').getAll();
      return await new Promise<
        { owner: string; id: string; form: PublicDraftForm }[]
      >((resolve, reject) => {
        transaction.addEventListener('complete', () =>
          resolve(
            (request.result as { owner: string; wire: string }[])
              .filter((row) => row.owner === original.owner)
              .map(
                (row) =>
                  JSON.parse(row.wire) as {
                    owner: string;
                    id: string;
                    form: PublicDraftForm;
                  }
              )
          )
        );
        transaction.addEventListener('abort', () =>
          reject(Error('actual native draft read failed'))
        );
      });
    },
    cancel() {
      cancellation.abort();
    },
    disconnect() {
      disconnectIdentityView(context);
    },
    async close() {
      cancellation.abort();
      if (source) closeOwnedEditSource(source);
      if (panel) await unmount(panel);
      target?.remove();
      for (const view of lookups) disposePublicView(view);
      closePublicRuntime(publicContext);
      closeIdentityView(context);
      closeBrowserDatabase(database);
      original.close();
    }
  };
}
