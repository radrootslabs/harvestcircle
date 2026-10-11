<script lang="ts">
  import SupportLink from '../../lib/components/SupportLink.svelte';
  import ReportPreview from '../../lib/components/ReportPreview.svelte';
  import {
    createGeneralReport,
    type ReportContext
  } from '../../lib/support/report.ts';
  import { deploymentRelayPolicy } from '../../lib/config/deployment-relays.ts';
  import { readRelayPolicy } from '../../lib/config/relays.ts';
  const sources = readRelayPolicy(deploymentRelayPolicy);
  let report = $state.raw<ReportContext | undefined>();
</script>

<svelte:head><title>About — HarvestCircle</title></svelte:head>
<div class="page page--reading stack">
  <h1>About HarvestCircle</h1>
  <p>Find food listings and speak directly with the people who posted them.</p>
  <section class="stack">
    <h2>How it works</h2>
    <p>
      Search without connecting. Connect a compatible Nostr extension to list
      food or send a private message.
    </p>
    <p>
      Messages do not reserve food or confirm an order. Payments are not handled
      here.
    </p>
  </section>
  <section class="stack">
    <h2>Before you make arrangements</h2>
    <p>
      Confirm the quantity, price and collection details with the seller. A
      profile name is not farm verification.
    </p>
  </section>
  <section id="help" class="stack">
    <h2>Help</h2>
    <details>
      <summary>Connecting an extension</summary>
      <p>
        Use Connect extension, review the identity shown, and approve only the
        action you chose. Connecting does not post a listing or send a message.
      </p>
      <p>
        Listing food requires a reachable reply inbox. Messages need the
        extension's private encryption and decryption capabilities and an
        explicit unlock.
      </p>
      <p>
        Without an available compatible extension, public browsing remains
        available. The website does not generate or import an identity secret.
      </p>
    </details>
    <details>
      <summary>Supported browsers and extensions</summary>
      <p>
        Real browser and extension combinations are still being qualified. No
        supported tuple is claimed for this development build.
      </p>
      <p>
        If the required capabilities are unavailable, posting and messages are
        unavailable. Mobile extension support is not implied.
      </p>
    </details>
    <details>
      <summary>Search sources and inboxes</summary>
      {#if sources.public.length}
        <p>Configured public sources:</p>
        <ul>
          {#each sources.public as source (source.origin)}<li
              class="message-text"
            >
              {source.origin}
            </li>{/each}
        </ul>
      {:else}<p>No production search sources have been approved.</p>{/if}
      {#if sources.inbox.length}
        <p>Configured private inbox sources:</p>
        <ul>
          {#each sources.inbox as source (source.origin)}<li
              class="message-text"
            >
              {source.origin}
            </li>{/each}
        </ul>
      {:else}<p>No production private inbox sources have been approved.</p>{/if}
      <p>
        Checks are bounded and can be incomplete. Selected sources can observe
        query and account interests; inbox retention needs operator
        confirmation.
      </p>
    </details>
  </section>
  <section class="stack">
    <h2>Listing rules</h2>
    <p>
      Use lawful food listings and truthful descriptions. Share public contact
      information only with the contact person's consent. This prototype does
      not certify food, farms or legal compliance.
    </p>
  </section>
  <section id="report" class="stack">
    <h2>Report a problem</h2>
    <p>
      Share only information needed to explain the issue. Listing and
      conversation controls let you review selected context. Private messages
      are never attached automatically, and support cannot automatically decrypt
      them.
    </p>
    <SupportLink />
    {#if report}<ReportPreview context={report} />
    {:else}<button
        type="button"
        onclick={() => {
          report = createGeneralReport();
        }}>Prepare a report description</button
      >{/if}
  </section>
</div>
