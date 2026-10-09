import { mount, unmount } from 'svelte';
import type { beforeNavigate as KitBeforeNavigate } from '$app/navigation';
type BeforeNavigate = Parameters<Parameters<typeof KitBeforeNavigate>[0]>[0];
import {
  finalizeEvent,
  type UnsignedEvent,
  type NostrEvent
} from 'applesauce-core/helpers';
import { renderFixture as renderThread } from './conversation-thread.ts';
import Panel from './reply-composer-panel.svelte';
import {
  createReplyController,
  createRuntimeReplyController,
  replySnapshot,
  replyText,
  updateReplyText,
  sendReply,
  closeReplyController,
  requestReplyNavigation,
  keepReplyEditing,
  discardReplyNavigation,
  selectReplyParent
} from '../../../src/lib/messaging/reply-controller.ts';
import {
  inboxListSnapshot,
  inboxReplyOwnership
} from '../../../src/lib/messaging/inbox-view.ts';
import {
  captureAuthenticatedReplyRumor,
  rumorPlanSnapshot,
  stopRumorPlan
} from '../../../src/lib/messaging/rumor-plan.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import { inboxSetupViewOwnership } from '../../../src/lib/messaging/inbox-setup-view.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler,
  type RequestClock
} from '../../../src/lib/nostr/request-scope.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';

const navigation: ((event: BeforeNavigate) => void)[] = [];
let destinations = Array.from<string>([]);
export function beforeNavigate(callback: (event: BeforeNavigate) => void) {
  navigation.push(callback);
}
export function goto(destination: string | URL) {
  destinations = destinations.concat(String(destination));
  return Promise.resolve();
}
export async function renderReplyFixture(qualified = true, production = false) {
  navigation.length = 0;
  destinations = [];
  let clockOffset = 0,
    alive = true;
  const ownSchedulers: ReturnType<typeof createPublicScheduler>[] = [];
  const clock: RequestClock = {
    now: () => performance.now() + clockOffset,
    schedule(callback, delay) {
      const timer = setTimeout(callback, delay);
      return () => clearTimeout(timer);
    }
  };
  const freshOwn: NonNullable<Parameters<typeof renderThread>[3]> = (
    owner,
    policy
  ) => {
    const event = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [['relay', 'wss://archive.example.org']],
        content: ''
      },
      new Uint8Array(32).fill(83)
    );
    if (event.pubkey !== owner) throw Error('wrong genuine owner');
    const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
    ownSchedulers.push(scheduler);
    const run = createPublicRun(scheduler, policy);
    return Promise.resolve(
      createInboxResolver(
        run,
        owner,
        (next) =>
          openInboxRequest(
            run,
            owner,
            (sink) => {
              sink({
                type: 'EVENT',
                from: 'wss://discovery.example.org',
                id: 'fixture',
                event
              });
              sink({
                type: 'EOSE',
                from: 'wss://discovery.example.org',
                id: 'fixture'
              });
              return () => {};
            },
            next
          ),
        () => alive
      )
    );
  };
  const thread = await renderThread(
    'inbound',
    undefined,
    qualified,
    production ? freshOwn : undefined
  );
  await thread.checkSetup();
  if (!(await thread.unlock())) throw Error('no genuine unlocked thread');
  let gate = false,
    lookups = 0;
  const scope = inboxReplyOwnership(
    thread.controller,
    thread.conversationId,
    thread.firstRumor
  );
  const setup = scope && inboxSetupViewOwnership(scope.setup);
  const publicContext = production ? createPublicRuntimeContext() : undefined;
  const runtime =
    publicContext &&
    setup &&
    mountPublicRuntime(publicContext, setup.policy, clock);
  if (production && !runtime) throw Error('no genuine public runtime');
  const candidate = runtime
    ? createRuntimeReplyController(
        thread.controller,
        thread.conversationId,
        thread.firstRumor,
        runtime
      )
    : createReplyController(
        thread.controller,
        thread.conversationId,
        thread.firstRumor,
        {
          lookupPeer: (peer) => {
            lookups++;
            if (gate) throw Error('controlled same-page setup cancellation');
            return Promise.resolve(thread.replyLookup(peer));
          }
        }
      );
  if (!candidate) throw Error('no original reply controller');
  const controller = candidate;
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, { target, props: { controller } });
  let unmounted = false,
    held = false,
    release: (() => void) | undefined;
  const provider = window.nostr as
    { signEvent(event: UnsignedEvent): Promise<NostrEvent> } | undefined;
  if (!provider) throw Error('no actual fixture provider');
  const nativeSign = provider.signEvent.bind(provider);
  provider.signEvent = (event: UnsignedEvent) =>
    held
      ? new Promise<NostrEvent>((resolve, reject) => {
          release = () => {
            held = false;
            nativeSign(event).then(resolve, reject);
          };
        })
      : nativeSign(event);
  async function closeComposer() {
    if (unmounted) return;
    unmounted = true;
    await unmount(mounted);
    closeReplyController(controller);
    target.remove();
  }
  return {
    owner: thread.owner,
    peer: thread.peer,
    parent: thread.firstRumor,
    conversationId: thread.conversationId,
    snapshot: () => replySnapshot(controller),
    text: () => replyText(controller),
    native: thread.native,
    delta: thread.delta,
    list: () => inboxListSnapshot(thread.controller),
    lookups: () => lookups,
    advance: () => {
      clockOffset += 15_001;
    },
    refreshSetup: thread.checkSetup,
    gate: (value: boolean) => {
      gate = value;
    },
    send: (review: unknown = 'reviewed_reply_send') =>
      sendReply(controller, review),
    update: (text: string, review: unknown = 'reviewed_private_text') =>
      updateReplyText(controller, text, review),
    selectParent: (parent: string) =>
      selectReplyParent(controller, parent, 'reviewed_reply_parent'),
    copied: async () => ({
      snapshot: replySnapshot({ ...controller }),
      text: replyText({ ...controller }),
      update: updateReplyText(
        { ...controller },
        'copied sentinel',
        'reviewed_private_text'
      ),
      send: await sendReply({ ...controller }, 'reviewed_reply_send'),
      navigation: requestReplyNavigation({ ...controller }),
      keep: keepReplyEditing({ ...controller }),
      discard: discardReplyNavigation(
        { ...controller },
        'reviewed_discard_private_text'
      )
    }),
    async admission() {
      const scope = inboxReplyOwnership(
        thread.controller,
        thread.conversationId,
        thread.firstRumor
      );
      if (!scope) throw Error('missing actual original room');
      const capture = (
        identity = scope.identity,
        session = scope.session,
        unlocked = scope.unlocked,
        room = scope.room
      ) =>
        captureAuthenticatedReplyRumor(
          identity,
          session,
          unlocked,
          room,
          'Local admission fixture only',
          Math.floor(Date.now() / 1000)
        );
      const genuine = capture();
      if (!genuine) throw Error('missing genuine plan');
      const wire = rumorPlanSnapshot(genuine)?.wire;
      stopRumorPlan(genuine);
      const copied = [
        capture({ ...scope.identity }),
        capture(scope.identity, { ...scope.session }),
        capture(scope.identity, scope.session, { ...scope.unlocked }),
        capture(scope.identity, scope.session, scope.unlocked, {
          ...scope.room
        })
      ].map((plan) => !!plan);
      const other = createIdentitySession();
      await connectIdentity(other);
      await probeIdentityMessaging(other, 'reviewed_self_copy');
      const foreign = capture(other);
      if (foreign) stopRumorPlan(foreign);
      disconnectIdentity(other);
      return { wire, copied, foreign: !!foreign };
    },
    navigate() {
      let cancelled = false;
      const event = {
        cancel: () => {
          cancelled = true;
        },
        to: { url: new URL('/messages', window.location.origin) },
        willUnload: false
      } as unknown as BeforeNavigate;
      for (const callback of navigation) callback(event);
      return cancelled;
    },
    destinations: () => destinations.slice(),
    holdSigner: () => {
      held = true;
    },
    pending: () => !!release,
    settle: () => release?.(),
    closeComposer,
    logout: thread.logout,
    async close() {
      release?.();
      provider.signEvent = nativeSign;
      await closeComposer();
      if (publicContext) closePublicRuntime(publicContext);
      await thread.close();
      alive = false;
      for (const scheduler of ownSchedulers) closePublicScheduler(scheduler);
    }
  };
}
