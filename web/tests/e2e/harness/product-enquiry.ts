import { mount, unmount, tick } from 'svelte';
import Panel from './product-enquiry-panel.svelte';
import { renderFixture as renderThread } from './conversation-thread.ts';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  createProductView,
  productViewSnapshot,
  closeProductView,
  type ProductViewSnapshot
} from '../../../src/lib/catalog/product-view.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import { getPublicKey } from 'applesauce-core/helpers';
import {
  captureProductEnquiry,
  productEnquirySnapshot,
  productEnquiryMessage,
  updateProductEnquiry,
  reuseProductEnquiryConversation,
  closeProductEnquiry,
  type ProductEnquiry
} from '../../../src/lib/messaging/product-enquiry.ts';
import type { InboxView } from '../../../src/lib/messaging/inbox-view.ts';
export { beforeNavigate, goto } from './conversation-thread.ts';

export async function renderFixture(existing = false) {
  const thread = existing ? await renderThread('inbound') : undefined;
  if (thread) {
    await thread.checkSetup();
    if (!(await thread.unlock())) throw Error('original pair unavailable');
  }
  const preparationCounts = thread?.delta() ?? {
    decrypts: 0,
    signs: 0,
    encrypts: 0
  };
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: false,
          nip50: false
        }
      ],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  if (!policy) throw Error('invalid fixed test policy');
  const context = createPublicRuntimeContext();
  let offset = 0;
  const runtime = mountPublicRuntime(context, policy, {
    now: () => performance.now() + offset,
    schedule(callback, delay) {
      const timer = setTimeout(callback, delay);
      return () => clearTimeout(timer);
    }
  });
  if (!runtime) throw Error('original runtime unavailable');
  const reference = encodeProductReference({
    kind: 30402,
    pubkey: getPublicKey(new Uint8Array(32).fill(84)),
    identifier: 'carrots'
  });
  if (!reference) throw Error('invalid public coordinate');
  const observers = new Set<(value: ProductViewSnapshot) => void>();
  const view = createProductView(
    createPublicView(runtime),
    reference,
    { nowSeconds: () => 1700000200 },
    (value) => {
      for (const callback of observers) callback(value);
    }
  );
  const target = document.createElement('div');
  document.body.append(target);
  let draft: ProductEnquiry | undefined;
  const mounted = mount(Panel, {
    target,
    props: {
      view,
      observe(callback: (value: ProductViewSnapshot) => void) {
        observers.add(callback);
        callback(productViewSnapshot(view));
        return () => {
          observers.delete(callback);
        };
      },
      capture(value: ProductEnquiry) {
        draft = value;
      }
    }
  });
  function capture(review: unknown = 'reviewed_product_enquiry') {
    const captured = captureProductEnquiry(view, review);
    if (captured) {
      if (draft) closeProductEnquiry(draft);
      draft = captured;
    }
    return !!captured;
  }
  let closed = false;
  return {
    viewSnapshot: () => productViewSnapshot(view),
    capture,
    snapshot: () => draft && productEnquirySnapshot(draft),
    message: () => draft && productEnquiryMessage(draft),
    update: (text: string) =>
      !!draft &&
      updateProductEnquiry(draft, text, 'reviewed_product_enquiry_edit'),
    native: () =>
      thread
        ? thread.native()
        : Promise.resolve({ received: [], pairs: [], sends: [] }),
    delta: () => {
      const actual = thread?.delta() ?? { decrypts: 0, signs: 0, encrypts: 0 };
      return {
        decrypts: actual.decrypts - preparationCounts.decrypts,
        signs: actual.signs - preparationCounts.signs,
        encrypts: actual.encrypts - preparationCounts.encrypts
      };
    },
    knownPath: () => (thread ? '/messages/' + thread.knownId : undefined),
    reuse: () =>
      draft && thread
        ? reuseProductEnquiryConversation(
            draft,
            thread.controller,
            'reviewed_product_enquiry_conversation'
          )
        : Promise.resolve(undefined),
    async copied() {
      const fake = {} as ProductEnquiry;
      return {
        view: !!captureProductEnquiry({ ...view }, 'reviewed_product_enquiry'),
        snapshot: productEnquirySnapshot(fake),
        message: productEnquiryMessage(fake),
        update: updateProductEnquiry(
          fake,
          'forged',
          'reviewed_product_enquiry_edit'
        ),
        reuse:
          draft &&
          (await reuseProductEnquiryConversation(
            draft,
            {} as InboxView,
            'reviewed_product_enquiry_conversation'
          ))
      };
    },
    async unreviewed() {
      return {
        capture: !!captureProductEnquiry(view, undefined),
        update: !!draft && updateProductEnquiry(draft, 'unreviewed', undefined),
        reuse:
          draft &&
          thread &&
          (await reuseProductEnquiryConversation(
            draft,
            thread.controller,
            undefined
          ))
      };
    },
    async persistentText() {
      const native = thread ? await thread.native() : undefined;
      return JSON.stringify({
        url: location.href,
        local: { ...localStorage },
        session: { ...sessionStorage },
        native
      });
    },
    advance() {
      offset += 15001;
    },
    async closeView() {
      closeProductView(view);
      await tick();
    },
    async close() {
      if (closed) return;
      closed = true;
      if (draft) closeProductEnquiry(draft);
      closeProductView(view);
      closePublicRuntime(context);
      await unmount(mounted);
      target.remove();
      await thread?.close();
    }
  };
}
