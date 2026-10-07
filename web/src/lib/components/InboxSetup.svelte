<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import Button from './primitives/Button.svelte';
  import {
    IDENTITY_VIEW_CONTEXT,
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
    inboxSetupViewSnapshot,
    subscribeInboxSetupView,
    checkInboxSetupView,
    reviewInboxSetupView,
    enableInboxSetupView,
    unlockInboxSetupView,
    readbackInboxSetupView,
    stopInboxSetupView,
    closeInboxSetupView,
    type InboxSetupView
  } from '../messaging/inbox-setup-view.ts';
  let { controller }: { controller?: InboxSetupView } = $props();
  const identityContext = getContext<IdentityViewContext>(
    IDENTITY_VIEW_CONTEXT
  );
  const publicContext = getContext<PublicRuntimeContext>(
    PUBLIC_RUNTIME_CONTEXT
  );
  let owned = $state.raw<InboxSetupView | undefined>(),
    panel = $state(inboxSetupViewSnapshot(undefined)),
    selected = $state<string[]>([]),
    removeTags = $state<number[]>([]),
    removeFields = $state<string[]>([]);
  let runtime: PublicRuntime | undefined;
  let off = () => {};
  onMount(() => {
    let disposed = false;
    function acquire() {
      if (disposed) return;
      if (owned) {
        // An injected genuine owner belongs to its caller. The production
        // context must retire its own old generation before reconnecting.
        if (controller || inboxSetupViewSnapshot(owned).owner) return;
        off();
        closeInboxSetupView(owned);
        owned = undefined;
        panel = inboxSetupViewSnapshot(undefined);
        selected = [];
        removeTags = [];
        removeFields = [];
      }
      owned =
        controller ??
        (runtime && createRuntimeInboxSetupView(identityContext, runtime));
      if (owned)
        off = subscribeInboxSetupView(owned, (next) => {
          panel = next;
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
          /* Keep unavailable panel without acquiring any effects. */
        });
    return () => {
      disposed = true;
      identityOff();
      off();
      closeInboxSetupView(owned);
    };
  });
  function originalFields(wire: string | null): {
    tags: unknown[];
    extras: string[];
  } {
    if (!wire) return { tags: [], extras: [] };
    try {
      const value: unknown = JSON.parse(wire);
      if (!value || typeof value !== 'object' || Array.isArray(value))
        return { tags: [], extras: [] };
      const row = value as Record<string, unknown>;
      const extras: string[] = [];
      function appendExtra(key: string) {
        extras.push(key);
      }
      for (const key in row) {
        if (
          ![
            'id',
            'pubkey',
            'sig',
            'kind',
            'created_at',
            'tags',
            'content'
          ].includes(key)
        )
          appendExtra(key);
      }
      return {
        tags: Array.isArray(row.tags) ? row.tags : [],
        extras
      };
    } catch {
      return { tags: [], extras: [] };
    }
  }
  const original = $derived(originalFields(panel.currentWire));
  function advertisedRelays(wire: string): string[] {
    const relays: string[] = [];
    function append(origin: string) {
      relays.push(origin);
    }
    for (const tag of originalFields(wire).tags) {
      if (
        Array.isArray(tag) &&
        tag.length === 2 &&
        tag[0] === 'relay' &&
        typeof tag[1] === 'string'
      )
        append(tag[1]);
    }
    return relays;
  }
  const proposedRelays = $derived(
    panel.preview ? advertisedRelays(panel.preview.wire) : []
  );
  const messages: Record<string, string> = {
    not_checked:
      'Check your observed public inbox preference before making changes.',
    checking: 'Checking your public inbox preference.',
    lookup_incomplete:
      'The lookup is incomplete. Your current inbox preference is unknown.',
    not_observed:
      'No preference was observed in this completed bounded lookup. This does not prove global absence.',
    needs_review: 'Review your observed configuration before changing it.',
    compatible:
      'Your observed preference is compatible with the configured destinations. Unlock to check inbox access.',
    review: 'Review the exact public replacement before enabling your inbox.',
    approval_wait:
      'Preparing your inbox setup. Extension approvals or named relay acceptance may be pending. Another approval may be needed.',
    awaiting_readback:
      'Relay acceptance and exact readback are separate facts.',
    checking_readback:
      'Checking the exact signed preference on the discovery path.',
    access_unavailable: 'Inbox access has not been qualified and exercised.',
    compatible_existing:
      'Your existing inbox configuration and exercised access are confirmed for this session.',
    verified:
      'Preference acceptance, exact readback and exercised inbox access are confirmed for this session.',
    paused: 'Inbox setup is paused. Your unsent text stays in this page.',
    unavailable:
      'Inbox setup is unavailable. Connect a messaging-capable extension and use qualified inbox sources.',
    storage_unavailable:
      'Browser storage is unavailable. The preference change has not been confirmed.',
    conflict:
      'A conflicting inbox preference was observed. Check the current preference and review again.',
    stopped:
      'Setup stopped. Review the original operation before any further action.',
    needs_action:
      'The preference operation needs attention. Setup is not complete.'
  };
</script>

<section class="stack" aria-label="Inbox setup">
  <h2>Reply inbox</h2>
  <p role="status" aria-live="polite">
    {messages[panel.status] ??
      'Setup could not continue. Check your current preference before trying again.'}
  </p>
  <p>
    A relay stores encrypted messages for retrieval. Enabling an inbox changes
    your public preference; it does not send your enquiry or publish a listing.
  </p>
  {#if panel.owner}<p>Public key: <code>{panel.owner}</code></p>{/if}
  <Button
    label="Check current preference"
    disabled={!owned || panel.busy}
    onclick={() => {
      void checkInboxSetupView(owned);
    }}
  />
  {#if panel.currentWire}
    <h3>Current observed destinations</h3>
    <ul aria-label="Current observed inbox destinations">
      {#each panel.existingInboxes as origin, index (index)}<li>
          {origin}
        </li>{/each}
    </ul>
    <details>
      <summary>Exact current public preference</summary>
      <pre>{panel.currentWire}</pre>
    </details>
  {/if}
  {#if panel.status === 'compatible' || panel.status === 'access_unavailable' || panel.status === 'awaiting_readback'}
    <Button
      label="Unlock inbox"
      disabled={!owned || panel.busy}
      onclick={() => {
        void unlockInboxSetupView(owned);
      }}
    />
  {/if}
  {#if panel.status !== 'compatible' && panel.availableInboxes.length > 0 && panel.currentWire !== undefined && !panel.busy && panel.status !== 'review' && panel.status !== 'awaiting_readback'}
    <fieldset>
      <legend>Select 1–3 configured inboxes</legend>
      {#each panel.availableInboxes as origin (origin)}
        <label
          ><input
            type="checkbox"
            bind:group={selected}
            value={origin}
          />{origin}</label
        >
      {/each}
    </fieldset>
    {#if original.tags.length > 0 || original.extras.length > 0}
      <details>
        <summary>Explicit removals from the replacement</summary>
        <p>
          Existing entries remain unless you select their removal. Unsupported
          extra fields require an explicit decision before replacement.
        </p>
        {#each original.tags as tag, index (index)}<label
            ><input
              type="checkbox"
              bind:group={removeTags}
              value={index}
            />Remove entry {index + 1}:
            <code>{JSON.stringify(tag)}</code></label
          >{/each}
        {#each original.extras as field (field)}<label
            ><input
              type="checkbox"
              bind:group={removeFields}
              value={field}
            />Remove extra field {field}</label
          >{/each}
      </details>
    {/if}
    <Button
      label="Review inbox change"
      disabled={!owned ||
        panel.busy ||
        selected.length < 1 ||
        selected.length > 3}
      onclick={() => {
        void reviewInboxSetupView(owned, selected, removeTags, removeFields);
      }}
    />
  {/if}
  {#if panel.preview}
    <h3>Selected configured reply inboxes</h3>
    <ul>
      {#each panel.preview.selectedInboxes as origin (origin)}<li>
          {origin}
        </li>{/each}
    </ul>
    <h3>Proposed public inbox destinations</h3>
    <ul aria-label="Proposed public inbox destinations">
      {#each proposedRelays as origin, index (index)}<li>{origin}</li>{/each}
    </ul>
    <p>{panel.preview.globalEffect}</p>
    <h3>Public preference publication destinations</h3>
    <ul>
      {#each panel.preview.destinations as origin (origin)}<li>
          {origin}
        </li>{/each}
    </ul>
    <details open>
      <summary>Exact proposed public preference</summary>
      <pre>{panel.preview.wire}</pre>
    </details>
    <Button
      label="Enable inbox"
      variant="primary"
      disabled={panel.busy}
      onclick={() => {
        void enableInboxSetupView(owned);
      }}
    />
  {/if}
  {#each panel.accepted as origin, index (index)}<p>
      Accepted by {origin}
    </p>{/each}
  {#each panel.readback as origin (origin)}<p>
      Exact signed preference read back from {origin}
    </p>{/each}
  {#if panel.accepted.length > 0}<Button
      label="Check acceptance and readback"
      disabled={panel.busy}
      onclick={() => {
        void readbackInboxSetupView(owned);
      }}
    />{/if}
  <Button
    label="Not now"
    onclick={() => {
      stopInboxSetupView(owned);
    }}
  />
</section>
