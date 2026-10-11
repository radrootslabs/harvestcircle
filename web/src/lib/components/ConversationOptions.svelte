<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import {
    subscribeInboxView,
    blockInboxConversation,
    type InboxView
  } from '../messaging/inbox-view.ts';
  import {
    conversationOptionsSnapshot,
    requestConversationOption
  } from '../messaging/cited-product.ts';
  import ReportPreview from './ReportPreview.svelte';
  import {
    createConversationReport,
    closeReportContext,
    type ReportContext
  } from '../support/report.ts';
  let {
    controller,
    conversationId,
    onblock,
    onreport
  }: {
    controller: InboxView;
    conversationId: string;
    onblock?: (peer: string) => void;
    onreport?: (peer: string) => void;
  } = $props();
  let revision = $state(0);
  let report = $state.raw<ReportContext | undefined>();
  let reportOwner = $state.raw<{ view: InboxView; id: string } | undefined>();
  $effect(() => {
    const view = controller,
      id = conversationId;
    untrack(() => {
      if (reportOwner && (reportOwner.view !== view || reportOwner.id !== id)) {
        if (report) closeReportContext(report);
        report = undefined;
        reportOwner = undefined;
      }
    });
  });
  let reviewing = $state(false),
    busy = $state(false),
    notice = $state<string | undefined>();
  async function confirmBlock() {
    if (!reviewing || busy) return;
    busy = true;
    const result = await blockInboxConversation(
      controller,
      conversationId,
      'reviewed_local_private_block'
    );
    busy = false;
    reviewing = false;
    notice =
      result.status === 'saved' || result.status === 'existing'
        ? 'Person blocked in this browser.'
        : 'Block was not confirmed. Existing local data has not been reset.';
  }
  const actual = $derived.by(() => {
    void revision;
    return conversationOptionsSnapshot(controller, conversationId);
  });
  onMount(() => {
    const off = subscribeInboxView(controller, () => {
      revision++;
    });
    return () => {
      off();
      if (report) closeReportContext(report);
    };
  });
</script>

{#if actual}
  <details>
    <summary>Identity and conversation options</summary>
    <p>Authenticated conversation peer: {actual.peer}</p>
    <button
      type="button"
      onclick={() => {
        if (onblock)
          requestConversationOption(
            controller,
            conversationId,
            'block',
            onblock,
            'reviewed_conversation_option'
          );
        else reviewing = true;
      }}>Block person</button
    >
    {#if reviewing}
      <p>
        Blocking hides messages and previews for this account in this browser
        and may discard unsaved private text. Encrypted evidence, delivery
        receipts and remote copies remain. It sends no report or sender
        notification and does not accept or reject an order. Undo is available
        in Privacy.
      </p>
      <button type="button" disabled={busy} onclick={() => void confirmBlock()}
        >Confirm local block</button
      >
      <button
        type="button"
        disabled={busy}
        onclick={() => {
          reviewing = false;
        }}>Cancel</button
      >
    {/if}
    {#if notice}<p role="status">{notice}</p>{/if}
    <button
      type="button"
      onclick={() => {
        if (onreport)
          requestConversationOption(
            controller,
            conversationId,
            'report',
            onreport,
            'reviewed_conversation_option'
          );
        else {
          if (report) closeReportContext(report);
          report = createConversationReport(controller, conversationId);
          reportOwner = report
            ? { view: controller, id: conversationId }
            : undefined;
        }
      }}>Report conversation</button
    >
    {#if report}{#key report}<ReportPreview context={report} />{/key}{/if}
  </details>
{/if}
