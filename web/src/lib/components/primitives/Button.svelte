<script lang="ts">
  import { navigationHref } from '../../navigation-url';
  type Props = {
    label: string;
    variant?: 'primary' | 'secondary';
  } & (
    | {
        kind?: 'button';
        type?: 'button' | 'submit' | 'reset';
        disabled?: boolean;
        onclick?: () => void;
        href?: never;
      }
    | {
        kind: 'link';
        href: string;
        type?: never;
        disabled?: never;
        onclick?: never;
      }
  );
  let {
    label,
    variant = 'secondary',
    kind = 'button',
    type = 'button',
    disabled = false,
    onclick,
    href
  }: Props = $props();
  function invoke() {
    if (!disabled && kind === 'button') onclick?.();
  }
</script>

{#if kind === 'link' && navigationHref(href)}
  <a
    href={navigationHref(href)}
    class="button"
    class:button--primary={variant === 'primary'}
    class:button--secondary={variant === 'secondary'}>{label}</a
  >
{:else if kind === 'link'}
  <span class="button">{label}</span>
{:else}
  <button
    {type}
    {disabled}
    onclick={invoke}
    class="button"
    class:button--primary={variant === 'primary'}
    class:button--secondary={variant === 'secondary'}>{label}</button
  >
{/if}
