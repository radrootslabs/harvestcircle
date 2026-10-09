<script lang="ts">
  import { getContext, onMount, tick } from 'svelte';
  import InboxStatus from './InboxStatus.svelte';
  import Button from './primitives/Button.svelte';
  import MessageItem from './MessageItem.svelte';
  import ConversationOptions from './ConversationOptions.svelte';
  import MessageComposer from './MessageComposer.svelte';
  import {
    createReplyController,
    createRuntimeReplyController,
    selectReplyParent,
    replySnapshot,
    closeReplyController,
    type ReplyController
  } from '../messaging/reply-controller.ts';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSession,
    subscribeIdentityView,
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
    inboxViewSetup,
    subscribeInboxView,
    closeInboxView,
    inboxThreadSnapshot,
    inboxThreadRevision,
    markInboxConversationDisplayed,
    type InboxView
  } from '../messaging/inbox-view.ts';
  let {
    controller,
    conversationId
  }: { controller?: InboxView; conversationId: string } = $props();
  const identity = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT),
    publicContext = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);
  let owned = $state.raw<InboxView | undefined>(),
    reply = $state.raw<ReplyController | undefined>(),
    model = $state(inboxThreadSnapshot(undefined, '')),
    announcement = $state(''),
    readProblem = $state(false),
    retryRevision = $state(0),
    displayedPending = $state(false);
  let runtime = $state.raw<PublicRuntime | undefined>();
  let off = () => {},
    initialized = false,
    previous = Array.from<string>([]),
    attempted = Array.from<string>([]),
    disposed = false;
  function captureReply(parent: string) {
    if (!owned) return;
    return runtime
      ? createRuntimeReplyController(owned, conversationId, parent, runtime)
      : createReplyController(owned, conversationId, parent);
  }
  function replyTo(parent: string) {
    if (reply && replySnapshot(reply)?.dirty) {
      selectReplyParent(reply, parent, 'reviewed_reply_parent');
      return;
    }
    if (reply) closeReplyController(reply);
    reply = captureReply(parent);
  }
  onMount(() => {
    let observedRevision = -2,
      observedMode = '';
    function acquire() {
      if (disposed) return;
      if (owned && inboxViewSnapshot(owned).owner) return;
      off();
      if (owned && !controller) closeInboxView(owned);
      owned = controller;
      if (!owned && runtime) {
        const session = identityViewSession(identity),
          setup = createRuntimeInboxSetupView(identity, runtime);
        if (session && setup) {
          owned = createInboxView({ identity: session, setup, conversationId });
          if (!owned) closeInboxSetupView(setup);
        }
      }
      observedRevision = -2;
      off = subscribeInboxView(owned, (panel) => {
        const revision = inboxThreadRevision(owned),
          mode = panel.state + ':' + panel.count + ':' + (panel.reason ?? '');
        if (revision === observedRevision && mode === observedMode) return;
        observedRevision = revision;
        observedMode = mode;
        const next = inboxThreadSnapshot(owned, conversationId);
        if (next.status !== 'ready') {
          if (reply) closeReplyController(reply);
          reply = undefined;
          model = next;
          announcement = '';
          readProblem = false;
          initialized = false;
          previous = [];
          attempted = [];
          return;
        }
        const ids = next.messages.map((row) => row.data.rumorId),
          added = ids.filter((id) => !previous.includes(id)).length;
        if (ids.length > 0 && initialized && added)
          announcement =
            added + (added === 1 ? ' message added.' : ' messages added.');
        if (ids.length > 0) initialized = true;
        previous = ids;
        model = next;
        if (!reply && ids.length) reply = captureReply(ids[ids.length - 1]);
      });
    }
    const identityOff = identity
      ? subscribeIdentityView(identity, acquire)
      : () => {};
    acquire();
    if (!controller && publicContext)
      void publicRuntimeReady(publicContext)
        .then((value) => {
          if (!disposed) {
            runtime = value;
            acquire();
          }
        })
        .catch(() => {});
    return () => {
      disposed = true;
      identityOff();
      off();
      if (reply) closeReplyController(reply);
      if (owned && !controller) {
        const setup = inboxViewSetup(owned);
        closeInboxView(owned);
        if (setup) closeInboxSetupView(setup);
      }
    };
  });
  $effect(() => {
    const original = owned,
      selected = conversationId,
      rows = model.messages;
    void retryRevision;
    if (!original || model.status !== 'ready' || displayedPending) return;
    const ids = rows
      .map((row) => row.data.rumorId)
      .filter((id) => !attempted.includes(id));
    if (!ids.length) return;
    attempted = attempted.concat(ids);
    displayedPending = true;
    void tick()
      .then(async () => {
        if (disposed || owned !== original || conversationId !== selected)
          return;
        const actual = inboxThreadSnapshot(original, selected);
        if (
          actual.status !== 'ready' ||
          ids.some(
            (id) => !actual.messages.some((row) => row.data.rumorId === id)
          )
        )
          return;
        const result = await markInboxConversationDisplayed(
          original,
          selected,
          ids,
          'reviewed_displayed_conversation'
        );
        if (!disposed && owned === original && conversationId === selected)
          readProblem = !result;
      })
      .finally(() => {
        displayedPending = false;
      });
  });
</script>

{#if owned}<InboxStatus
    controller={owned}
    showLists={false}
    manageController={false}
  />{:else}<InboxStatus showLists={false} manageController={false} />{/if}
<div class="page page--reading stack">
  <h2>Conversation</h2>
  {#if owned && model.status === 'ready'}
    <ConversationOptions controller={owned} {conversationId} />
    {#each model.messages as message (message.data.rumorId)}<MessageItem
        controller={owned}
        {conversationId}
        {message}
        {runtime}
        onreply={replyTo}
      />{/each}
    {#if reply}{#key reply}<MessageComposer
          controller={reply}
          focusScope="conversation"
        />{/key}{/if}
  {:else}<p>
      This conversation is unavailable until it is unlocked in the matching
      browser and account.
    </p>{/if}
  {#if readProblem}<p class="notice">
      Local read status could not be confirmed. Encrypted copies are preserved.
    </p>
    <Button
      label="Retry local read status"
      disabled={displayedPending}
      onclick={() => {
        attempted = [];
        readProblem = false;
        retryRevision++;
      }}
    />{/if}
  <p role="status" aria-live="polite" data-thread-announcement>
    {announcement}
  </p>
  <a href="/messages">Back to messages</a>
</div>
