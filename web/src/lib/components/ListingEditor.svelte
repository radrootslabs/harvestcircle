<script lang="ts">
  /* global document, setInterval, clearInterval */
  import { getContext, onMount, tick } from 'svelte';
  import ListingReview from './ListingReview.svelte';
  import ConflictNotice from './ConflictNotice.svelte';
  import ConfirmPanel from './ConfirmPanel.svelte';
  import InboxSetup from './InboxSetup.svelte';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    publicRuntimeReady,
    type PublicRuntimeContext,
    type PublicRuntime
  } from '../runtime/public-runtime.ts';
  import {
    createRuntimeInboxSetupView,
    inboxSetupViewSnapshot,
    subscribeInboxSetupView,
    closeInboxSetupView,
    type InboxSetupView
  } from '../messaging/inbox-setup-view.ts';
  import {
    openEditorReview,
    editorReviewSnapshot,
    closeEditorReview,
    publishEditorRevision,
    editorPublicationOperation,
    type EditorReview,
    type EditorFormError
  } from '../publishing/editor-workflow.ts';
  import type { ListingOperationView } from '../publishing/listing-operation-view.ts';
  import CapabilityGate from './CapabilityGate.svelte';
  import Notice from './Notice.svelte';
  import OperationStatus from './OperationStatus.svelte';
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
    saveAndCloseListingEditor,
    closeListingEditor,
    subscribeListingEditor,
    listingEditorEditSource,
    observeListingEditor,
    type ListingEditor as Editor,
    type ListingEditorRoute,
    type ListingEditorSnapshot
  } from '../publishing/listing-editor.ts';
  import { internalHref } from '../navigation-url.ts';
  let {
    route,
    onfirstsave,
    onsavedclose,
    inboxController
  }: {
    route: ListingEditorRoute;
    onfirstsave?: (draftId: string, current: () => boolean) => Promise<void>;
    onsavedclose?: () => Promise<void>;
    inboxController?: InboxSetupView;
  } = $props();
  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);
  const publicContext = getContext<PublicRuntimeContext>(
    PUBLIC_RUNTIME_CONTEXT
  );
  let runtime = $state.raw<PublicRuntime | undefined>(),
    setup = $state.raw<InboxSetupView | undefined>();
  let setupState = $state(inboxSetupViewSnapshot(undefined));
  let review = $state.raw<EditorReview | undefined>(),
    reviewBusy = $state(false);
  let soldConfirm = $state(false),
    publishBusy = $state(false);
  let publicationOperation = $state.raw<ListingOperationView | undefined>();
  let errors = $state<readonly EditorFormError[]>([]),
    reviewMessage = $state('');
  let identity = $state(identityViewSnapshot(context)),
    mounted = $state(false),
    editor = $state.raw<Editor | undefined>(),
    snapshot = $state<ListingEditorSnapshot>(),
    unavailable = $state(false);
  let closeBusy = $state(false),
    navigationFailed = $state(false);
  let firstNavigation: Promise<void> | undefined;
  let firstOwner: Editor | undefined;
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
  const editable = $derived(
    !!(visible && visible.editable && !publicationOperation && !publishBusy)
  );
  onMount(() => {
    mounted = true;
    const off = subscribeIdentityView(context, (next) => {
      identity = next;
    });
    if (publicContext)
      void publicRuntimeReady(publicContext)
        .then((value) => {
          if (mounted) runtime = value;
        })
        .catch(() => {
          /* Draft saving does not depend on inbox setup. */
        });
    return () => {
      mounted = false;
      off();
    };
  });
  $effect(() => {
    const author = admittedAuthor,
      available = active,
      source = runtime,
      supplied = inboxController;
    if (!available || !author) {
      setup = undefined;
      setupState = inboxSetupViewSnapshot(undefined);
      return;
    }
    const owned =
      supplied ?? (source && createRuntimeInboxSetupView(context, source));
    setup = owned;
    const off = owned
      ? subscribeInboxSetupView(owned, (next) => {
          setupState = next;
        })
      : () => {};
    return () => {
      off();
      if (owned) closeInboxSetupView(owned);
    };
  });
  const visibleReview = $derived.by(() => {
    if (
      (!setupState.owner && route.mode !== 'edit') ||
      !snapshot ||
      !visible ||
      !review
    )
      return;
    return editorReviewSnapshot(review) ? review : undefined;
  });
  $effect(() => {
    const resource = route,
      author = admittedAuthor,
      editRuntime = route.mode === 'edit' ? runtime : undefined;
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
    const observer =
      resource.mode === 'edit'
        ? setInterval(() => {
            if (owned) observeListingEditor(owned);
          }, 250)
        : undefined;
    editor = undefined;
    snapshot = undefined;
    review = undefined;
    publicationOperation = undefined;
    soldConfirm = false;
    errors = [];
    reviewMessage = '';
    form = blankForm();
    unavailable = false;
    void openListingEditor(
      context,
      resource,
      cancellation.signal,
      editRuntime
    ).then((result) => {
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
        if (
          snapshot?.state === 'saved' &&
          snapshot.draftId &&
          resource.mode === 'new' &&
          owned &&
          firstOwner !== owned &&
          !closeBusy &&
          onfirstsave
        ) {
          firstOwner = owned;
          firstNavigation = onfirstsave(
            snapshot.draftId,
            () =>
              alive &&
              mounted &&
              editor === owned &&
              !!owned &&
              !!listingEditorSnapshot(owned)
          ).catch(() => {
            if (alive) navigationFailed = true;
          });
        }
        if (!snapshot) {
          unavailable = true;
          form = blankForm();
        }
      });
    });
    return () => {
      alive = false;
      if (observer !== undefined) clearInterval(observer);
      cancellation.abort();
      off();
      if (owned) closeListingEditor(owned);
      if (editor === owned && review) closeEditorReview(review);
      form = blankForm();
    };
  });
  function changed() {
    const original = editor;
    if (!original || !listingEditorSnapshot(original)?.editable) return;
    if (review) closeEditorReview(review);
    review = undefined;
    errors = [];
    soldConfirm = false;
    void editListingEditor(original, { ...form });
  }
  async function showReview(status: 'active' | 'sold' = 'active') {
    const original = editor,
      resource = route,
      author = admittedAuthor;
    if (
      reviewBusy ||
      closeBusy ||
      publishBusy ||
      publicationOperation ||
      !original ||
      !listingEditorSnapshot(original)?.editable
    )
      return;
    reviewBusy = true;
    try {
      const result = await openEditorReview(
        original,
        setup,
        { ...form },
        status
      );
      if (
        !mounted ||
        editor !== original ||
        route !== resource ||
        admittedAuthor !== author ||
        !listingEditorSnapshot(original)
      ) {
        if (result.status === 'review') closeEditorReview(result.review);
        return;
      }
      if (result.status === 'blocked') {
        errors = result.errors;
        reviewMessage = result.message;
        await tick();
        document.querySelector<HTMLElement>('#listing-review-errors')?.focus();
        return;
      }
      if (review) closeEditorReview(review);
      review = result.review;
      errors = [];
      reviewMessage = '';
      await tick();
      if (mounted && editor === original && editorReviewSnapshot(result.review))
        document.querySelector<HTMLElement>('#listing-review-heading')?.focus();
    } finally {
      reviewBusy = false;
    }
  }
  async function editDetails() {
    if (review) closeEditorReview(review);
    review = undefined;
    errors = [];
    reviewMessage = '';
    await tick();
    if (mounted && editor && listingEditorSnapshot(editor)?.editable)
      document.querySelector<HTMLInputElement>('#listing-title')?.focus();
  }
  async function publishChanges() {
    const original = review,
      resource = route,
      author = admittedAuthor;
    if (
      !original ||
      publishBusy ||
      publicationOperation ||
      route.mode !== 'edit'
    )
      return;
    publishBusy = true;
    try {
      const result = await publishEditorRevision(
        original,
        'publish_reviewed_revision'
      );
      if (
        !mounted ||
        review !== original ||
        route !== resource ||
        admittedAuthor !== author
      )
        return;
      publicationOperation = editorPublicationOperation(original);
      if (result.status !== 'completed')
        reviewMessage =
          'The update did not complete. Your proposed draft remains here. Check any retained operation before another attempt.';
    } finally {
      publishBusy = false;
    }
  }
  function fieldError(field: keyof PublicDraftForm) {
    return errors.find((value) => value.field === field);
  }
  function fieldId(field: keyof PublicDraftForm) {
    return (
      'listing-' +
      (field === 'contactType'
        ? 'contact-type'
        : field === 'contactValue'
          ? 'contact-value'
          : field)
    );
  }
  function connect() {
    return connectIdentityView(context);
  }
  async function saveClose() {
    const original = editor,
      resource = route,
      author = admittedAuthor;
    if (
      closeBusy ||
      publishBusy ||
      publicationOperation ||
      !original ||
      !listingEditorSnapshot(original)?.editable
    )
      return;
    closeBusy = true;
    try {
      const saved = await saveAndCloseListingEditor(original);
      await firstNavigation;
      if (
        !saved ||
        !mounted ||
        editor !== original ||
        route !== resource ||
        !author ||
        admittedAuthor !== author
      )
        return;
      await onsavedclose?.();
    } catch {
      if (mounted) navigationFailed = true;
    } finally {
      closeBusy = false;
    }
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
    {#if route.mode === 'edit' && editor && listingEditorEditSource(editor)}
      <ConflictNotice
        source={listingEditorEditSource(editor)!}
        proposed={form}
      />
      {#if editable}<Notice
          tone="info"
          text="Changes stay local until you review and publish them."
        />{/if}
    {/if}
    {#if !editable}<Notice
        tone="info"
        text={route.mode === 'draft'
          ? 'This draft has a retained listing operation. Keep its original outcome for reconciliation.'
          : 'These retained local values are read-only until the current listing is checked.'}
      />{:else}<p>
        Changes are saved in this browser. Browser storage is not a backup.
      </p>{/if}
    {#if reviewMessage}<Notice tone="warning" text={reviewMessage} />{/if}
    {#if soldConfirm && editable}<ConfirmPanel
        title="Confirm sold update"
        text="Marking this listing as sold is a public availability update, not a payment or stock reservation receipt. Review the exact update before publishing."
        confirmLabel="Review sold update"
        onconfirm={() => {
          soldConfirm = false;
          void showReview('sold');
        }}
        oncancel={() => {
          soldConfirm = false;
        }}
      />{/if}
    {#if visibleReview}<ListingReview
        review={visibleReview}
        onedit={editDetails}
        onpublish={route.mode === 'edit'
          ? () => {
              void publishChanges();
            }
          : undefined}
        publishing={publishBusy}
        captured={!!publicationOperation}
      />{:else}
      {#if errors.length}<div
          id="listing-review-errors"
          tabindex="-1"
          role="alert"
        >
          <p>Check these listing fields. Your input remains here.</p>
          <ul>
            {#each errors as error (error.field)}<li>
                <a href={internalHref('#' + fieldId(error.field))}
                  >{error.message}</a
                >
              </li>{/each}
          </ul>
        </div>{/if}
      <form
        class="stack"
        onsubmit={(event) => event.preventDefault()}
        novalidate
      >
        <div class="field">
          <label class="label" for="listing-title">Product name</label><input
            id="listing-title"
            aria-invalid={fieldError('title') ? 'true' : undefined}
            aria-errormessage={fieldError('title')
              ? 'listing-title-error'
              : undefined}
            type="text"
            value={form.title}
            oninput={(event) => {
              form = { ...form, title: event.currentTarget.value };
              changed();
            }}
            readonly={!editable}
          />
          {#if fieldError('title')}<p id="listing-title-error">
              {fieldError('title')?.message}
            </p>{/if}
        </div>
        <div class="field">
          <label class="label" for="listing-description">Description</label
          ><textarea
            id="listing-description"
            aria-invalid={fieldError('description') ? 'true' : undefined}
            aria-errormessage={fieldError('description')
              ? 'listing-description-error'
              : undefined}
            rows="6"
            value={form.description}
            oninput={(event) => {
              form = { ...form, description: event.currentTarget.value };
              changed();
            }}
            readonly={!editable}></textarea>
          {#if fieldError('description')}<p id="listing-description-error">
              {fieldError('description')?.message}
            </p>{/if}
        </div>
        <div class="field">
          <label class="label" for="listing-location">Public area</label>
          <p id="listing-location-hint" class="text-small">
            A town or collection area is enough.
          </p>
          <input
            id="listing-location"
            aria-invalid={fieldError('location') ? 'true' : undefined}
            aria-errormessage={fieldError('location')
              ? 'listing-location-error'
              : undefined}
            type="text"
            aria-describedby="listing-location-hint"
            value={form.location}
            oninput={(event) => {
              form = { ...form, location: event.currentTarget.value };
              changed();
            }}
            readonly={!editable}
          />
          {#if fieldError('location')}<p id="listing-location-error">
              {fieldError('location')?.message}
            </p>{/if}
        </div>
        <div class="cluster">
          <div class="field">
            <label class="label" for="listing-amount">Price</label><input
              id="listing-amount"
              aria-invalid={fieldError('amount') ? 'true' : undefined}
              aria-errormessage={fieldError('amount')
                ? 'listing-amount-error'
                : undefined}
              type="text"
              inputmode="decimal"
              value={form.amount}
              oninput={(event) => {
                form = { ...form, amount: event.currentTarget.value };
                changed();
              }}
              readonly={!editable}
            />
            {#if fieldError('amount')}<p id="listing-amount-error">
                {fieldError('amount')?.message}
              </p>{/if}
          </div>
          <div class="field">
            <label class="label" for="listing-currency">Currency</label><input
              id="listing-currency"
              aria-invalid={fieldError('currency') ? 'true' : undefined}
              aria-errormessage={fieldError('currency')
                ? 'listing-currency-error'
                : undefined}
              type="text"
              maxlength="3"
              value={form.currency}
              oninput={(event) => {
                form = { ...form, currency: event.currentTarget.value };
                changed();
              }}
              readonly={!editable}
            />
            {#if fieldError('currency')}<p id="listing-currency-error">
                {fieldError('currency')?.message}
              </p>{/if}
          </div>
          <div class="field">
            <label class="label" for="listing-unit">Per</label><select
              id="listing-unit"
              aria-invalid={fieldError('unit') ? 'true' : undefined}
              aria-errormessage={fieldError('unit')
                ? 'listing-unit-error'
                : undefined}
              value={form.unit}
              onchange={(event) => {
                form = { ...form, unit: event.currentTarget.value };
                changed();
              }}
              disabled={!editable}
              ><option value="">Choose a unit</option
              >{#each foodUnits as unit (unit)}<option value={unit}
                  >{unit}</option
                >{/each}</select
            >
            {#if fieldError('unit')}<p id="listing-unit-error">
                {fieldError('unit')?.message}
              </p>{/if}
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
            aria-invalid={fieldError('quantity') ? 'true' : undefined}
            aria-errormessage={fieldError('quantity')
              ? 'listing-quantity-error'
              : undefined}
            type="text"
            inputmode="decimal"
            aria-describedby="listing-quantity-unit"
            value={form.quantity}
            oninput={(event) => {
              form = { ...form, quantity: event.currentTarget.value };
              changed();
            }}
            readonly={!editable}
          />
          {#if fieldError('quantity')}<p id="listing-quantity-error">
              {fieldError('quantity')?.message}
            </p>{/if}
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
                aria-invalid={fieldError('contactType') ? 'true' : undefined}
                aria-errormessage={fieldError('contactType')
                  ? 'listing-contact-type-error'
                  : undefined}
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
                disabled={!editable}
                ><option value="">No public contact</option><option
                  value="email">Email</option
                ><option value="phone">Phone</option><option value="https"
                  >Website</option
                ></select
              >
              {#if fieldError('contactType')}<p id="listing-contact-type-error">
                  {fieldError('contactType')?.message}
                </p>{/if}
            </div>
            <div class="field">
              <label class="label" for="listing-contact-value"
                >Public contact details</label
              ><input
                id="listing-contact-value"
                aria-invalid={fieldError('contactValue') ? 'true' : undefined}
                aria-errormessage={fieldError('contactValue')
                  ? 'listing-contact-value-error'
                  : undefined}
                type="text"
                value={form.contactValue}
                oninput={(event) => {
                  form = { ...form, contactValue: event.currentTarget.value };
                  changed();
                }}
                readonly={!editable}
              />
              {#if fieldError('contactValue')}<p
                  id="listing-contact-value-error"
                >
                  {fieldError('contactValue')?.message}
                </p>{/if}
            </div>
          </div>
        </details>
      </form>
      {#if editable}<button
          type="button"
          onclick={() => {
            void showReview();
          }}
          disabled={reviewBusy || closeBusy}
          >{reviewBusy
            ? 'Preparing review'
            : route.mode === 'edit'
              ? 'Review changes'
              : 'Review listing'}</button
        >{/if}
    {/if}
    {#if editable && route.mode === 'edit' && !visibleReview}<details>
        <summary>Other listing actions</summary>
        <button
          type="button"
          onclick={() => {
            soldConfirm = true;
          }}
          disabled={reviewBusy || closeBusy}>Mark as sold</button
        >
      </details>{/if}
    {#if publicationOperation}<OperationStatus
        publicOperation={publicationOperation}
      />{/if}
    {#if editable && route.mode !== 'edit' && setup}<InboxSetup
        controller={setup}
        inline
      />{/if}
    {#each visible.operations as operation (operation)}<OperationStatus
        publicOperation={operation}
      />{/each}
    {#if editable}<button
        type="button"
        onclick={saveClose}
        disabled={closeBusy || reviewBusy}>Save and close</button
      >{/if}
    {#if navigationFailed}<Notice
        tone="warning"
        text="Navigation did not complete. Your saved local data remains in this browser."
      />{/if}
    {#if editable}<p role="status">
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
