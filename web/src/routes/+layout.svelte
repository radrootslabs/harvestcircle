<script lang="ts">
  import '../theme.css';
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { page } from '$app/state';
  import AppShell from '../lib/components/AppShell.svelte';

  let { children }: { children: Snippet } = $props();
</script>

<AppShell currentPath={page.url.pathname}>
  {@render children()}
</AppShell>
