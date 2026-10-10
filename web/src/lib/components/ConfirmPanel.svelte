<script lang="ts">
  let {
    title,
    text,
    confirmLabel,
    onconfirm,
    oncancel
  }: {
    title: string;
    text: string;
    confirmLabel: string;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<section class="stack" aria-label={title}>
  <h2>{title}</h2>
  <p>{text}</p>
  <div class="cluster">
    <button type="button" class="button button--primary" onclick={onconfirm}
      >{confirmLabel}</button
    >
    <button type="button" onclick={oncancel}>Keep editing</button>
  </div>
</section>
