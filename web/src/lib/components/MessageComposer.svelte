<script lang="ts">
  /* global document, setInterval, clearInterval */
  import { onMount, tick } from 'svelte';
  import { beforeNavigate, goto } from '$app/navigation';
  import { internalHref } from '../navigation-url.ts';
  import {
    replySnapshot,
    replyText,
    updateReplyText,
    sendReply,
    requestReplyNavigation,
    keepReplyEditing,
    discardReplyNavigation,
    replyBeforeUnload,
    closeReplyController,
    type ReplyController
  } from '../messaging/reply-controller.ts';
  let {
    controller,
    focusScope = 'enquiry'
  }: { controller: ReplyController; focusScope?: 'conversation' | 'enquiry' } =
    $props();
  let model = $state<ReturnType<typeof replySnapshot>>(),
    text = $state('');
  let destination: string | undefined,
    disposed = false;
  const fieldId = $derived(
    focusScope === 'conversation' ? 'conversation-reply' : 'enquiry-reply'
  );
  function focusEditor() {
    if (disposed) return;
    if (focusScope === 'conversation')
      document.getElementById('conversation-reply')?.focus();
    else document.getElementById('enquiry-reply')?.focus();
  }
  function focusKeep() {
    if (disposed) return;
    if (focusScope === 'conversation')
      document.getElementById('conversation-keep-reply')?.focus();
    else document.getElementById('enquiry-keep-reply')?.focus();
  }
  function refresh() {
    if (disposed) return;
    model = replySnapshot(controller);
    text = replyText(controller) ?? '';
  }
  beforeNavigate((event) => {
    if (
      disposed ||
      requestReplyNavigation(controller) !== 'confirmation_required'
    )
      return;
    event.cancel();
    const url = event.to?.url;
    destination =
      url && !event.willUnload
        ? internalHref(url.pathname + url.search + url.hash)
        : undefined;
    refresh();
    void tick().then(focusKeep);
  });
  onMount(() => {
    refresh();
    const timer = setInterval(refresh, 200);
    return () => {
      disposed = true;
      clearInterval(timer);
      closeReplyController(controller);
      text = '';
      model = undefined;
      destination = undefined;
    };
  });
  async function send() {
    const work = sendReply(controller, 'reviewed_reply_send');
    refresh();
    await work;
    refresh();
  }
  function warnUnload(event: BeforeUnloadEvent) {
    replyBeforeUnload(controller, event);
  }
</script>

<svelte:window onbeforeunload={warnUnload} />

{#if model}
  <div class="stack">
    <label for={fieldId}>Reply</label>
    <textarea
      id={fieldId}
      value={text}
      disabled={model.busy || model.state === 'completed'}
      aria-describedby={fieldId + '-save'}
      oninput={(event) => {
        updateReplyText(
          controller,
          event.currentTarget.value,
          'reviewed_private_text'
        );
        refresh();
      }}></textarea>
    <p id={fieldId + '-save'}>
      {model.saveCopy} Unsent text is lost when this page closes.
    </p>
    <p>{model.pairCopy}</p>
    <p>
      Replying moves this conversation to your inbox in this view. It does not
      accept an order.
    </p>
    <p>Messages do not reserve products or record payment.</p>
    <button
      type="button"
      class="button button--primary"
      disabled={model.busy || model.state === 'completed' || !text.trim()}
      onclick={() => {
        void send();
      }}>Send reply</button
    >
    <p role="status" aria-live="polite">{model.copy}</p>
    {#if model.navigation === 'confirmation_required'}
      <div class="notice stack" role="group" aria-label="Unsaved private reply">
        <p>
          Discard unsaved private text? Keep editing to preserve it. An
          outstanding extension call remains pending until it settles.
        </p>
        <div class="cluster">
          <button
            type="button"
            class="button button--secondary"
            id={focusScope === 'conversation'
              ? 'conversation-keep-reply'
              : 'enquiry-keep-reply'}
            onclick={() => {
              keepReplyEditing(controller);
              destination = undefined;
              refresh();
              void tick().then(focusEditor);
            }}>Keep editing</button
          >
          <button
            type="button"
            class="button button--secondary"
            onclick={() => {
              const target = internalHref(destination);
              const result = discardReplyNavigation(
                controller,
                'reviewed_discard_private_text'
              );
              destination = undefined;
              refresh();
              if (result === 'allowed') {
                if (target) void goto(target);
              } else void tick().then(focusEditor);
            }}>Discard draft</button
          >
        </div>
      </div>
    {/if}
  </div>
{/if}
