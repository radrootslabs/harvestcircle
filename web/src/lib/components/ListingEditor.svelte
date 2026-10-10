<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import CapabilityGate from './CapabilityGate.svelte';
  import Notice from './Notice.svelte';
  import { foodUnits } from '../contracts/food-availability-v1/values.ts';
  import type { PublicDraftForm } from '../contracts/local-records.ts';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    subscribeIdentityView,
    connectIdentityView,
    disconnectIdentityView,
    type IdentityViewContext
  } from '../runtime/view-context.ts';
  import {
    openListingEditor,
    listingEditorSnapshot,
    editListingEditor,
    closeListingEditor,
    subscribeListingEditor,
    type ListingEditor as Editor,
    type ListingEditorRoute,
    type ListingEditorSnapshot
  } from '../publishing/listing-editor.ts';
  import { internalHref } from '../navigation-url.ts';
  let { route }: { route: ListingEditorRoute } = $props();
  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);
  let identity = $state(identityViewSnapshot(context)),
    mounted = $state(false),
    editor = $state.raw<Editor | undefined>(),
    snapshot = $state<ListingEditorSnapshot>(),
    unavailable = $state(false);
  type Form = {
    -readonly [Key in keyof PublicDraftForm]: PublicDraftForm[Key];
  };
  const blankForm = (): Form => ({
    title: '',
    description: '',
    location: '',
    amount: '',
    currency: '',
    unit: '',
    quantity: '',
    contactType: '',
    contactValue: ''
  });
  let form = $state<Form>(blankForm());
  const admittedAuthor = $derived(
    identity.identity.state !== 'guest' && identity.identity.state !== 'pending'
      ? identity.identity.publicKey
      : undefined
  );
  const active = $derived(mounted && identity.mounted);
  const visible = $derived(
    snapshot &&
      editor &&
      identity.mounted &&
      identity.identity.state !== 'guest' &&
      identity.identity.state !== 'pending' &&
      identity.identity.publicKey === snapshot.owner &&
      listingEditorSnapshot(editor)
  );
  onMount(() => {
    mounted = true;
    const off = subscribeIdentityView(context, (next) => {
      identity = next;
    });
    return () => {
      mounted = false;
      off();
    };
  });
  $effect(() => {
    const resource = route,
      author = admittedAuthor;
    if (!active || !author) {
      editor = undefined;
      snapshot = undefined;
      form = blankForm();
      unavailable = false;
      return;
    }
    const cancellation = new AbortController();
    let alive = true,
      owned: Editor | undefined,
      off = () => {};
    editor = undefined;
    snapshot = undefined;
    form = blankForm();
    unavailable = false;
    void openListingEditor(context, resource, cancellation.signal).then(
      (result) => {
        if (!alive) {
          if (result.status === 'ready') closeListingEditor(result.editor);
          return;
        }
        if (result.status !== 'ready') {
          unavailable = true;
          return;
        }
        owned = result.editor;
        editor = owned;
        const next = listingEditorSnapshot(owned);
        if (!next) {
          unavailable = true;
          return;
        }
        snapshot = next;
        form = { ...next.form };
        off = subscribeListingEditor(owned, () => {
          snapshot = owned && listingEditorSnapshot(owned);
          if (!snapshot) {
            unavailable = true;
            form = blankForm();
          }
        });
      }
    );
    return () => {
      alive = false;
      cancellation.abort();
      off();
      if (owned) closeListingEditor(owned);
      form = blankForm();
    };
  });
  function changed() {
    const original = editor;
    if (!original || !listingEditorSnapshot(original)?.editable) return;
    void editListingEditor(original, { ...form });
  }
  function connect() {
    return connectIdentityView(context);
  }
  function disconnect() {
    disconnectIdentityView(context);
  }
  const title = $derived(
    route.mode === 'new'
      ? 'New listing'
      : route.mode === 'draft'
        ? 'Continue draft'
        : 'Edit listing'
  );
</script>

<svelte:head
  ><title>{title} · HarvestCircle</title><meta
    name="robots"
    content="noindex"
  /></svelte:head
>
<div class="page page--reading stack">
  <a
    href={internalHref(
      route.mode === 'edit' ? `/products/${route.naddr}` : '/selling'
    )}>{route.mode === 'edit' ? 'Back to listing' : 'Back to Selling'}</a
  >
  <h1>{title}</h1>
  {#if !visible}
    <CapabilityGate
      identity={identity.identity}
      required="connection"
      focusTarget="protected"
      onconnect={mounted ? connect : undefined}
      ondisconnect={disconnect}
    />
    {#if unavailable}<Notice
        tone="warning"
        text={route.mode === 'draft'
          ? 'This draft is not available here.'
          : route.mode === 'edit'
            ? 'Supported local listing values are not available for editing. Keep this listing read-only.'
            : 'Browser storage is unavailable. Your existing local data has not been reset.'}
      />{:else if identity.identity.state !== 'guest' && identity.identity.state !== 'pending'}<p
        role="status"
      >
        Loading local values.
      </p>{/if}
  {:else}
    {#if !visible.editable}<Notice
        tone="info"
        text="These retained local values are read-only until the current listing is checked."
      />{:else}<p>
        Changes are saved in this browser. Browser storage is not a backup.
      </p>{/if}
    <form class="stack" onsubmit={(event) => event.preventDefault()} novalidate>
      <div class="field">
        <label class="label" for="listing-title">Product name</label><input
          id="listing-title"
          type="text"
          value={form.title}
          oninput={(event) => {
            form = { ...form, title: event.currentTarget.value };
            changed();
          }}
          readonly={!visible.editable}
        />
      </div>
      <div class="field">
        <label class="label" for="listing-description">Description</label
        ><textarea
          id="listing-description"
          rows="6"
          value={form.description}
          oninput={(event) => {
            form = { ...form, description: event.currentTarget.value };
            changed();
          }}
          readonly={!visible.editable}></textarea>
      </div>
      <div class="field">
        <label class="label" for="listing-location">Public area</label>
        <p id="listing-location-hint" class="text-small">
          A town or collection area is enough.
        </p>
        <input
          id="listing-location"
          type="text"
          aria-describedby="listing-location-hint"
          value={form.location}
          oninput={(event) => {
            form = { ...form, location: event.currentTarget.value };
            changed();
          }}
          readonly={!visible.editable}
        />
      </div>
      <div class="cluster">
        <div class="field">
          <label class="label" for="listing-amount">Price</label><input
            id="listing-amount"
            type="text"
            inputmode="decimal"
            value={form.amount}
            oninput={(event) => {
              form = { ...form, amount: event.currentTarget.value };
              changed();
            }}
            readonly={!visible.editable}
          />
        </div>
        <div class="field">
          <label class="label" for="listing-currency">Currency</label><input
            id="listing-currency"
            type="text"
            maxlength="3"
            value={form.currency}
            oninput={(event) => {
              form = { ...form, currency: event.currentTarget.value };
              changed();
            }}
            readonly={!visible.editable}
          />
        </div>
        <div class="field">
          <label class="label" for="listing-unit">Per</label><select
            id="listing-unit"
            value={form.unit}
            onchange={(event) => {
              form = { ...form, unit: event.currentTarget.value };
              changed();
            }}
            disabled={!visible.editable}
            ><option value="">Choose a unit</option
            >{#each foodUnits as unit (unit)}<option value={unit}>{unit}</option
              >{/each}</select
          >
        </div>
      </div>
      <div class="field">
        <label class="label" for="listing-quantity"
          >Advertised quantity (optional)</label
        >
        <p id="listing-quantity-unit">
          {form.unit || 'Uses the selected price unit'}
        </p>
        <input
          id="listing-quantity"
          type="text"
          inputmode="decimal"
          aria-describedby="listing-quantity-unit"
          value={form.quantity}
          oninput={(event) => {
            form = { ...form, quantity: event.currentTarget.value };
            changed();
          }}
          readonly={!visible.editable}
        />
      </div>
      <details>
        <summary>Add public contact details (optional)</summary>
        <div class="stack">
          <p>Only add contact details you want to include publicly.</p>
          <div class="field">
            <label class="label" for="listing-contact-type"
              >Public contact type</label
            ><select
              id="listing-contact-type"
              value={form.contactType}
              onchange={(event) => {
                const value = event.currentTarget.value;
                if (
                  value === '' ||
                  value === 'email' ||
                  value === 'phone' ||
                  value === 'https'
                ) {
                  form = { ...form, contactType: value };
                  changed();
                }
              }}
              disabled={!visible.editable}
              ><option value="">No public contact</option><option value="email"
                >Email</option
              ><option value="phone">Phone</option><option value="https"
                >Website</option
              ></select
            >
          </div>
          <div class="field">
            <label class="label" for="listing-contact-value"
              >Public contact details</label
            ><input
              id="listing-contact-value"
              type="text"
              value={form.contactValue}
              oninput={(event) => {
                form = { ...form, contactValue: event.currentTarget.value };
                changed();
              }}
              readonly={!visible.editable}
            />
          </div>
        </div>
      </details>
    </form>
    {#if visible.editable}<p role="status">
        {visible.state === 'saved'
          ? 'Saved in this browser.'
          : visible.state === 'saving'
            ? 'Saving in this browser.'
            : visible.state === 'failed'
              ? 'Not saved. Keep your current input here.'
              : 'Not yet saved.'}
      </p>
      {#if visible.failure}<Notice
          tone="warning"
          text="Saving did not complete. Keep your current input. Existing local data has not been reset."
        />{/if}{/if}
  {/if}
</div>
