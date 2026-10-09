<script module lang="ts">
  declare function setInterval(
    callback: () => void,
    milliseconds: number
  ): number;
  declare function clearInterval(timer: number): void;
</script>

<script lang="ts">
  import { getContext, onMount, untrack } from 'svelte';
  import CapabilityGate from './CapabilityGate.svelte';
  import InboxSetup from './InboxSetup.svelte';
  import MessageComposer from './MessageComposer.svelte';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    identityViewSession,
    subscribeIdentityView,
    connectIdentityView,
    probeIdentityViewMessaging,
    invalidateIdentityView,
    type IdentityViewContext
  } from '../runtime/view-context.ts';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    publicRuntimeReady,
    type PublicRuntimeContext,
    type PublicRuntime
  } from '../runtime/public-runtime.ts';
  import {
    createRuntimeInboxSetupView,
    closeInboxSetupView,
    stopInboxSetupView,
    type InboxSetupView
  } from '../messaging/inbox-setup-view.ts';
  import {
    createProductRequestController,
    configureProductRequest,
    closeProductRequestController,
    stopProductRequestWork,
    productRequestSnapshot,
    type ProductRequestController
  } from '../messaging/product-request-controller.ts';
  import type {
    ProductView,
    ProductViewSnapshot
  } from '../catalog/product-view.ts';
  import {
    captureProductEnquiry,
    productEnquirySnapshot,
    closeProductEnquiry,
    type ProductEnquiry
  } from '../messaging/product-enquiry.ts';
  import Button from './primitives/Button.svelte';
  let {
    view,
    observation,
    oncapture,
    setup,
    onrequest
  }: {
    view: ProductView;
    observation?: ProductViewSnapshot;
    oncapture?: (draft: ProductEnquiry) => void;
    setup?: InboxSetupView;
    onrequest?: (controller: ProductRequestController) => void;
  } = $props();
  let draft = $state.raw<ProductEnquiry | undefined>(),
    captured = $state<ReturnType<typeof productEnquirySnapshot>>(),
    workflow = $state<ReturnType<typeof productRequestSnapshot>>(),
    request = $state.raw<ProductRequestController | undefined>(),
    ownedSetup = $state.raw<InboxSetupView | undefined>();
  const identityContext = getContext<IdentityViewContext>(
    IDENTITY_VIEW_CONTEXT
  );
  const publicContext = getContext<PublicRuntimeContext>(
    PUBLIC_RUNTIME_CONTEXT
  );
  let identity = $state(
    identityContext
      ? identityViewSnapshot(identityContext).identity
      : { state: 'guest' as const, reason: 'unavailable' as const }
  );
  let runtime: PublicRuntime | undefined;
  function acquire() {
    if (!identityContext) return;
    identity = identityViewSnapshot(identityContext).identity;
    if (!setup && runtime && identity.state === 'messaging_capable') {
      if (ownedSetup) closeInboxSetupView(ownedSetup);
      ownedSetup = createRuntimeInboxSetupView(identityContext, runtime);
    }
    configure();
  }
  function configure() {
    const actor = identityContext && identityViewSession(identityContext);
    const selected = setup ?? ownedSetup;
    if (request && actor && selected)
      configureProductRequest(
        request,
        actor,
        selected,
        'reviewed_product_request_setup'
      );
  }
  onMount(() => {
    let alive = true;
    const off = identityContext
      ? subscribeIdentityView(identityContext, acquire)
      : () => {};
    if (publicContext)
      void publicRuntimeReady(publicContext).then((value) => {
        if (alive) {
          runtime = value;
          acquire();
        }
      });
    const timer = setInterval(() => {
      if (draft) captured = productEnquirySnapshot(draft);
      workflow = request && productRequestSnapshot(request);
    }, 250);
    return () => {
      alive = false;
      clearInterval(timer);
      off();
      if (ownedSetup) closeInboxSetupView(ownedSetup);
    };
  });
  $effect(() => {
    void setup;
    configure();
  });
  $effect(() => {
    const original = view;
    return () =>
      untrack(() => {
        void original;
        if (draft) closeProductEnquiry(draft);
        if (request) closeProductRequestController(request);
        request = undefined;
        draft = undefined;
        captured = undefined;
      });
  });
  $effect(() => {
    void observation;
    if (draft) captured = productEnquirySnapshot(draft);
  });
  function open() {
    if (draft) closeProductEnquiry(draft);
    draft = captureProductEnquiry(view, 'reviewed_product_enquiry');
    captured = draft ? productEnquirySnapshot(draft) : undefined;
    if (draft) oncapture?.(draft);
    request = draft && createProductRequestController(draft, runtime);
    configure();
    if (request) onrequest?.(request);
  }
  function cancelSetup() {
    if (request)
      stopProductRequestWork(request, 'reviewed_product_request_cancel');
    if (identityContext) invalidateIdentityView(identityContext);
    stopInboxSetupView(setup ?? ownedSetup);
  }
  function discard() {
    cancelSetup();
    if (request) closeProductRequestController(request);
    if (draft) closeProductEnquiry(draft);
    request = undefined;
    draft = undefined;
    captured = undefined;
  }
</script>

{#if captured}
  <section class="stack" aria-label="Product request">
    <h2>Message about {captured.title}</h2>
    <p>Publisher: <code>{captured.peer}</code></p>
    <p>Advertised price (seller assertion): {captured.advertisedPrice}</p>
    {#if request}<MessageComposer
        {request}
        oncancel={cancelSetup}
        ondiscard={discard}
      />{/if}
    <details>
      <summary>Included product references</summary>
      <p>Product: <code>nostr:{captured.naddr}</code></p>
      <p>Viewed listing: <code>nostr:{captured.nevent}</code></p>
    </details>
    {#if !captured.listingCurrent}<p role="status">
        Listing status needs review. Your unsent text is kept in memory.
      </p>{/if}
    <p>
      This is a message, not a confirmed order. It does not reserve the food.
    </p>
    <CapabilityGate
      {identity}
      required="messaging"
      compact={true}
      primary={true}
      onconnect={identityContext
        ? () => connectIdentityView(identityContext)
        : undefined}
      onprobe={identityContext
        ? () =>
            probeIdentityViewMessaging(identityContext, 'reviewed_self_copy')
        : undefined}
    />
    {#if identity.state === 'messaging_capable'}
      {#key setup ?? ownedSetup}<InboxSetup
          controller={setup ?? ownedSetup}
          inline={true}
        />{/key}
    {/if}
    {#if !workflow || workflow.state === 'idle'}<p>
        Connect and prepare your inbox to send a request. No message has been
        sent.
      </p>{/if}
  </section>
{:else}
  <Button
    label="Message seller"
    onclick={open}
    disabled={observation?.outcome !== 'active' || observation.lastKnown}
  />
{/if}
