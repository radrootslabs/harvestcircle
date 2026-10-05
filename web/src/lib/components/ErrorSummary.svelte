<script lang="ts">
  import { internalHref } from '../navigation-url';
  let {
    errors
  }: { errors: readonly Readonly<{ href: string; text: string }>[] } = $props();
  const id = $props.id();
</script>

{#if errors.length}
  <section class="notice notice--error stack stack--tight" aria-labelledby={id}>
    <h2 {id}>Check the following fields</h2>
    <ul>
      {#each errors as error, index (index)}
        <li>
          {#if error.href.startsWith('#') && internalHref(error.href)}<a
              class="shell-link"
              href={internalHref(error.href)}>{error.text}</a
            >{:else}<span>{error.text}</span>{/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}
