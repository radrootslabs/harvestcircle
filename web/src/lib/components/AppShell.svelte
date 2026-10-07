<script lang="ts">
  import type { Snippet } from 'svelte';
  import { internalHref } from '../navigation-url';
  import Button from './primitives/Button.svelte';
  import CapabilityGate from './CapabilityGate.svelte';
  import type { IdentityViewActionResult } from '../runtime/view-context.ts';
  import type { IdentitySnapshot } from '../runtime/identity-session.ts';
  import Disclosure from './primitives/Disclosure.svelte';

  type ShellRoute =
    '/search' | '/sell' | '/selling' | '/messages' | '/about' | '/privacy';
  type Identity =
    | {
        kind: 'guest';
        onconnect?: () => Promise<IdentityViewActionResult> | void;
      }
    | { kind: 'connected'; publicKey: string; ondisconnect?: () => void };
  let {
    children,
    currentPath = '/',
    availableRoutes = [],
    identity = { kind: 'guest' },
    capability,
    ondisconnect
  }: {
    children: Snippet;
    currentPath?: string;
    availableRoutes?: readonly ShellRoute[];
    identity?: Identity;
    capability?: IdentitySnapshot;
    ondisconnect?: () => void;
  } = $props();

  const observed = $derived(
    capability ??
      (identity.kind === 'guest'
        ? { state: 'guest' as const, reason: 'disconnected' as const }
        : {
            state: 'connected' as const,
            publicKey: identity.publicKey,
            messaging: 'not_probed' as const
          })
  );
  const unavailable = $derived(
    !availableRoutes.includes('/search') ||
      !availableRoutes.includes('/about') ||
      !availableRoutes.includes('/privacy') ||
      (identity.kind === 'guest'
        ? !availableRoutes.includes('/sell') || !identity.onconnect
        : !availableRoutes.includes('/selling') ||
          !availableRoutes.includes('/messages') ||
          !identity.ondisconnect)
  );

  function available(href: string) {
    return (
      href === '/' ||
      availableRoutes.some((route) => route === href.split('#')[0])
    );
  }
</script>

{#snippet navigation(label: string, href: string, brand = false)}
  {#if available(href) && internalHref(href)}
    <a
      href={internalHref(href)}
      class="shell-link"
      class:brand
      aria-current={currentPath === href ? 'page' : undefined}>{label}</a
    >
  {:else}
    <span
      class="shell-link text-muted"
      aria-disabled="true"
      aria-describedby="shell-availability"
      aria-current={currentPath === href ? 'page' : undefined}>{label}</span
    >
  {/if}
{/snippet}

<a href={internalHref('#main-content')} class="skip-link visually-hidden"
  >Skip to main content</a
>
<header class="navbar">
  <nav aria-label="Primary" class="page navbar__inner">
    <div class="cluster">
      {@render navigation('HarvestCircle', '/', true)}
      {@render navigation('Search', '/search')}
      {#if identity.kind === 'guest'}
        {@render navigation('List food', '/sell')}
        {@render navigation('Messages', '/messages')}
      {:else}
        {@render navigation('Messages', '/messages')}
        {@render navigation('Selling', '/selling')}
        <Disclosure summary="Identity">
          <p class="key">{identity.publicKey}</p>
          <Button
            label="Disconnect"
            disabled={!identity.ondisconnect}
            onclick={identity.ondisconnect}
          />
        </Disclosure>
      {/if}
      <CapabilityGate
        focusTarget="navbar"
        {ondisconnect}
        identity={observed}
        compact
        focusScope={currentPath}
        onconnect={identity.kind === 'guest' ? identity.onconnect : undefined}
      />
    </div>
    {#if unavailable}
      <p id="shell-availability" class="text-small text-muted">
        Unavailable during development: disabled navigation and actions.
      </p>
    {/if}
  </nav>
</header>
<main id="main-content" tabindex="-1" class="page">
  {@render children()}
</main>
<footer class="footer">
  <nav aria-label="Footer" class="page cluster">
    {@render navigation('About', '/about')}
    {@render navigation('Privacy', '/privacy')}
    {@render navigation('Help / report', '/about#help')}
  </nav>
</footer>
