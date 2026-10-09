<script lang="ts">
  import { onMount, setContext, untrack } from 'svelte';
  import ProductEnquiry from '../../../src/lib/components/ProductEnquiry.svelte';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSession,
    identityViewSnapshot,
    subscribeIdentityView,
    type IdentityViewContext
  } from '../../../src/lib/runtime/view-context.ts';
  import { identityMessagingOwnership } from '../../../src/lib/runtime/identity-session.ts';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    type PublicRuntimeContext
  } from '../../../src/lib/runtime/public-runtime.ts';
  import type {
    ProductView,
    ProductViewSnapshot
  } from '../../../src/lib/catalog/product-view.ts';
  import type { ProductEnquiry as Draft } from '../../../src/lib/messaging/product-enquiry.ts';
  import type { ProductRequestController } from '../../../src/lib/messaging/product-request-controller.ts';
  import {
    closeInboxSetupView,
    type InboxSetupView
  } from '../../../src/lib/messaging/inbox-setup-view.ts';
  import type { IdentitySession } from '../../../src/lib/runtime/identity-session.ts';
  let {
    view,
    context,
    publicContext,
    observe,
    makeSetup,
    capture,
    request
  }: {
    view: ProductView;
    context: IdentityViewContext;
    publicContext: PublicRuntimeContext;
    observe: (callback: (value: ProductViewSnapshot) => void) => () => void;
    makeSetup: (identity: IdentitySession) => InboxSetupView;
    capture: (draft: Draft) => void;
    request: (controller: ProductRequestController) => void;
  } = $props();
  setContext(
    IDENTITY_VIEW_CONTEXT,
    untrack(() => context)
  );
  setContext(
    PUBLIC_RUNTIME_CONTEXT,
    untrack(() => publicContext)
  );
  let observation = $state.raw<ProductViewSnapshot | undefined>(),
    setup = $state.raw<InboxSetupView | undefined>();
  onMount(() => {
    let original: IdentitySession | undefined;
    const acquire = () => {
      const identity = identityViewSession(context);
      if (identity === original && setup) return;
      if (
        identityViewSnapshot(context).identity.state !== 'messaging_capable' ||
        !identity ||
        !identityMessagingOwnership(identity)?.current()
      )
        return;
      if (setup) closeInboxSetupView(setup);
      original = identity;
      setup = makeSetup(identity);
    };
    const off = subscribeIdentityView(context, acquire);
    acquire();
    return () => {
      off();
      if (setup) closeInboxSetupView(setup);
    };
  });
  $effect(() =>
    observe((value) => {
      observation = value;
    })
  );
</script>

<section data-request-fixture>
  <ProductEnquiry
    {view}
    {observation}
    {setup}
    oncapture={capture}
    onrequest={request}
  />
</section>
