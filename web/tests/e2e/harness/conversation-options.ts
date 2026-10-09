import { mount, unmount, tick } from 'svelte';
import {
  renderFixture as renderThread,
  type Scenario
} from './conversation-thread.ts';
import Panel from './conversation-options-panel.svelte';
import {
  captureCitedProduct,
  citedProductSnapshot,
  closeCitedProduct,
  conversationOptionsSnapshot,
  requestConversationOption,
  type CitedProduct
} from '../../../src/lib/messaging/cited-product.ts';
import {
  inboxReplyOwnership,
  inboxMessageSnapshot,
  subscribeInboxView,
  closeInboxView
} from '../../../src/lib/messaging/inbox-view.ts';
import { inboxSetupViewOwnership } from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { neventEncode } from 'applesauce-core/helpers/pointers';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
export { beforeNavigate, goto } from './conversation-thread.ts';

export type OptionsScenario = Scenario | 'exact' | 'stale' | 'parent_reference';
export async function renderFixture(scenario: OptionsScenario = 'contexts') {
  const publicSecret = new Uint8Array(32).fill(84);
  const version = finalizeEvent(
    {
      kind: 30402,
      created_at: scenario === 'stale' ? 1700000050 : 1700000060,
      content: 'Independently signed carrots',
      tags: [
        ['d', 'carrots'],
        ['title', 'Verified carrots'],
        ['summary', 'Actual signed listing'],
        ['published_at', '1700000000'],
        ['location', 'Victoria'],
        ['price', '3.5', 'CAD'],
        ['radroots:price_unit', 'lb'],
        ['status', 'active']
      ]
    },
    publicSecret
  );
  const selectedCitation = encodeProductReference({
    kind: 30402,
    pubkey: getPublicKey(publicSecret),
    identifier: 'carrots'
  });
  const seeded = ['exact', 'stale', 'parent_reference'].includes(scenario);
  const included = neventEncode({
    id: scenario === 'parent_reference' ? 'f'.repeat(64) : version.id,
    author: version.pubkey,
    kind: 30402
  });
  const body = seeded
    ? 'About carrots\nProduct: nostr:' +
      selectedCitation +
      '\nVersion: nostr:' +
      included +
      '\nQuoted price: 4 GBP https://untrusted.example.org'
    : undefined;
  const kind: Scenario =
    scenario === 'parent_reference'
      ? 'hostile'
      : scenario === 'exact' || scenario === 'stale'
        ? 'inbound'
        : scenario;
  const thread = await renderThread(kind, undefined, false, undefined, body);
  await thread.checkSetup();
  if (!(await thread.unlock())) throw Error('no genuine thread');
  const message =
    thread
      .thread()
      .messages.find((row) =>
        row.citations.some((ref) => ref.naddr === thread.carrots)
      ) ?? thread.thread().messages[0];
  if (!message) throw Error('no original message');
  const scope = inboxReplyOwnership(
    thread.controller,
    thread.conversationId,
    message.data.rumorId
  );
  const setup = scope && inboxSetupViewOwnership(scope.setup);
  const context = createPublicRuntimeContext();
  let clockOffset = 0;
  const candidate =
    setup &&
    mountPublicRuntime(context, setup.policy, {
      now: () => performance.now() + clockOffset,
      schedule(callback, delay) {
        const timer = setTimeout(callback, delay);
        return () => clearTimeout(timer);
      }
    });
  if (!candidate) throw Error('no original runtime');
  const runtime = candidate;
  const hooks: { action: string; peer: string }[] = [];
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, {
    target,
    props: {
      controller: thread.controller,
      conversationId: thread.conversationId,
      runtime,
      message,
      hook: (action: string, peer: string) => {
        hooks.push({ action, peer });
      }
    }
  });
  const tokens: CitedProduct[] = [];
  const terminal: string[] = [];
  const terminalOff: (() => void)[] = [];
  let unmounted = false;
  function capture(
    value: unknown = thread.carrots,
    review: unknown = 'reviewed_product_citation'
  ) {
    const token = captureCitedProduct(
      thread.controller,
      thread.conversationId,
      message.data.rumorId,
      value,
      runtime,
      review
    );
    if (token) tokens.push(token);
    return token;
  }
  async function closePanel() {
    if (unmounted) return;
    unmounted = true;
    await unmount(mounted);
    target.remove();
  }
  return {
    currentMessage: () =>
      !!inboxMessageSnapshot(
        thread.controller,
        thread.conversationId,
        message.data.rumorId
      ),
    terminal: () => terminal.slice(),
    revokeAndClick: async (action: 'logout' | 'selection' | 'close') => {
      const button = Array.from(
        target.querySelectorAll<HTMLButtonElement>('button')
      ).find((row) => row.textContent === 'Reply to this message');
      if (!button) throw Error('no original rendered Reply control');
      if (action === 'logout') thread.logout();
      else if (action === 'selection')
        await thread.resolve('dddddddd-dddd-4ddd-8ddd-dddddddddddd');
      else {
        const revoked = () =>
          !inboxMessageSnapshot(
            thread.controller,
            thread.conversationId,
            message.data.rumorId
          );
        terminalOff.push(
          subscribeInboxView(thread.controller, () => {
            if (!revoked()) return;
            terminal.push('reentered');
            closeInboxView(thread.controller);
          })
        );
        terminalOff.push(
          subscribeInboxView(thread.controller, () => {
            if (revoked()) throw Error('controlled terminal observer failure');
          })
        );
        terminalOff.push(
          subscribeInboxView(thread.controller, () => {
            if (revoked()) terminal.push('survived');
          })
        );
        closeInboxView(thread.controller);
      }
      await tick();
      button.click();
    },
    advance: () => {
      clockOffset += 15_001;
    },
    owner: thread.owner,
    peer: thread.peer,
    parent: message.data.replyTo,
    rumorId: message.data.rumorId,
    citation: thread.carrots,
    citedVersion: version.id,
    delta: thread.delta,
    native: thread.native,
    hooks: () => hooks.slice(),
    options: () =>
      conversationOptionsSnapshot(thread.controller, thread.conversationId),
    capture: (value?: unknown, review?: unknown) => !!capture(value, review),
    snapshot: () => {
      const token = tokens.at(-1);
      return token && citedProductSnapshot(token);
    },
    copied() {
      const token = tokens.at(-1);
      return {
        view: !!captureCitedProduct(
          { ...thread.controller },
          thread.conversationId,
          message.data.rumorId,
          thread.carrots,
          runtime,
          'reviewed_product_citation'
        ),
        runtime: !!captureCitedProduct(
          thread.controller,
          thread.conversationId,
          message.data.rumorId,
          thread.carrots,
          { ...runtime },
          'reviewed_product_citation'
        ),
        snapshot: token && citedProductSnapshot({ ...token }),
        options: conversationOptionsSnapshot(
          { ...thread.controller },
          thread.conversationId
        ),
        hook: requestConversationOption(
          { ...thread.controller },
          thread.conversationId,
          'block',
          (peer) => hooks.push({ action: 'copied', peer }),
          'reviewed_conversation_option'
        )
      };
    },
    logout: thread.logout,
    closePanel,
    async close() {
      for (const off of terminalOff) off();
      await closePanel();
      for (const token of tokens) closeCitedProduct(token);
      closePublicRuntime(context);
      await thread.close();
    }
  };
}
