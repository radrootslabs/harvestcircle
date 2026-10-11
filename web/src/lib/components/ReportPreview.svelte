<script lang="ts">
  import { onMount } from 'svelte';
  import { deploymentSupportPolicy } from '../config/deployment-support.ts';
  import type { SupportPolicy } from '../config/support.ts';
  import { internalHref } from '../navigation-url.ts';
  import SupportLink from './SupportLink.svelte';
  import {
    reportContextSnapshot,
    subscribeReportContext,
    closeReportContext,
    reviewReportPreview,
    reportPreviewSnapshot,
    closeReportPreview,
    copyReportPreview,
    type ReportContext,
    type ReportPreview
  } from '../support/report.ts';
  let {
    context,
    policy = deploymentSupportPolicy
  }: { context: ReportContext; policy?: SupportPolicy } = $props();
  let revision = $state(0),
    notes = $state(''),
    listingVersion = $state(false),
    coordinate = $state(false),
    peer = $state(false),
    messages = $state<string[]>([]),
    privateDisclosure = $state(false),
    preview = $state.raw<ReportPreview | undefined>(),
    busy = $state(false),
    notice = $state<string | undefined>();
  const actual = $derived.by(() => {
    void revision;
    return reportContextSnapshot(context);
  });
  const text = $derived.by(() => {
    void revision;
    return preview && reportPreviewSnapshot(preview);
  });
  function discard() {
    if (preview) closeReportPreview(preview);
    preview = undefined;
    notice = undefined;
  }
  function choose(id: string, checked: boolean) {
    discard();
    messages = checked
      ? [...messages, id]
      : messages.filter((value) => value !== id);
  }
  function review() {
    discard();
    preview = reviewReportPreview(
      context,
      JSON.stringify({
        listingVersion,
        coordinate,
        peer,
        messages,
        notes,
        privateDisclosure
      }),
      'reviewed_report_preview'
    );
    if (!preview)
      notice =
        actual?.kind === 'conversation' && !privateDisclosure
          ? 'Review the private sharing disclosure before preparing selected private information.'
          : 'A report preview was not prepared. Select only the information you intend to share and keep the description short.';
  }
  async function copy() {
    const selected = preview;
    if (!selected || busy) return;
    busy = true;
    const outcome = await copyReportPreview(selected, 'reviewed_copy_report');
    busy = false;
    if (preview !== selected) return;
    notice =
      outcome === 'copied'
        ? 'Report copied. Nothing was sent to support.'
        : outcome === 'stopped'
          ? 'This report is no longer current. A copy already requested may still remain outside HarvestCircle.'
          : 'Copy was unavailable. Select the reviewed text below if you still want to share it.';
  }
  onMount(() => {
    const original = context;
    const off = subscribeReportContext(original, () => {
      revision++;
      if (!reportContextSnapshot(original)) {
        discard();
        notes = '';
        messages = [];
        peer = false;
        listingVersion = false;
        coordinate = false;
        privateDisclosure = false;
      }
    });
    return () => {
      off();
      discard();
      closeReportContext(original);
    };
  });
</script>

<section class="stack" aria-label="Prepare a selected report">
  <h3>Choose what to share</h3>
  <p>
    Only selected information appears in the preview. Preparing or copying a
    report does not send it.
  </p>
  {#if actual}
    {#if actual.kind === 'listing'}
      <fieldset class="stack">
        <legend>Public listing information</legend>
        <label
          ><input
            type="checkbox"
            checked={listingVersion}
            onchange={(event) => {
              discard();
              listingVersion = event.currentTarget.checked;
            }}
          /> Share listing version</label
        >
        <label
          ><input
            type="checkbox"
            checked={coordinate}
            onchange={(event) => {
              discard();
              coordinate = event.currentTarget.checked;
            }}
          /> Share coordinate</label
        >
      </fieldset>
    {:else if actual.kind === 'conversation'}
      <p class="notice">
        Support cannot automatically read encrypted messages. Selected private
        text and identifiers become readable outside HarvestCircle if you share
        them.
      </p>
      <fieldset class="stack">
        <legend>Private information to include</legend>
        <label
          ><input
            type="checkbox"
            checked={peer}
            onchange={(event) => {
              discard();
              peer = event.currentTarget.checked;
            }}
          /> Share authenticated peer key</label
        >
        {#each actual.messages as message, index (message.id)}
          <div class="stack">
            <label
              ><input
                type="checkbox"
                aria-label={'Share message text and ID ' + (index + 1)}
                checked={messages.includes(message.id)}
                onchange={(event) =>
                  choose(message.id, event.currentTarget.checked)}
              />
              Share message text and ID {index + 1}</label
            >
            <p class="description">{message.content}</p>
          </div>
        {/each}
      </fieldset>
    {/if}
    <label for="report-problem-description">Problem description</label>
    <textarea
      id="report-problem-description"
      rows="4"
      maxlength="4096"
      value={notes}
      oninput={(event) => {
        discard();
        notes = event.currentTarget.value;
      }}></textarea>
    <p>
      Type only information you intend to share. No other messages, drafts,
      receipts or account data are attached.
    </p>
    {#if actual.kind === 'conversation'}
      <label
        ><input
          type="checkbox"
          checked={privateDisclosure}
          onchange={(event) => {
            discard();
            privateDisclosure = event.currentTarget.checked;
          }}
        /> I understand selected private information will leave HarvestCircle if I
        share it.</label
      >
    {/if}
    <button type="button" disabled={busy} onclick={review}>Review report</button
    >
    {#if text !== undefined}
      <pre
        class="description"
        role="region"
        aria-label="Selected report preview">{text}</pre>
      <button type="button" disabled={busy} onclick={() => void copy()}
        >Copy reviewed report</button
      >
    {/if}
  {:else}<p class="notice">
      This report context is no longer available. Reopen it from the current
      listing or conversation if appropriate.
    </p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  <p>
    Copied text can remain in your clipboard or other applications.
    Disconnecting or clearing local data cannot erase those copies. A copy
    already requested may complete after disconnecting.
  </p>
  <SupportLink {policy} />
  <a href={internalHref('/about#report')}>Help and report instructions</a>
</section>
