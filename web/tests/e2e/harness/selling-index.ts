// HC_TEST_ONLY_RELAY/QUALIFICATION_PORT: original installed provider, native
// IndexedDB and bounded SDK loopback. Controlled observations are not live Q.
import { mount, unmount } from 'svelte';
import Selling from '../../../src/routes/selling/+page.svelte';
import { createFixture as publicationFixture } from './listing-operation-view.ts';
import { makeFixture } from './approved-signing.ts';
import { fixturePolicy } from './inbox-setup.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  disconnectIdentityView,
  closeIdentityView,
  IDENTITY_VIEW_CONTEXT
} from '../../../src/lib/runtime/view-context.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  PUBLIC_RUNTIME_CONTEXT
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicDraftRepository,
  createPublicDraft,
  readPublicDraft,
  savePublicDraft
} from '../../../src/lib/persistence/drafts.ts';
import type { PublicDraftForm } from '../../../src/lib/contracts/local-records.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
import { buildFoodTemplate } from '../../../src/lib/contracts/food-availability-v1/write.ts';

export async function createFixture(mode: 'empty' | 'retained' = 'retained') {
  const original = mode === 'retained' ? await publicationFixture() : undefined;
  const signer = mode === 'empty' ? makeFixture() : undefined;
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  if (signer)
    Object.defineProperty(window, 'nostr', {
      configurable: true,
      value: { getPublicKey: () => Promise.resolve(signer.owner) }
    });
  if (original && (await original.signed()) !== 'prepared')
    throw Error('original signed source missing');
  const row = await original?.row();
  const event = row?.artifact
    ? (JSON.parse(row.artifact.wire) as NostrEvent)
    : undefined;
  const owner = original?.owner ?? signer?.owner;
  if (!owner) throw Error('controlled owner missing');
  const context = createIdentityViewContext();
  mountIdentityView(context);
  await connectIdentityView(context);
  const publicContext = createPublicRuntimeContext();
  mountPublicRuntime(publicContext, fixturePolicy());
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner;
  let panel: ReturnType<typeof mount> | undefined;
  let target: HTMLElement | undefined;
  let switched: ReturnType<typeof makeFixture> | undefined;
  return {
    owner,
    event,
    command: original?.command,
    remoteListing() {
      if (!signer) throw Error('remote-only installed owner missing');
      const now = Math.floor(Date.now() / 1000);
      const built = buildFoodTemplate({
        identifier: 'remote-only-food',
        title: 'Remote own eggs',
        description: 'Full external own listing description',
        location: 'External farm',
        amount: '6',
        currency: 'CAD',
        unit: 'each',
        created_at: now,
        published_at: now,
        status: 'active'
      });
      if (!built.ok) throw Error('focused external fixture invalid');
      return signer.sign({
        kind: 30402,
        created_at: now,
        tags: built.wire_parts.tags.map((tag) => [...tag]),
        content: built.wire_parts.content
      });
    },
    async switchOwner() {
      switched = makeFixture();
      const next = switched.owner;
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        value: { getPublicKey: () => Promise.resolve(next) }
      });
      await connectIdentityView(context);
      return next;
    },
    async render() {
      if (panel) await unmount(panel);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp121-selling';
      document.body.append(target);
      panel = mount(Selling, {
        target,
        context: new Map<string, unknown>([
          [IDENTITY_VIEW_CONTEXT, context],
          [PUBLIC_RUNTIME_CONTEXT, publicContext]
        ])
      });
    },
    async external() {
      await original?.externalSource();
      const next = await original?.row();
      return next?.artifact
        ? (JSON.parse(next.artifact.wire) as NostrEvent)
        : undefined;
    },
    async observed(
      status: 'accepted' | 'refused' | 'timed_out',
      target: number,
      readback: boolean
    ) {
      await original?.evidence(status, target, readback);
    },
    async settled() {
      if (!original) throw Error('original operation missing');
      const current = await original.row();
      for (let i = 0; i < current.capture.targets.length; i++)
        await original.evidence('accepted', i, true);
    },
    async draft(title = 'Unfinished spinach') {
      const repository = createPublicDraftRepository(database, owner);
      if (!repository) throw Error('original draft repository missing');
      const form: PublicDraftForm = {
        title,
        description: '',
        location: '',
        amount: '',
        currency: '',
        unit: '',
        quantity: '',
        contactType: '',
        contactValue: ''
      };
      const saved = await createPublicDraft(repository, form);
      if (!saved.ok) throw Error(saved.reason);
      return saved.value;
    },
    async changeCapturedDraft() {
      const row = await original?.row();
      if (!row || row.source.type !== 'draft')
        throw Error('original draft capture missing');
      const repository = createPublicDraftRepository(database, owner);
      if (!repository) throw Error('original draft repository missing');
      const read = await readPublicDraft(repository, row.source.id);
      if (!read.ok) throw Error(read.reason);
      const saved = await savePublicDraft(
        repository,
        read.value.id,
        read.value.revision,
        {
          ...read.value.form,
          title: 'Unfinished proposed carrots',
          description: ''
        }
      );
      if (!saved.ok) throw Error(saved.reason);
      return saved.value;
    },
    async records() {
      const transaction = browserDatabaseTransaction(
        database,
        ['public_drafts', 'public_operations'],
        'readonly'
      );
      const drafts = transaction.objectStore('public_drafts').getAll();
      const operations = transaction.objectStore('public_operations').getAll();
      return await new Promise<{ drafts: unknown[]; operations: unknown[] }>(
        (resolve, reject) => {
          transaction.addEventListener('complete', () =>
            resolve({ drafts: drafts.result, operations: operations.result })
          );
          transaction.addEventListener('abort', () =>
            reject(Error('native seller read failed'))
          );
        }
      );
    },
    counts: () => original?.counts() ?? { signs: 0, keys: 0 },
    reconnect() {
      return connectIdentityView(context);
    },
    disconnect() {
      disconnectIdentityView(context);
    },
    async close() {
      if (panel) await unmount(panel);
      target?.remove();
      closePublicRuntime(publicContext);
      closeIdentityView(context);
      closeBrowserDatabase(database);
      original?.close();
      signer?.close();
      switched?.close();
      if (signer) {
        if (prior) Object.defineProperty(window, 'nostr', prior);
        else Reflect.deleteProperty(window, 'nostr');
      }
    }
  };
}
