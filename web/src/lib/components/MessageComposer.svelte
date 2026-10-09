<script lang="ts">
  /* global document, setInterval, clearInterval */
  import { onMount, tick } from 'svelte';
  import { beforeNavigate, goto } from '$app/navigation';
  import { internalHref } from '../navigation-url.ts';
  import {
    productRequestSnapshot,
    productRequestText,
    updateProductRequestText,
    sendProductRequest,
    requestProductRequestNavigation,
    keepProductRequestEditing,
    discardProductRequestNavigation,
    productRequestBeforeUnload,
    closeProductRequestController,
    type ProductRequestController
  } from '../messaging/product-request-controller.ts';
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
    request,
    oncancel,
    ondiscard,
    focusScope = 'enquiry'
  }: {
    controller?: ReplyController;
    request?: ProductRequestController;
    oncancel?: () => void;
    ondiscard?: () => void;
    focusScope?: 'conversation' | 'enquiry';
  } = $props();
  let model = $state<
      | ReturnType<typeof replySnapshot>
      | ReturnType<typeof productRequestSnapshot>
    >(),
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
    model = request
      ? productRequestSnapshot(request)
      : controller && replySnapshot(controller);
    text =
      (request
        ? productRequestText(request)
        : controller && replyText(controller)) ?? '';
  }
  function askNavigation() {
    return request
      ? requestProductRequestNavigation(request)
      : controller
        ? requestReplyNavigation(controller)
        : 'invalid';
  }
  beforeNavigate((event) => {
    if (disposed || askNavigation() !== 'confirmation_required') return;
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
      if (request) closeProductRequestController(request);
      else if (controller) closeReplyController(controller);
      text = '';
      model = undefined;
      destination = undefined;
    };
  });
  async function send() {
    const work = request
      ? sendProductRequest(request, 'reviewed_product_request_send')
      : controller && sendReply(controller, 'reviewed_reply_send');
    refresh();
    await work;
    refresh();
  }
  function warnUnload(event: BeforeUnloadEvent) {
    if (request) productRequestBeforeUnload(request, event);
    else if (controller) replyBeforeUnload(controller, event);
  }
</script>

<svelte:window onbeforeunload={warnUnload} />

{#if model}
  <div class="stack">
    <label for={fieldId}>{request ? 'Your message' : 'Reply'}</label>
    <textarea
      id={fieldId}
      value={text}
      disabled={model.busy || model.state === 'completed'}
      aria-describedby={fieldId + '-save'}
      oninput={(event) => {
        if (request)
          updateProductRequestText(
            request,
            event.currentTarget.value,
            'reviewed_private_text'
          );
        else if (controller)
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
    {#if !request}<p>
        Replying moves this conversation to your inbox in this view. It does not
        accept an order.
      </p>{/if}
    <p>Messages do not reserve products or record payment.</p>
    <button
      type="button"
      class="button button--primary"
      disabled={model.busy ||
        model.state === 'completed' ||
        !text.trim() ||
        (!!request && !productRequestSnapshot(request)?.readyToSend)}
      onclick={() => {
        void send();
      }}>{request ? 'Send request' : 'Send reply'}</button
    >
    <p role="status" aria-live="polite">{model.copy}</p>
    {#if request}<button
        type="button"
        class="button button--secondary"
        onclick={() => {
          oncancel?.();
          destination = undefined;
          if (askNavigation() === 'allowed') ondiscard?.();
          refresh();
          void tick().then(focusKeep);
        }}>Cancel</button
      >{/if}
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
              if (request) keepProductRequestEditing(request);
              else if (controller) keepReplyEditing(controller);
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
              const result = request
                ? discardProductRequestNavigation(
                    request,
                    'reviewed_discard_private_text'
                  )
                : controller
                  ? discardReplyNavigation(
                      controller,
                      'reviewed_discard_private_text'
                    )
                  : 'invalid';
              destination = undefined;
              refresh();
              if (result === 'allowed') {
                ondiscard?.();
                if (target) void goto(target);
              } else void tick().then(focusEditor);
            }}>Discard draft</button
          >
        </div>
      </div>
    {/if}
  </div>
{/if}
