<script lang="ts">
  import Notice from './Notice.svelte';
  import {
    editorReviewSnapshot,
    type EditorReview
  } from '../publishing/editor-workflow.ts';
  let { review, onedit }: { review: EditorReview; onedit: () => void } =
    $props();
  const preview = $derived(editorReviewSnapshot(review));
  function tag(name: string) {
    return (
      preview?.tags
        .find((value) => value[0] === name)
        ?.slice(1)
        .join(' ') ?? ''
    );
  }
</script>

{#if preview}
  <section class="stack" aria-label="Public listing review">
    <h2 id="listing-review-heading" tabindex="-1">Public review</h2>
    <p>Check the complete public listing before any signing or publication.</p>
    <h3>{tag('title')}</h3>
    <p class="description" id="listing-review-content">{preview.content}</p>
    <dl>
      <dt>Derived summary</dt>
      <dd>{preview.summary}</dd>
      <dt>Public area</dt>
      <dd>{tag('location')}</dd>
      <dt>Price</dt>
      <dd>{tag('price')} per {tag('radroots:price_unit')}</dd>
      {#if tag('radroots:quantity')}<dt>Advertised quantity</dt>
        <dd>{tag('radroots:quantity')}</dd>{/if}
      <dt>Publisher</dt>
      <dd class="key">{preview.owner}</dd>
      <dt>Public destinations</dt>
      <dd>
        <ul>
          {#each preview.targets as target (target)}<li class="key">
              {target}
            </li>{/each}
        </ul>
      </dd>
    </dl>
    <p>Messages: ready to receive replies</p>
    <p>This review has not requested a signature or published the listing.</p>
    <button type="button" onclick={onedit}>Edit details</button>
  </section>
{:else}
  <Notice
    tone="warning"
    text="This review is no longer current. Return to the current listing values."
  />
  <button type="button" onclick={onedit}>Edit details</button>
{/if}
