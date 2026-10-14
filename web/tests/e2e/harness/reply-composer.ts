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
import {
  inboxSetupViewOwnership,
  inboxSetupViewSnapshot
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import { browserDatabaseTransaction } from '../../../src/lib/persistence/database.ts';
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
  const manager = navigator.locks;
  const originalDescriptor = Object.getOwnPropertyDescriptor(
    manager,
    'request'
  );
  const originalRequest = Reflect.get(manager, 'request');
  let crossOwnerSecond: string | undefined;
  let nativeTransactions: {
    stores: string[];
    outcome: 'abort' | 'complete';
  }[] = [];
  let restoreTransaction = () => {};
  let commandIds: string[] = [];
  let lockAttempts: { name: string; ifAvailable: boolean; granted: boolean }[] =
    [];
  const observedRequest = ((...args: unknown[]) => {
    const callbackIndex = args.length - 1;
    const callback = args[callbackIndex] as (lock: Lock | null) => unknown;
    const options = args.length === 3 ? (args[1] as LockOptions) : undefined;
    const forwarded = args.slice();
    forwarded[callbackIndex] = (lock: Lock | null) => {
      lockAttempts = lockAttempts.concat({
        name: String(args[0]),
        ifAvailable: options?.ifAvailable === true,
        granted: lock !== null
      });
      if (lock && options?.ifAvailable && args[0] === crossOwnerSecond) {
        crossOwnerSecond = undefined;
        const capturedSecond = Math.floor(clockSamples.at(-1)!.value / 1000);
        return (async () => {
          await new Promise<void>((resolve) => {
            function forwardAfterBoundary() {
              if (Math.floor(originalNow() / 1000) === capturedSecond) {
                window.setTimeout(forwardAfterBoundary, 1);
                return;
              }
              resolve();
            }
            window.setTimeout(
              forwardAfterBoundary,
              Math.max(1, (capturedSecond + 1) * 1000 - originalNow() + 1)
            );
          });
          return callback(lock);
        })();
      }
      return callback(lock);
    };
    return Reflect.apply(
      originalRequest,
      manager,
      forwarded
    ) as Promise<unknown>;
  }) as typeof manager.request;
  manager.request = observedRequest;
  const originalNowDescriptor = Object.getOwnPropertyDescriptor(Date, 'now');
  const originalNow = Reflect.get(Date, 'now');
  let clockSamples: { value: number; stack: string | undefined }[] = [];
  const observedNow = () => {
    const value = originalNow();
    clockSamples = clockSamples.concat({
      value,
      stack: new Error().stack?.split('\n').slice(1, 5).join('\n')
    });
    return value;
  };
  Date.now = observedNow;
  function restoreSendClock() {
    crossOwnerSecond = undefined;
    restoreTransaction();
    restoreTransaction = () => {};
    if (Date.now === observedNow && originalNowDescriptor)
      Object.defineProperty(Date, 'now', originalNowDescriptor);
  }

  let clockOffset = 0,
    alive = true;
  let nextTimer = 0;
  const timers = new Map<number, { at: number; callback: () => void }>();
  const ownSchedulers: ReturnType<typeof createPublicScheduler>[] = [];
  const clock: RequestClock = {
    now: () => clockOffset,
    schedule(callback, delay) {
      const id = ++nextTimer;
      timers.set(id, { at: clockOffset + delay, callback });
      return () => timers.delete(id);
    }
  };
  function advanceClock() {
    const target = clockOffset + 15_001;
    for (;;) {
      const next = [...timers]
        .filter(([, timer]) => timer.at <= target)
        .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
      if (!next) break;
      const [id, timer] = next;
      timers.delete(id);
      clockOffset = timer.at;
      timer.callback();
    }
    clockOffset = target;
  }
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
  let clicks = 0;
  target.addEventListener(
    'click',
    () => {
      clicks++;
    },
    { capture: true }
  );
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
    diagnostic: async () => ({
      setup: inboxSetupViewSnapshot(scope?.setup),
      locks: await navigator.locks.query(),
      lockAttempts: lockAttempts.slice(),
      clicks,
      clockSamples: clockSamples.slice(),
      nativeTransactions: nativeTransactions.slice(),
      commandIds: commandIds.slice()
    }),
    resetDiagnostics: () => {
      lockAttempts = [];
      clicks = 0;
      clockSamples = [];
      nativeTransactions = [];
      commandIds = [];
    },
    configureSendClock(crossSecond: boolean) {
      if (!scope) throw Error('no original reply scope');
      crossOwnerSecond = crossSecond
        ? 'harvestcircle:owner:' + scope.owner
        : undefined;
      Date.now = observedNow;
      const db = browserDatabaseTransaction(
        scope.database,
        ['private_sends'],
        'readonly'
      ).db;
      const descriptor = Object.getOwnPropertyDescriptor(db, 'transaction');
      const original = Reflect.get(db, 'transaction');
      const uuidDescriptor = Object.getOwnPropertyDescriptor(
        crypto,
        'randomUUID'
      );
      const originalUuid = Reflect.get(crypto, 'randomUUID');
      const observedUuid = () => {
        const id = Reflect.apply(originalUuid, crypto, []);
        commandIds = commandIds.concat(id);
        return id;
      };
      crypto.randomUUID = observedUuid;
      const observed = ((...args: Parameters<IDBDatabase['transaction']>) => {
        const tx = Reflect.apply(original, db, args);
        if (
          tx.mode === 'readwrite' &&
          tx.objectStoreNames.contains('private_sends')
        ) {
          for (const outcome of ['abort', 'complete'] as const)
            tx.addEventListener(outcome, () => {
              nativeTransactions = nativeTransactions.concat({
                stores: Array.from(tx.objectStoreNames),
                outcome
              });
            });
        }
        return tx;
      }) as typeof db.transaction;
      db.transaction = observed;
      restoreTransaction = () => {
        if (db.transaction === observed) {
          if (descriptor) Object.defineProperty(db, 'transaction', descriptor);
          else Reflect.deleteProperty(db, 'transaction');
        }
        if (crypto.randomUUID === observedUuid) {
          if (uuidDescriptor)
            Object.defineProperty(crypto, 'randomUUID', uuidDescriptor);
          else Reflect.deleteProperty(crypto, 'randomUUID');
        }
      };
    },
    restoreSendClock,
    text: () => replyText(controller),
    native: thread.native,
    delta: thread.delta,
    list: () => inboxListSnapshot(thread.controller),
    lookups: () => lookups,
    advance: advanceClock,
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
      restoreSendClock();
      release?.();
      provider.signEvent = nativeSign;
      await closeComposer();
      if (publicContext) closePublicRuntime(publicContext);
      await thread.close();
      if (manager.request === observedRequest) {
        if (originalDescriptor)
          Object.defineProperty(manager, 'request', originalDescriptor);
        else Reflect.deleteProperty(manager, 'request');
      }
      if (Date.now === observedNow && originalNowDescriptor)
        Object.defineProperty(Date, 'now', originalNowDescriptor);
      alive = false;
      for (const scheduler of ownSchedulers) closePublicScheduler(scheduler);
      timers.clear();
    }
  };
}
