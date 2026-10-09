<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import { page } from '$app/state';
  import AccountGate from '../../../lib/components/AccountGate.svelte';
  import {
    CONVERSATION_DIRECTORY_CONTEXT,
    resolveLocalConversation,
    subscribeConversationDirectory,
    type ConversationDirectory
  } from '../../../lib/messaging/conversation-directory.ts';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewMatchesOwner,
    subscribeIdentityView,
    type IdentityViewContext
  } from '../../../lib/runtime/view-context.ts';
  const directory = getContext<ConversationDirectory | undefined>(
    CONVERSATION_DIRECTORY_CONTEXT
  );
  const identity = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);
  let resolved = $state(false),
    mounted = $state(false);
  let generation = 0;
  async function refresh(conversationId = page.params.conversationId) {
    const original = ++generation;
    resolved = false;
    if (!directory || !mounted) return;
    const result = await resolveLocalConversation(directory, conversationId);
    if (original !== generation || !mounted) return;
    resolved =
      result.status === 'resolved' &&
      identityViewMatchesOwner(identity, result.owner);
  }
  onMount(() => {
    mounted = true;
    const offIdentity = subscribeIdentityView(identity, () => {
      void refresh();
    });
    const offDirectory = directory
      ? subscribeConversationDirectory(directory, () => {
          void refresh();
        })
      : () => {};
    void refresh();
    return () => {
      mounted = false;
      generation++;
      offIdentity();
      offDirectory();
    };
  });
  $effect(() => {
    const conversationId = page.params.conversationId;
    if (mounted) void refresh(conversationId);
  });
</script>

<svelte:head>
  {#if resolved}
    <title>HarvestCircle</title><meta name="robots" content="noindex" />
  {/if}
</svelte:head>
{#if resolved}
  <div class="page page--reading stack">
    <h1>Conversation</h1>
    <p>Unlock message contents to read this conversation.</p>
  </div>
{:else}
  <AccountGate />
  <div class="page page--reading stack">
    <p>
      This conversation is unavailable until it is unlocked in the matching
      browser and account.
    </p>
  </div>
{/if}
<div class="page page--reading"><a href="/messages">Back to messages</a></div>
