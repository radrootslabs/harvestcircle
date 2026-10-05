<script lang="ts">
  import { internalHref } from '../navigation-url';
  import { safeContextBack } from '../routes';
  type Props = { href?: unknown; fallback?: unknown };
  let { href, fallback = '/search' }: Props = $props();
  let target = $derived(
    safeContextBack(href) ?? safeContextBack(fallback) ?? '/search'
  );
  let label = $derived(
    target === '/selling'
      ? 'Back to selling'
      : target === '/messages'
        ? 'Back to messages'
        : target.startsWith('/products/')
          ? 'Back to listing'
          : 'Search food'
  );
</script>

<a href={internalHref(target)} class="button button--secondary">{label}</a>
