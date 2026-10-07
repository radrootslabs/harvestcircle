<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { IdentityViewActionResult } from '../runtime/view-context.ts';
  import type { IdentitySnapshot } from '../runtime/identity-session.ts';
  import Button from './primitives/Button.svelte';
  import { internalHref } from '../navigation-url.ts';
  let {
    identity,
    required = 'connection',
    compact = false,
    focusScope = '',
    focusTarget = 'inline',
    ondisconnect,
    onconnect,
    onprobe
  }: {
    identity: IdentitySnapshot;
    required?: 'connection' | 'messaging';
    compact?: boolean;
    focusScope?: string;
    focusTarget?: 'navbar' | 'protected' | 'inline';
    ondisconnect?: () => void;
    onconnect?: () => Promise<IdentityViewActionResult> | void;
    onprobe?: () => Promise<IdentityViewActionResult> | void;
  } = $props();
  let alive = false;
  onMount(() => {
    alive = true;
    return () => {
      alive = false;
    };
  });
  const connected = $derived(
    identity.state !== 'guest' && identity.state !== 'pending'
  );
  const text = $derived(
    identity.admission === 'busy'
      ? 'An extension request is still pending. Wait for it to finish before trying again.'
      : identity.state === 'pending'
        ? 'Waiting for extension approval.'
        : identity.state === 'guest'
          ? identity.reason === 'refused'
            ? 'Connection was not approved.'
            : identity.reason === 'missing' || identity.reason === 'unavailable'
              ? 'A compatible Nostr extension is not available.'
              : identity.reason === 'changed_key'
                ? 'The extension identity changed. Connect again to continue.'
                : identity.reason === 'invalid_key'
                  ? 'The extension did not return a supported identity.'
                  : 'Use your Nostr extension to choose your identity.'
          : required === 'connection'
            ? 'Connected.'
            : identity.messaging === 'capable'
              ? 'Messaging support checked.'
              : identity.messaging === 'unsupported'
                ? 'This extension does not support the required message encryption.'
                : identity.messaging === 'refused'
                  ? 'The messaging support check was not approved.'
                  : 'Check support for message encryption before continuing.'
  );
  async function run(
    command: (() => Promise<IdentityViewActionResult> | void) | undefined,
    messaging: boolean
  ) {
    const originalScope = focusScope;
    let result: IdentityViewActionResult | void;
    try {
      result = await command?.();
    } catch {
      return;
    }
    if (
      !result ||
      !result.current() ||
      result.identity.admission === 'busy' ||
      result.identity.state === 'guest' ||
      result.identity.state === 'pending' ||
      (messaging && result.identity.messaging !== 'capable')
    )
      return;
    await tick();
    if (!alive || focusScope !== originalScope || !result.current()) return;
    if (focusTarget === 'navbar')
      document.getElementById('identity-navbar-status')?.focus();
    else if (focusTarget === 'protected')
      document.getElementById('identity-protected-status')?.focus();
    else document.getElementById('identity-inline-status')?.focus();
  }
  function connect() {
    void run(onconnect, false);
  }
  function probe() {
    void run(onprobe, true);
  }
</script>

{#if !compact}<h2>
    {connected ? 'Connection capabilities' : 'Connect to continue'}
  </h2>{/if}
<p
  id={focusTarget === 'navbar'
    ? 'identity-navbar-status'
    : focusTarget === 'protected'
      ? 'identity-protected-status'
      : 'identity-inline-status'}
  role="status"
  tabindex="-1"
>
  {text}
</p>
{#if !connected}
  {#if !compact}<p>Your private key stays in the extension.</p>{/if}
  <Button
    label="Connect extension"
    disabled={identity.state === 'pending' || !onconnect}
    onclick={connect}
  />
  {#if identity.state === 'pending' && ondisconnect}<Button
      label="Disconnect"
      onclick={ondisconnect}
    />{/if}
  {#if !compact}<div class="cluster">
      <a href={internalHref('/search')}>Keep browsing</a><a
        href={internalHref('/about#help')}>Connection help</a
      >
    </div>{/if}
{:else if identity.state !== 'guest' && identity.state !== 'pending' && required === 'messaging' && identity.messaging !== 'capable'}
  <p>
    This check asks your extension to encrypt and decrypt a disposable
    self-copy. It does not unlock messages or change public inbox preferences.
  </p>
  <Button label="Check messaging support" disabled={!onprobe} onclick={probe} />
{/if}
