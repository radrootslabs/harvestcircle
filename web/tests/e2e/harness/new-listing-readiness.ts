import {
  newListingReadinessSnapshot,
  captureNewListingReadiness,
  newListingReadinessOwnership,
  closeNewListingReadiness
} from '../../../src/lib/publishing/new-listing-readiness.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  stopInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  disconnectIdentity,
  connectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicDraftRepository,
  listPublicDrafts
} from '../../../src/lib/persistence/drafts.ts';
import {
  createPublicDraftController,
  flushPublicDraft,
  publicDraftControllerSnapshot,
  closePublicDraftController
} from '../../../src/lib/publishing/draft-controller.ts';
import { makeSetupFixture, fixturePolicy, inbox } from './inbox-setup.ts';
export async function createFixture(
  mode: 'guest' | 'signing_only' | 'compatible_unqualified' | 'qualified'
) {
  const f = await makeSetupFixture([['relay', inbox]]);
  if (mode === 'guest') disconnectIdentity(f.identity);
  if (mode === 'signing_only') {
    disconnectIdentity(f.identity);
    const provider = window.nostr as {
      getPublicKey(): Promise<string>;
      signEvent(...args: unknown[]): Promise<never>;
    };
    Object.defineProperty(window, 'nostr', {
      configurable: true,
      value: {
        getPublicKey: () => provider.getPublicKey(),
        signEvent: (...args: Parameters<typeof provider.signEvent>) =>
          provider.signEvent(...args)
      }
    });
    await connectIdentity(f.identity);
  }
  // HC_TEST_ONLY_QUALIFICATION_PORT: actual original owners with controlled
  // exercised observations; this never qualifies real extensions or relays.
  const setup = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
    ...(mode === 'qualified'
      ? {
          observeAccess: (
            context: import('../../../src/lib/messaging/readiness.ts').OwnInboxAccessContext
          ) => ({
            ...context,
            readTargets: context.readTargets.slice(),
            archiveTargets: context.archiveTargets.slice(),
            receive: 'qualified_exercised' as const,
            archive: 'qualified_exercised' as const,
            current: () => true
          })
        }
      : {})
  });
  if (setup) {
    await checkInboxSetupView(setup);
    await unlockInboxSetupView(setup);
  }
  const baseline = f.counts();
  const proof = captureNewListingReadiness(
    f.identity,
    setup,
    'reviewed_new_listing_inbox'
  );
  const held = newListingReadinessOwnership(proof);
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const repository = createPublicDraftRepository(opened.owner, f.owner);
  if (!repository) throw Error('original draft repository unavailable');
  const form = {
    title: '',
    description: 'unfinished local public description',
    location: '',
    amount: '.',
    currency: '',
    unit: '',
    quantity: '',
    contactType: '' as const,
    contactValue: ''
  };
  const draft = await createPublicDraftController(repository, form);
  if (!draft) throw Error('original draft controller unavailable');
  return {
    snapshot() {
      return {
        readiness: newListingReadinessSnapshot(f.identity, setup),
        captured: !!proof,
        current: held?.current() ?? false,
        counts: f.counts(),
        baseline
      };
    },
    async save() {
      const saved = await flushPublicDraft(draft);
      return {
        saved,
        snapshot: publicDraftControllerSnapshot(draft),
        rows: await listPublicDrafts(repository),
        counts: f.counts(),
        baseline
      };
    },
    copies() {
      return {
        copiedSetup: !!captureNewListingReadiness(
          f.identity,
          setup && { ...setup },
          'reviewed_new_listing_inbox'
        ),
        copiedProof: !!newListingReadinessOwnership(proof && { ...proof }),
        genuine: held?.current() ?? false
      };
    },
    stop() {
      stopInboxSetupView(setup);
    },
    revoke() {
      if (proof) closeNewListingReadiness(proof);
    },
    close() {
      closePublicDraftController(draft);
      closeInboxSetupView(setup);
      closeBrowserDatabase(opened.owner);
      f.close();
    }
  };
}
