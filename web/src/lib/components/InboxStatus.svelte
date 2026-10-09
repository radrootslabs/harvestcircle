<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { internalHref } from '../navigation-url.ts';
  import ConversationRow from './ConversationRow.svelte';
  import CapabilityGate from './CapabilityGate.svelte';
  import InboxSetup from './InboxSetup.svelte';
  import Button from './primitives/Button.svelte';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    identityViewSession,
    subscribeIdentityView,
    connectIdentityView,
    probeIdentityViewMessaging,
    disconnectIdentityView,
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
    closeInboxSetupView
  } from '../messaging/inbox-setup-view.ts';
  import {
    createInboxView,
    inboxViewSnapshot,
    inboxListSnapshot,
    inboxListRevision,
    openInboxConversation,
    loadOlderInboxView,
    inboxViewOlderAvailable,
    inboxViewSetup,
    subscribeInboxView,
    unlockInboxView,
    checkInboxView,
    readNewInboxView,
    stopInboxView,
    closeInboxView,
    type InboxView
  } from '../messaging/inbox-view.ts';
  let { controller }: { controller?: InboxView } = $props();
  const identityContext = getContext<IdentityViewContext>(
      IDENTITY_VIEW_CONTEXT
    ),
    publicContext = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);
  let identity = $state(identityViewSnapshot(identityContext)),
    panel = $state(inboxViewSnapshot(undefined)),
    list = $state(inboxListSnapshot(undefined)),
    olderAvailable = $state(false),
    owned = $state.raw<InboxView | undefined>();
  let runtime: PublicRuntime | undefined,
    off = () => {};
  const setup = $derived(inboxViewSetup(owned));
  onMount(() => {
    let observedRevision = -2,
      observedMode = '';
    function updateList() {
      const revision = inboxListRevision(owned),
        mode = panel.state + ':' + panel.count + ':' + (panel.reason ?? '');
      if (revision !== observedRevision || mode !== observedMode) {
        list = inboxListSnapshot(owned);
        observedRevision = revision;
        observedMode = mode;
      }
    }
    let disposed = false;
    function acquire() {
      if (disposed) return;
      identity = identityViewSnapshot(identityContext);
      if (owned && inboxViewSnapshot(owned).owner) return;
      off();
      if (owned && !controller) {
        const oldSetup = inboxViewSetup(owned);
        closeInboxView(owned);
        if (oldSetup) closeInboxSetupView(oldSetup);
      }
      owned = controller;
      if (
        !owned &&
        runtime &&
        identity.identity.state === 'messaging_capable'
      ) {
        const session = identityViewSession(identityContext),
          originalSetup = createRuntimeInboxSetupView(identityContext, runtime);
        if (session && originalSetup) {
          owned = createInboxView({ identity: session, setup: originalSetup });
          if (!owned) closeInboxSetupView(originalSetup);
        }
      }
      panel = inboxViewSnapshot(owned);
      observedRevision = -2;
      updateList();
      olderAvailable = inboxViewOlderAvailable(owned);
      off = subscribeInboxView(owned, (next) => {
        panel = next;
        updateList();
        olderAvailable = inboxViewOlderAvailable(owned);
      });
    }
    const identityOff = identityContext
      ? subscribeIdentityView(identityContext, acquire)
      : () => {};
    acquire();
    if (!controller && publicContext)
      void publicRuntimeReady(publicContext)
        .then((value) => {
          if (disposed) return;
          runtime = value;
          acquire();
        })
        .catch(() => {
          /* Keep genuine unavailable state. */
        });
    return () => {
      disposed = true;
      identityOff();
      off();
      if (owned && !controller) {
        const originalSetup = inboxViewSetup(owned);
        closeInboxView(owned);
        if (originalSetup) closeInboxSetupView(originalSetup);
      }
    };
  });
  async function openConversation(peer: string) {
    const original = owned;
    if (!original) return;
    const result = await openInboxConversation(
      original,
      peer,
      'reviewed_admitted_conversation_navigation'
    );
    const href = internalHref(result);
    if (owned !== original || inboxListSnapshot(original).status !== 'ready')
      return;
    if (href) void goto(href);
  }
  const effectiveIdentity = $derived(
    panel.owner ? panel.identity : identity.identity
  );
  const reasons: Record<string, string> = {
    access_unavailable:
      'Inbox access has not been qualified and exercised. Unlocked local copies do not prove a successful relay check.',
    storage_unavailable:
      'Browser storage is unavailable. Your inbox has not been checked.',
    unavailable:
      'The message check could not complete. Existing encrypted copies are preserved.',
    elapsed:
      'The bounded inbox check reached its active work limit. Existing encrypted copies are preserved.',
    budget:
      'The bounded inbox check reached its item or size limit. Continue only after reviewing the limit.',
    declined:
      'Your extension did not approve decryption. Choose Read new messages to try another explicit batch.',
    refused:
      'Your extension did not approve decryption. Choose Read new messages to try another explicit batch.',
    stopped: 'Messages are locked. Existing encrypted copies are preserved.',
    wait_expired:
      'Decryption is paused. Wait for the original extension request to settle before another explicit batch.'
  };
  const checked = $derived(
    panel.lastCheckedAt === null
      ? null
      : new Date(panel.lastCheckedAt).toLocaleTimeString()
  );
</script>

<svelte:head
  ><title>HarvestCircle</title><meta
    name="robots"
    content="noindex"
  /></svelte:head
>
<div class="page page--reading stack">
  <h1>
    {effectiveIdentity.state === 'guest' ||
    effectiveIdentity.state === 'pending'
      ? 'Connect or unlock'
      : 'Messages'}
  </h1>
  {#if panel.cleanupRequired}<p class="notice">
      Private resource cleanup is incomplete. Wait before reconnecting or
      starting another message action.
    </p>{/if}
  <CapabilityGate
    identity={effectiveIdentity}
    required="messaging"
    focusTarget="protected"
    onconnect={identity.mounted
      ? () => connectIdentityView(identityContext)
      : undefined}
    onprobe={identity.mounted
      ? () => probeIdentityViewMessaging(identityContext, 'reviewed_self_copy')
      : undefined}
    ondisconnect={() => disconnectIdentityView(identityContext)}
  />
  {#if panel.state === 'locked' || panel.state === 'unlocking'}
    <h2>Unlock your messages</h2>
    <p>
      Your extension will be asked to decrypt your messages. Each explicit batch
      checks up to 20 encrypted envelopes and may require two decryption
      approvals for each envelope. Further batches need another explicit action.
    </p>
    <Button
      label="Unlock messages"
      disabled={!owned || panel.busy}
      onclick={() => {
        if (owned) void unlockInboxView(owned, 'reviewed_messages_unlock');
      }}
    />
  {/if}
  {#if panel.state === 'partial' || panel.state === 'ready' || panel.state === 'empty' || panel.state === 'unlocking'}
    <p role="status" aria-live="polite">
      {panel.state === 'unlocking'
        ? 'Waiting for your explicit extension approvals.'
        : panel.state === 'empty'
          ? 'No messages found in the checked inbox history.'
          : panel.state === 'partial'
            ? 'Your unlocked local messages are available with limitations.'
            : 'Messages are unlocked. Check here for replies.'}
    </p>
    {#if panel.reason}<p class="notice">
        {reasons[panel.reason] ??
          'The check is partial. Existing encrypted copies are preserved; review the limitation before continuing.'}
      </p>{/if}
    <Button
      label="Check for messages"
      disabled={!owned || panel.busy}
      onclick={() => {
        if (owned) void checkInboxView(owned, 'reviewed_foreground_inbox');
      }}
    />
    {#if checked}<p>
        Last checked at {checked}. This was a bounded foreground check, not
        complete inbox history.
      </p>{/if}
    {#if panel.queued > 0}
      <p>
        {panel.queued} encrypted envelopes await an explicit decryption batch. Your
        extension will be asked to decrypt them; up to 20 envelopes are attempted
        per batch.
      </p>
      <Button
        label={'Read new messages (' + panel.queued + ')'}
        disabled={!owned || panel.busy}
        onclick={() => {
          if (owned) void readNewInboxView(owned, 'reviewed_decrypt_batch');
        }}
      />
    {/if}
    {#if owned && list.status === 'ready'}
      {#if list.requests.length > 0}<section
          class="stack"
          aria-label="Message requests"
        >
          <h2>Message requests</h2>
          {#each list.requests as row (row.peer)}<ConversationRow
              controller={owned}
              {row}
              onopen={openConversation}
            />{/each}
        </section>{/if}
      {#if list.conversations.length > 0}<section
          class="stack"
          aria-label="Conversations"
        >
          <h2>Conversations</h2>
          {#each list.conversations as row (row.peer)}<ConversationRow
              controller={owned}
              {row}
              onopen={openConversation}
            />{/each}
        </section>{/if}
      <p class="text-small">
        New means locally unread in this browser. No remote read receipt or
        background message awareness is implied.
      </p>
    {/if}
    <Button
      label="Load older messages"
      disabled={!owned || panel.busy || !olderAvailable}
      onclick={() => {
        if (owned) void loadOlderInboxView(owned, 'reviewed_load_older');
      }}
    />
    <Button
      label="Lock messages"
      disabled={!owned}
      onclick={() => {
        if (owned) stopInboxView(owned);
      }}
    />
  {/if}
  {#key setup}<InboxSetup controller={setup} />{/key}
  <p>
    Check here for replies. This prototype does not send push or email
    notifications.
  </p>
</div>
