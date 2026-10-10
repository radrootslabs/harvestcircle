import { mount, unmount, tick } from 'svelte';
import type { beforeNavigate as KitBeforeNavigate } from '$app/navigation';
import {
  getPublicKey,
  finalizeEvent,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import Panel from './product-request-panel.svelte';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  identityViewSnapshot,
  identityViewSession,
  disconnectIdentityView,
  closeIdentityView
} from '../../../src/lib/runtime/view-context.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createProductView,
  productViewSnapshot,
  closeProductView,
  acknowledgeProductViewUncertainty,
  productViewListingOwnership,
  type ProductView,
  type ProductStatusAcknowledgement,
  type ProductViewSnapshot
} from '../../../src/lib/catalog/product-view.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import {
  productEnquirySnapshot,
  productEnquiryMessage,
  captureProductEnquiry,
  updateProductEnquiry,
  type ProductEnquiry
} from '../../../src/lib/messaging/product-enquiry.ts';
import {
  createProductRequestController,
  productRequestSnapshot,
  productRequestText,
  updateProductRequestText,
  configureProductRequest,
  sendProductRequest,
  requestProductRequestNavigation,
  keepProductRequestEditing,
  discardProductRequestNavigation,
  closeProductRequestController,
  type ProductRequestController
} from '../../../src/lib/messaging/product-request-controller.ts';
import {
  createInboxSetupView,
  inboxSetupViewSnapshot,
  type InboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  resolveInboxPreference,
  settleInboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import { resolveInboxReadback } from '../../../src/lib/nostr/inbox-readback.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  browserExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../../src/lib/nostr/extension-scheduler.ts';

type BeforeNavigate = Parameters<Parameters<typeof KitBeforeNavigate>[0]>[0];
const navigation: ((event: BeforeNavigate) => void)[] = [];
let destinations: string[] = [];
export function beforeNavigate(callback: (event: BeforeNavigate) => void) {
  navigation.push(callback);
}
export function goto(destination: string | URL) {
  destinations = destinations.concat(String(destination));
  return Promise.resolve();
}
type Mode =
  | 'normal'
  | 'refused_key'
  | 'hold_key'
  | 'changed_key'
  | 'hold_encrypt'
  | 'declined_encrypt'
  | 'hold_seal';

// Controlled disposable provider, actual SDK crypto/storage/DOM. No production
// credential, extension, relay access or qualification is supplied by this test.
export async function renderFixture(qualified = true) {
  navigation.length = 0;
  destinations = [];
  const secret = new Uint8Array(32).fill(83),
    peerSecret = new Uint8Array(32).fill(84),
    owner = getPublicKey(secret),
    peer = getPublicKey(peerSecret);
  let mode: Mode = 'normal',
    release: (() => void) | undefined,
    keys = 0,
    encrypts = 0,
    decrypts = 0,
    signs = 0,
    active = true,
    offset = 0,
    pending = false;
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  const held = async <T>(work: () => T): Promise<T> => {
    pending = true;
    return new Promise<T>((resolve) => {
      release = () => {
        pending = false;
        release = undefined;
        resolve(work());
      };
    });
  };
  const provider = {
    getPublicKey: () => {
      keys++;
      if (mode === 'refused_key')
        return Promise.reject(Error('controlled refusal'));
      const value = () => (mode === 'changed_key' ? peer : owner);
      return mode === 'hold_key' ? held(value) : Promise.resolve(value());
    },
    signEvent: (template: UnsignedEvent) => {
      signs++;
      const value = () => finalizeEvent(template, secret);
      return mode === 'hold_seal' && template.kind === 13
        ? held(value)
        : Promise.resolve(value());
    },
    nip44: {
      encrypt: (target: string, text: string) => {
        encrypts++;
        if (mode === 'declined_encrypt')
          return Promise.reject(Error('controlled encryption refusal'));
        const value = () => {
          const key = nip44.v2.utils.getConversationKey(secret, target);
          try {
            return nip44.v2.encrypt(text, key);
          } finally {
            key.fill(0);
          }
        };
        return mode === 'hold_encrypt' ? held(value) : Promise.resolve(value());
      },
      decrypt: (target: string, wire: string) => {
        decrypts++;
        const key = nip44.v2.utils.getConversationKey(secret, target);
        try {
          return Promise.resolve(nip44.v2.decrypt(wire, key));
        } finally {
          key.fill(0);
        }
      }
    }
  };
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: provider
  });
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: true,
          nip50: false
        }
      ],
      inbox: ['wss://archive.example.org', 'wss://peer.example.org'].map(
        (origin) => ({ origin, read: true, write: true })
      ),
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    })
  );
  if (!policy) throw Error('invalid fixed test policy');
  const publicContext = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(publicContext, policy, {
      now: () => offset,
      schedule: () => () => {}
    });
  if (!runtime) throw Error('original runtime unavailable');
  const context = createIdentityViewContext();
  mountIdentityView(context);
  const reference = encodeProductReference({
    kind: 30402,
    pubkey: peer,
    identifier: 'carrots'
  });
  if (!reference) throw Error('invalid coordinate');
  const observers = new Set<(value: ProductViewSnapshot) => void>();
  const view = createProductView(
    createPublicView(runtime),
    reference,
    { nowSeconds: () => 1700000200 },
    (value) => {
      for (const callback of observers) callback(value);
    }
  );
  let otherView: ProductView | undefined;
  let retainedConsent: ProductStatusAcknowledgement | undefined;
  const target = document.createElement('div');
  document.body.append(target);
  let draft: ProductEnquiry | undefined,
    controller: ProductRequestController | undefined,
    headless: ProductRequestController | undefined,
    setup: InboxSetupView | undefined;
  const mounted = mount(Panel, {
    target,
    props: {
      view,
      context,
      publicContext,
      observe(callback: (value: ProductViewSnapshot) => void) {
        observers.add(callback);
        callback(productViewSnapshot(view));
        return () => {
          observers.delete(callback);
        };
      },
      makeSetup(identity: IdentitySession) {
        const source = createPublicView(runtime);
        const made = createInboxSetupView({
          identity,
          policy,
          lookup: (current = () => true) =>
            settleInboxResolver(
              resolveInboxPreference(source, beginPublicViewRun(source), owner),
              () =>
                active &&
                !!identityMessagingOwnership(identity)?.current() &&
                current()
            ),
          readback: (record, author, id) =>
            resolveInboxReadback(
              source,
              beginPublicViewRun(source),
              record,
              author,
              id,
              policy
            ),
          observeAccess: qualified
            ? (data) => ({
                ...data,
                receive: 'qualified_exercised',
                archive: 'qualified_exercised',
                current: () =>
                  active && !!identityMessagingOwnership(identity)?.current()
              })
            : undefined,
          close: () => disposePublicView(source)
        });
        if (!made) throw Error('genuine setup unavailable');
        setup = made;
        return made;
      },
      capture(value: ProductEnquiry) {
        draft = value;
      },
      request(value: ProductRequestController) {
        controller = value;
      }
    }
  });
  async function native() {
    const opened = await openBrowserDatabase();
    if (opened.state !== 'ready')
      throw Error('native browser inspection unavailable');
    try {
      const transaction = browserDatabaseTransaction(
        opened.owner,
        ['private_sends'],
        'readonly'
      );
      const sends = await new Promise<unknown[]>((resolve, reject) => {
        const store = transaction.objectStore('private_sends');
        const request = store.getAll();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () =>
          reject(
            request.error ?? Error('controlled browser inspection failed')
          );
      });
      return { sends };
    } finally {
      closeBrowserDatabase(opened.owner);
    }
  }
  await tick();
  return {
    owner,
    peer,
    viewSnapshot: () => productViewSnapshot(view),
    captureUncertain(review: unknown) {
      const acknowledgement = acknowledgeProductViewUncertainty(view, review);
      return !!captureProductEnquiry(
        view,
        'reviewed_product_enquiry',
        acknowledgement
      );
    },
    createOtherView() {
      otherView = createProductView(
        createPublicView(runtime),
        reference,
        { nowSeconds: () => 1700000200 },
        () => {}
      );
    },
    otherViewSnapshot: () => otherView && productViewSnapshot(otherView),
    consentCases() {
      retainedConsent = acknowledgeProductViewUncertainty(
        view,
        'reviewed_last_known_listing'
      );
      return {
        genuine: !!retainedConsent,
        current: !!productViewListingOwnership(
          view,
          retainedConsent
        )?.current(),
        copied: !!productViewListingOwnership(view, {
          ...retainedConsent
        } as ProductStatusAcknowledgement),
        other:
          !!otherView &&
          !!productViewListingOwnership(otherView, retainedConsent),
        otherControl:
          !!otherView &&
          !!productViewListingOwnership(
            otherView,
            acknowledgeProductViewUncertainty(
              otherView,
              'reviewed_last_known_listing'
            )
          )?.current(),
        unreviewed: !!acknowledgeProductViewUncertainty(view, undefined)
      };
    },
    consentCurrent: () =>
      !!productViewListingOwnership(view, retainedConsent)?.current(),
    identity: () => identityViewSnapshot(context),
    setup: () => inboxSetupViewSnapshot(setup),
    snapshot: () => controller && productRequestSnapshot(controller),
    text: () => controller && productRequestText(controller),
    product: () => draft && productEnquirySnapshot(draft),
    message: () => draft && productEnquiryMessage(draft),
    native,
    counts: () => ({ keys, encrypts, decrypts, signs }),
    pending: () => pending,
    scheduler: () => {
      const scheduler = browserExtensionScheduler();
      return scheduler && extensionSchedulerSnapshot(scheduler);
    },
    mode(value: Mode) {
      mode = value;
    },
    settle() {
      mode = 'normal';
      release?.();
    },
    logout() {
      disconnectIdentityView(context);
    },
    update(value: string) {
      return (
        !!controller &&
        updateProductRequestText(controller, value, 'reviewed_private_text')
      );
    },
    directUpdate(value: string) {
      return (
        !!draft &&
        updateProductEnquiry(draft, value, 'reviewed_product_enquiry_edit')
      );
    },
    beginHeadless() {
      const original = captureProductEnquiry(view, 'reviewed_product_enquiry');
      const actor = identityViewSession(context);
      headless = original && createProductRequestController(original, runtime);
      if (
        !headless ||
        !actor ||
        !setup ||
        !configureProductRequest(
          headless,
          actor,
          setup,
          'reviewed_product_request_setup'
        )
      )
        throw Error('genuine headless test owner unavailable');
      return sendProductRequest(headless, 'reviewed_product_request_send');
    },
    headlessSnapshot: () => headless && productRequestSnapshot(headless),
    headlessText: () => headless && productRequestText(headless),
    revokeView() {
      closeProductView(view);
    },
    unreviewed: () => controller && sendProductRequest(controller, undefined),
    copied: async () => ({
      create: !!createProductRequestController({} as ProductEnquiry),
      snapshot: productRequestSnapshot({} as ProductRequestController),
      configure: configureProductRequest(
        {} as ProductRequestController,
        {} as IdentitySession,
        {} as InboxSetupView,
        'reviewed_product_request_setup'
      ),
      send: await sendProductRequest(
        {} as ProductRequestController,
        'reviewed_product_request_send'
      )
    }),
    request: () => controller && requestProductRequestNavigation(controller),
    keep: () => controller && keepProductRequestEditing(controller),
    discard: () =>
      controller &&
      discardProductRequestNavigation(
        controller,
        'reviewed_discard_private_text'
      ),
    async navigate() {
      let cancelled = false;
      const event = {
        cancel() {
          cancelled = true;
        },
        to: { url: new URL('/about', location.href) },
        willUnload: false
      } as BeforeNavigate;
      for (const callback of navigation) callback(event);
      await tick();
      return { cancelled, destinations: destinations.slice() };
    },
    destinations: () => destinations.slice(),
    persistentText: async () =>
      JSON.stringify({
        url: location.href,
        local: { ...localStorage },
        session: { ...sessionStorage },
        native: await native()
      }),
    advance() {
      offset += 15001;
    },
    async closeView() {
      closeProductView(view);
      if (headless) closeProductRequestController(headless);
      await tick();
    },
    async close() {
      active = false;
      mode = 'normal';
      release?.();
      closeProductView(view);
      if (otherView) closeProductView(otherView);
      await unmount(mounted);
      closeIdentityView(context);
      closePublicRuntime(publicContext);
      target.remove();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
      secret.fill(0);
      peerSecret.fill(0);
    }
  };
}
