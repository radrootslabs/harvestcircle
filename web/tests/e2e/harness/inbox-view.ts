import { mount, unmount } from 'svelte';
import Panel from './inbox-view-panel.svelte';
import { makeFixture as makeQueueFixture } from './decryption-queue.ts';
import { makeSetupFixture, fixturePolicy, inbox } from './inbox-setup.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  inboxSetupViewSnapshot,
  inboxSetupViewOwnership,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  createInboxView,
  inboxViewSnapshot,
  unlockInboxView,
  checkInboxView,
  readNewInboxView,
  stopInboxView,
  closeInboxView
} from '../../../src/lib/messaging/inbox-view.ts';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import { identityMessagingOwnership } from '../../../src/lib/runtime/identity-session.ts';
export { makeSetupFixture, inboxViewSnapshot };
export async function renderFixture(count = 21, qualified = false) {
  const f = await makeQueueFixture(count);
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('missing controlled policy');
  let access = true,
    closedSetups = 0,
    failSetupCleanup = false;
  const original = identityMessagingOwnership(f.identity);
  if (!original) throw Error('missing original fixture identity');
  const setup = createInboxSetupView({
    identity: f.identity,
    policy,
    close: () => {
      closedSetups++;
      if (failSetupCleanup && closedSetups === 1)
        throw Error('controlled original setup disposal failure');
    },
    lookup: () => Promise.resolve(f.context.own),
    observeAccess: qualified
      ? (context) => ({
          ...context,
          receive: 'qualified_exercised',
          archive: 'qualified_exercised',
          current: () => access && original.current()
        })
      : undefined
  });
  if (!setup) throw Error('missing original setup');
  const controller = createInboxView({ identity: f.identity, setup });
  if (!controller) throw Error('missing original inbox view');
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, { target, props: { controller } });
  const baseline = f.counts();
  return {
    controller,
    setup,
    snapshot: () => inboxViewSnapshot(controller),
    setupSnapshot: () => inboxSetupViewSnapshot(setup),
    closeInvalidatedPage() {
      f.disconnect();
      const closed = closeInboxView(controller);
      return {
        closed,
        closedSetups,
        setupStatus: inboxSetupViewSnapshot(setup).status
      };
    },
    retryFailedSetupClose() {
      failSetupCleanup = true;
      const first = closeInboxView(controller);
      const incomplete = inboxViewSnapshot(controller).cleanupRequired;
      const second = closeInboxView(controller);
      return {
        first,
        incomplete,
        second,
        closedSetups,
        cleanupRequired: inboxViewSnapshot(controller).cleanupRequired
      };
    },
    checkSetup: () => checkInboxSetupView(setup),
    unlock: (review: unknown = 'reviewed_messages_unlock') =>
      unlockInboxView(controller, review),
    check: (review: unknown = 'reviewed_foreground_inbox') =>
      checkInboxView(controller, review),
    next: (review: unknown = 'reviewed_decrypt_batch') =>
      readNewInboxView(controller, review),
    delta: () => ({
      keys: f.counts().keys - baseline.keys,
      decrypts: f.counts().decrypts - baseline.decrypts,
      encrypts: f.counts().encrypts - baseline.encrypts,
      signs: f.counts().signs - baseline.signs
    }),
    encryptedUnchanged: () => f.encryptedUnchanged(),
    pending: () => f.pending(),
    settle: () => f.settle(),
    mode: f.mode,
    scheduler: () => f.scheduler(),
    logout: () => f.disconnect(),
    revokeAccess: () => {
      access = false;
    },
    stop: () => stopInboxView(controller),
    copied: async () => ({
      unlock: await unlockInboxView(
        { ...controller },
        'reviewed_messages_unlock'
      ),
      check: await checkInboxView(
        { ...controller },
        'reviewed_foreground_inbox'
      ),
      next: await readNewInboxView({ ...controller }, 'reviewed_decrypt_batch')
    }),
    mutateOwnership() {
      const first = inboxSetupViewOwnership(setup);
      if (!first) throw Error('missing genuine observed ownership');
      Reflect.set(first, 'identity', {});
      Reflect.set(first, 'current', () => false);
      const second = inboxSetupViewOwnership(setup);
      return !!second && second.identity === f.identity && second.current();
    },
    async close() {
      f.settle();
      closeInboxView(controller);
      closeInboxSetupView(setup);
      await unmount(mounted);
      f.close();
      target.remove();
    }
  };
}
export async function renderSetupFixture(
  mode: 'missing' | 'incomplete' | 'compatible' = 'missing'
) {
  const f = await makeSetupFixture([['relay', inbox]], {}, mode === 'missing');
  const setup = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () =>
      Promise.resolve(f.resolve(undefined, mode !== 'incomplete').resolver)
  });
  if (!setup) throw Error('missing setup');
  const controller = createInboxView({ identity: f.identity, setup });
  if (!controller) throw Error('missing view');
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, { target, props: { controller } });
  return {
    controller,
    setup,
    snapshot: () => inboxViewSnapshot(controller),
    counts: f.counts,
    async close() {
      closeInboxView(controller);
      closeInboxSetupView(setup);
      await unmount(mounted);
      f.close();
      target.remove();
    }
  };
}

// This original standalone fixture does not exercise navigation. Any SSR or
// mounted regression action that attempts it must fail rather than succeed.
export function goto(): never {
  throw new Error('HCP101 standalone harness must not navigate');
}
