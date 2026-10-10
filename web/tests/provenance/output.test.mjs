import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  writeFile
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { generateBuildInfo } from '../../tools/build-info.mjs';
import { auditOutput } from '../../tools/check-output.mjs';

// Slow-host verification allows1800s for each setup/copy/compiler/audit test.
// Vite children and admission checks keep1800s within their1800s group.
// Serial top-level tests keep compatibility work out of the mutation budget.
{
  const source = fileURLToPath(new URL('../../../', import.meta.url));
  // Exact HCP043 product shell bytes, immutable compatibility evidence only.
  const historicalProduct =
    '<svelte:head><title>Food — HarvestCircle</title></svelte:head>\n<div class="page page--reading stack">\n  <h1>Food</h1>\n  <p class="notice">Food details are unavailable during development.</p>\n</div>\n';
  // Frozen public-only HCP055 producers, source6023db01169aa82cb6fe68ca94e1a73750247e72.
  // Compatibility evidence only; runtime tests never require ancestor Git history.
  /** @type {Record<string, {source:string, sha256:string}>} */
  const historicalPublicOnly = {
    'src/routes/+layout.svelte': {
      source:
        "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';\n  import { onMount, setContext, type Snippet } from 'svelte';\n  import {\n    createPublicRuntimeContext,\n    mountPublicRuntime,\n    closePublicRuntime,\n    PUBLIC_RUNTIME_CONTEXT\n  } from '../lib/runtime/public-runtime.ts';\n  import { page } from '$app/state';\n  import AppShell from '../lib/components/AppShell.svelte';\n\n  const publicContext = createPublicRuntimeContext();\n  setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);\n  onMount(() => {\n    mountPublicRuntime(publicContext);\n    return () => closePublicRuntime(publicContext);\n  });\n\n  let { children }: { children: Snippet } = $props();\n</script>\n\n<AppShell\n  currentPath={page.url.pathname}\n  availableRoutes={[\n    '/search',\n    '/sell',\n    '/selling',\n    '/messages',\n    '/about',\n    '/privacy'\n  ]}\n>\n  {@render children()}\n</AppShell>\n",
      sha256: '956da247f23ad9695c3c6f8bff42801aceaa5b84a2606a1a8a5263fd0c637dc7'
    },
    'src/lib/components/AppShell.svelte': {
      source:
        "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  import { internalHref } from '../navigation-url';\n  import Button from './primitives/Button.svelte';\n  import Disclosure from './primitives/Disclosure.svelte';\n\n  type ShellRoute =\n    '/search' | '/sell' | '/selling' | '/messages' | '/about' | '/privacy';\n  type Identity =\n    | { kind: 'guest'; onconnect?: () => void }\n    | { kind: 'connected'; publicKey: string; ondisconnect?: () => void };\n  let {\n    children,\n    currentPath = '/',\n    availableRoutes = [],\n    identity = { kind: 'guest' }\n  }: {\n    children: Snippet;\n    currentPath?: string;\n    availableRoutes?: readonly ShellRoute[];\n    identity?: Identity;\n  } = $props();\n\n  const unavailable = $derived(\n    !availableRoutes.includes('/search') ||\n      !availableRoutes.includes('/about') ||\n      !availableRoutes.includes('/privacy') ||\n      (identity.kind === 'guest'\n        ? !availableRoutes.includes('/sell') || !identity.onconnect\n        : !availableRoutes.includes('/selling') ||\n          !availableRoutes.includes('/messages') ||\n          !identity.ondisconnect)\n  );\n\n  function available(href: string) {\n    return (\n      href === '/' ||\n      availableRoutes.some((route) => route === href.split('#')[0])\n    );\n  }\n</script>\n\n{#snippet navigation(label: string, href: string, brand = false)}\n  {#if available(href) && internalHref(href)}\n    <a\n      href={internalHref(href)}\n      class=\"shell-link\"\n      class:brand\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</a\n    >\n  {:else}\n    <span\n      class=\"shell-link text-muted\"\n      aria-disabled=\"true\"\n      aria-describedby=\"shell-availability\"\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</span\n    >\n  {/if}\n{/snippet}\n\n<a href={internalHref('#main-content')} class=\"skip-link visually-hidden\"\n  >Skip to main content</a\n>\n<header class=\"navbar\">\n  <nav aria-label=\"Primary\" class=\"page navbar__inner\">\n    <div class=\"cluster\">\n      {@render navigation('HarvestCircle', '/', true)}\n      {@render navigation('Search', '/search')}\n      {#if identity.kind === 'guest'}\n        {@render navigation('List food', '/sell')}\n        <Button\n          label=\"Connect extension\"\n          disabled={!identity.onconnect}\n          onclick={identity.onconnect}\n        />\n      {:else}\n        {@render navigation('Messages', '/messages')}\n        {@render navigation('Selling', '/selling')}\n        <Disclosure summary=\"Identity\">\n          <p class=\"key\">{identity.publicKey}</p>\n          <Button\n            label=\"Disconnect\"\n            disabled={!identity.ondisconnect}\n            onclick={identity.ondisconnect}\n          />\n        </Disclosure>\n      {/if}\n    </div>\n    {#if unavailable}\n      <p id=\"shell-availability\" class=\"text-small text-muted\">\n        Unavailable during development: disabled navigation and actions.\n      </p>\n    {/if}\n  </nav>\n</header>\n<main id=\"main-content\" tabindex=\"-1\" class=\"page\">\n  {@render children()}\n</main>\n<footer class=\"footer\">\n  <nav aria-label=\"Footer\" class=\"page cluster\">\n    {@render navigation('About', '/about')}\n    {@render navigation('Privacy', '/privacy')}\n    {@render navigation('Help / report', '/about#help')}\n  </nav>\n</footer>\n",
      sha256: 'c8afbb1ee62a16ad47558829518b347fe3e2a312e490312ded812a8b538d828f'
    },
    'src/lib/components/AccountGate.svelte': {
      source:
        '<script lang="ts">\n  import Button from \'./primitives/Button.svelte\';\n</script>\n\n<svelte:head>\n  <title>HarvestCircle</title>\n  <meta name="robots" content="noindex" />\n</svelte:head>\n<div class="page page--reading stack">\n  <h1>Connect or unlock</h1>\n  <p>Connect an extension and unlock your account to continue.</p>\n  <p class="notice">Account access is unavailable during development.</p>\n  <div class="cluster">\n    <Button label="Connect extension" disabled />\n    <Button label="Unlock" disabled />\n  </div>\n</div>\n',
      sha256: 'd5f90fb4b9af50037ed19dde626552e82df67017a39408bdbdeff1e712c10fbd'
    },
    // Exact predecessor AccountGate route bytes, historical compatibility only.
    'src/routes/sell/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '23547dcfa10e85865e31b66c31ea0d043592e3c316c94da2d74bdf2dd419b38a'
    },
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '465852393264789ceae4133e8d79416ebf37d5485b08fb54c61056fa3714dc5d'
    },
    'src/routes/products/[naddr=naddr]/edit/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '465852393264789ceae4133e8d79416ebf37d5485b08fb54c61056fa3714dc5d'
    },
    // Historical public-only variants must also restore their original private
    // shell. The successor route imports owned identity context, which those
    // historical roots never provided. Current actual-output cases retain it.
    'src/routes/messages/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '23547dcfa10e85865e31b66c31ea0d043592e3c316c94da2d74bdf2dd419b38a'
    },
    'src/routes/messages/[conversationId=local_id]/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '93e21d162901af304f2a142acc7ddce87f7826421735d8a3af4ae281744ab3a0'
    }
  };
  // Exact accepted HCP105 public product producers, frozen before enquiry.
  // Restore only this public-only graph; current output retains HCP106 source.
  const historicalPublicProduct = {
    'src/routes/products/[naddr=naddr]/+page.svelte': {
      source:
        "<script lang=\"ts\">\n  import { getContext, onMount } from 'svelte';\n  import { page } from '$app/state';\n  import {\n    PUBLIC_RUNTIME_CONTEXT,\n    publicRuntimeReady,\n    createPublicView,\n    disposePublicView,\n    type PublicView,\n    type PublicRuntime,\n    type PublicRuntimeContext\n  } from '../../../lib/runtime/public-runtime.ts';\n  import {\n    createProductView,\n    closeProductView,\n    type ProductView,\n    type ProductViewSnapshot\n  } from '../../../lib/catalog/product-view.ts';\n  import { decodeProductReference } from '../../../lib/nostr/references.ts';\n  import { readPublicContact } from '../../../lib/contracts/food-availability-v1/contact-read.ts';\n  import { copyProductReference } from '../../../lib/navigation-copy.ts';\n  import { internalHref, navigationHref } from '../../../lib/navigation-url.ts';\n  import ListingFacts from '../../../lib/components/ListingFacts.svelte';\n  import PublisherIdentity from '../../../lib/components/PublisherIdentity.svelte';\n  import Disclosure from '../../../lib/components/primitives/Disclosure.svelte';\n  import Button from '../../../lib/components/primitives/Button.svelte';\n  const context = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);\n  let runtime = $state.raw<PublicRuntime | undefined>(),\n    result = $state.raw<ProductViewSnapshot | undefined>(),\n    error = $state<string | undefined>(),\n    copyNotice = $state<string | undefined>();\n  const reference = $derived(decodeProductReference(page.params.naddr));\n  const contact = $derived(readPublicContact(result?.food?.content));\n  let disposed = false;\n  onMount(() => {\n    const unavailable = () => {\n      if (!disposed) error = 'Food sources are unavailable.';\n    };\n    try {\n      void publicRuntimeReady(context)\n        .then((value) => {\n          if (!disposed) {\n            runtime = value;\n            if (!value) unavailable();\n          }\n        })\n        .catch(unavailable);\n    } catch {\n      unavailable();\n    }\n    return () => {\n      disposed = true;\n    };\n  });\n  $effect(() => {\n    const selected = reference,\n      shared = runtime;\n    result = undefined;\n    copyNotice = undefined;\n    if (!selected || !shared) return;\n    let active = true,\n      acquired: PublicView | undefined,\n      owner: ProductView | undefined;\n    try {\n      acquired = createPublicView(shared);\n      owner = createProductView(\n        acquired,\n        selected.naddr,\n        { nowSeconds: () => Math.floor(Date.now() / 1000) },\n        (value) => {\n          if (active && !disposed) result = value;\n        }\n      );\n      error = undefined;\n    } catch {\n      error = 'Food sources are unavailable.';\n      if (acquired)\n        try {\n          disposePublicView(acquired);\n        } catch {\n          /* Shared runtime keeps a failed disposer retryable. */\n        }\n    }\n    return () => {\n      active = false;\n      if (owner) closeProductView(owner);\n    };\n  });\n  async function copyLink() {\n    const selected = reference;\n    if (!selected) return;\n    const copied = await copyProductReference(selected.naddr, page.url.origin);\n    if (!disposed && reference?.naddr === selected.naddr)\n      copyNotice = copied\n        ? 'Link copied.'\n        : 'Copy was unavailable. Select the public link below.';\n  }\n</script>\n\n<svelte:head\n  ><title>{result?.food?.title ?? 'Food'} — HarvestCircle</title></svelte:head\n>\n<div class=\"page page--reading stack\">\n  <a href={internalHref('/search')}>Search food</a>\n  <h1>{result?.food?.title ?? 'Food'}</h1>\n  {#if !reference}<p class=\"notice\">This food reference is invalid.</p>\n  {:else if error || result?.outcome === 'unavailable'}<p class=\"notice\">\n      Food sources are unavailable.\n    </p>\n  {:else if !result || result.outcome === 'checking'}<p>\n      Checking food sources…\n    </p>\n  {:else if result.outcome === 'unobserved'}<p class=\"notice\">\n      This listing was not observed in the sources checked. It may be\n      unavailable or missing from these sources.\n    </p>\n  {:else if result.outcome === 'withdrawn'}<p class=\"notice\">\n      This listing was withdrawn by its publisher.\n    </p>\n  {:else if result.outcome === 'unsupported'}<p class=\"notice\">\n      The latest known listing uses an unsupported food format.\n    </p>\n  {:else if result.outcome === 'future_quarantined' || result.outcome === 'clock_unavailable'}<p\n      class=\"notice\"\n    >\n      Listing time could not be confirmed. Food details are unavailable.\n    </p>{/if}\n  {#if result}\n    <PublisherIdentity publisher={result.publisher} />\n    {#if result.lastKnown}<p class=\"notice\">\n        This is last-known information. The listing status check is incomplete.\n      </p>{/if}\n    {#if result.food && result.createdAt !== undefined}<ListingFacts\n        food={result.food}\n        createdAt={result.createdAt}\n      />{/if}\n    {#if result.outcome === 'sold'}<p class=\"notice\">\n        The seller marked this listing sold.\n      </p>{/if}\n    {#if result.outcome === 'active'}<p>\n        Confirm availability with the seller.\n      </p>\n      <Button label=\"Message seller\" disabled={true} />\n      <p class=\"notice\">\n        In-app messaging is unavailable during development.\n      </p>{/if}\n    <Disclosure summary=\"Listing and source details\">\n      {#if result.eventId}<p>\n          Listing version: <code>{result.eventId}</code>\n        </p>{/if}\n      {#each result.sources as source (source.result.context)}<p>\n          {source.kind}: {source.state}. Coverage: {source.result.coverage}.\n        </p>\n        {#each source.result.sources as relay (relay.source)}<p>\n            {relay.source}: {relay.state}.\n          </p>{/each}{/each}\n      <p>\n        These bounded checks do not prove complete coverage or current stock.\n      </p>\n    </Disclosure>\n    {#if contact && result.outcome === 'active'}<Disclosure\n        summary=\"Other contact details\"\n        ><a href={navigationHref(contact.href)}>{contact.href}</a></Disclosure\n      >{/if}\n  {/if}\n  {#if reference}<Button label=\"Copy link\" onclick={copyLink} /><a\n      href={internalHref(`/products/${reference.naddr}`)}>Public food link</a\n    >{/if}\n  {#if copyNotice}<p role=\"status\">{copyNotice}</p>{/if}\n</div>\n",
      sha256: '1869d4dc32ea794d5ee5c1d6cc6b4aac94483482d18cff0562c0cb10087b5278'
    },
    'src/lib/catalog/product-view.ts': {
      source:
        "import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';\nimport {\n  decodeProductReference,\n  type ProductReference\n} from '../nostr/references.ts';\nimport {\n  beginPublicViewRun,\n  disposePublicView,\n  publicViewProjectionAvailable,\n  publicViewRunCurrent,\n  publicViewOwnsRun,\n  subscribePublicView,\n  type PublicView\n} from '../runtime/public-runtime.ts';\nimport {\n  publicRequestScopeSnapshot,\n  publicRunSnapshot,\n  closePublicRequest,\n  disposePublicRun,\n  type PublicRequest\n} from '../nostr/request-scope.ts';\nimport {\n  createPublicHeadCandidate,\n  publicHeadSnapshot,\n  selectPublicHead,\n  type PublicHead\n} from './heads.ts';\nimport {\n  createPublicViewHeadResolver,\n  headResolutionSnapshot,\n  resolveHeads,\n  closeHeadResolver\n} from './resolve-head.ts';\nimport {\n  createPublisherViews,\n  readPublisherPage,\n  publisherSnapshot,\n  type PublisherView\n} from './publishers.ts';\nimport type { FoodProjection } from '../contracts/food-availability-v1/read.ts';\nimport type { WallClock } from './clock-policy.ts';\nimport type { ForegroundSchedule } from './search-view.ts';\nexport type ProductViewSnapshot = Readonly<{\n  reference: ProductReference;\n  outcome:\n    | 'checking'\n    | 'unobserved'\n    | 'unavailable'\n    | 'active'\n    | 'sold'\n    | 'withdrawn'\n    | 'unsupported'\n    | 'future_quarantined'\n    | 'clock_unavailable';\n  food: FoodProjection | undefined;\n  eventId: string | undefined;\n  createdAt: number | undefined;\n  publisher: PublisherView;\n  lastKnown: boolean;\n  sources: readonly ReturnType<typeof publicRequestScopeSnapshot>[];\n}>;\ndeclare const productBrand: unique symbol;\nexport type ProductView = Readonly<{ readonly [productBrand]: true }>;\ninterface Owner {\n  snapshot: () => ProductViewSnapshot;\n  close: () => void;\n}\nconst owners = new WeakMap<ProductView, Owner>();\nfunction ownerOf(token: ProductView): Owner {\n  const owner = owners.get(token);\n  if (!owner) throw Error('product_view_invalid');\n  return owner;\n}\nfunction foreground(callback: () => void, delay: number): () => void {\n  const timer = setTimeout(() => {\n    callback();\n  }, delay);\n  return () => {\n    clearTimeout(timer);\n  };\n}\n// One explicit route read, one coordinate, shared run/request budgets. No forged\n// seed, independent cache, relay hints, automatic refresh or additional deadline.\nexport function createProductView(\n  view: PublicView,\n  value: unknown,\n  clock: WallClock,\n  onUpdate: (value: ProductViewSnapshot) => void,\n  schedule: ForegroundSchedule = foreground\n): ProductView {\n  const decoded = decodeProductReference(value);\n  if (!decoded) throw Error('product_reference_invalid');\n  const reference = decoded;\n  const run = beginPublicViewRun(view),\n    resolver = createPublicViewHeadResolver(view, run, clock),\n    publishers = createPublisherViews(view, clock);\n  let closed = false,\n    failed = false,\n    head: PublicHead | undefined,\n    cancel: (() => void) | undefined;\n  const requests = new Map<PublicRequest, true>();\n  const current = () => !closed && publicViewRunCurrent(view, run);\n  function snapshot(): ProductViewSnapshot {\n    const available =\n      !closed &&\n      publicViewProjectionAvailable(view) &&\n      publicViewOwnsRun(view, run);\n    const row = available ? headResolutionSnapshot(resolver)[0] : undefined;\n    let scopes = Array.from(requests.keys(), publicRequestScopeSnapshot);\n    for (const scope of [row?.headSources, row?.deletionSources])\n      if (\n        scope &&\n        !scopes.some((s) => s.result.context === scope.result.context)\n      )\n        scopes = scopes.concat(scope);\n    const complete =\n      !failed &&\n      scopes.length > 0 &&\n      scopes.every(\n        (s) => s.state === 'eose' && s.result.coverage === 'bounded-eose'\n      );\n    let outcome: ProductViewSnapshot['outcome'];\n    if (!available) outcome = 'unavailable';\n    else if (row?.deletion.outcome === 'suppressed') outcome = 'withdrawn';\n    else if (\n      row?.state.display === 'future_quarantined' ||\n      row?.state.display === 'clock_unavailable'\n    )\n      outcome = row.state.display;\n    else if (row?.state.display === 'unsupported') outcome = 'unsupported';\n    else if (row?.state.food) outcome = row.state.food.status;\n    else if (\n      !row &&\n      (failed ||\n        (scopes.length > 0 &&\n          scopes.every(\n            (scope) =>\n              scope.result.sources.length > 0 &&\n              scope.result.sources.every(\n                (source) =>\n                  source.state === 'error' || source.state === 'closed'\n              )\n          )))\n    )\n      outcome = 'unavailable';\n    else\n      outcome =\n        current() && scopes.some((s) => s.state === 'active')\n          ? 'checking'\n          : 'unobserved';\n    const publisher = publisherSnapshot(publishers, reference.pubkey);\n    const stillAvailable =\n      !closed &&\n      publicViewProjectionAvailable(view) &&\n      publicViewOwnsRun(view, run);\n    return {\n      reference: { ...reference },\n      outcome: stillAvailable ? outcome : 'unavailable',\n      food: stillAvailable ? row?.state.food : undefined,\n      eventId: stillAvailable ? row?.state.head.id : undefined,\n      createdAt: stillAvailable ? row?.state.head.created_at : undefined,\n      publisher: stillAvailable\n        ? publisher\n        : {\n            pubkey: reference.pubkey,\n            label: reference.pubkey,\n            assertedName: false\n          },\n      lastKnown: !stillAvailable || !complete || (row?.lastKnown ?? true),\n      sources: scopes\n    };\n  }\n  function publish() {\n    const value = snapshot();\n    if (!closed && publicViewOwnsRun(view, run)) onUpdate(value);\n  }\n  function queue() {\n    if (!current() || publicRunSnapshot(run).activeRequests === 0) return;\n    const next = schedule(() => {\n      if (closed || !publicViewOwnsRun(view, run)) return;\n      cancel = undefined;\n      publish();\n      queue();\n    }, 250);\n    if (current()) cancel = next;\n    else next();\n  }\n  const token = Object.freeze({}) as ProductView;\n  owners.set(token, {\n    snapshot,\n    close() {\n      closed = true;\n      const previous = cancel;\n      cancel = undefined;\n      previous?.();\n      let error = false;\n      try {\n        closeHeadResolver(resolver);\n      } catch {\n        error = true;\n      }\n      try {\n        if (publicViewOwnsRun(view, run)) disposePublicView(view);\n        else disposePublicRun(run);\n      } catch {\n        error = true;\n      }\n      if (error) throw Error('product_view_close_failed');\n    }\n  });\n  try {\n    const filter =\n      reference.identifier === ''\n        ? {\n            kinds: [30402],\n            authors: [reference.pubkey],\n            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay\n          }\n        : {\n            kinds: [30402],\n            authors: [reference.pubkey],\n            '#d': [reference.identifier],\n            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay\n          };\n    const request = subscribePublicView(\n      view,\n      run,\n      'head',\n      [filter],\n      (proof) => {\n        if (!current()) return;\n        const candidate = createPublicHeadCandidate(proof);\n        if (!candidate) return;\n        const next = publicHeadSnapshot(candidate);\n        if (\n          next.kind !== 30402 ||\n          next.pubkey !== reference.pubkey ||\n          next.identifier !== reference.identifier\n        )\n          return;\n        head = selectPublicHead(head, candidate).head;\n        try {\n          resolveHeads(resolver, [head]);\n          if (current())\n            for (const request of readPublisherPage(publishers, run, [\n              reference.pubkey\n            ]))\n              requests.set(request, true);\n        } catch {\n          failed = true;\n        }\n      }\n    );\n    requests.set(request, true);\n    if (publicRequestScopeSnapshot(request).result.sources.length === 0) {\n      failed = true;\n      closePublicRequest(request);\n    }\n  } catch {\n    failed = true;\n  }\n  publish();\n  queue();\n  return token;\n}\nexport function productViewSnapshot(token: ProductView): ProductViewSnapshot {\n  return ownerOf(token).snapshot();\n}\nexport function closeProductView(token: ProductView): void {\n  ownerOf(token).close();\n}\n",
      sha256: 'e3739170e3c700246a80c1358c7b96541a5cbf1e4c26905e6ca60537adfce9cf'
    }
  };
  const base = await mkdtemp(path.join(os.tmpdir(), 'hc actual output '));
  let setupReady = false;
  /** @param {import('node:test').TestContext} t */
  function active(t) {
    if (t.signal.aborted) throw new Error('Output fixture test aborted');
  }
  /** @param {import('node:test').TestContext} t */
  function ready(t) {
    active(t);
    assert.ok(setupReady, 'Actual output setup must complete before consumers');
  }
  try {
    /** @param {string} root @param {string[]} args */
    const git = (root, ...args) =>
      execFileSync('git', ['-C', root, ...args], {
        env: Object.fromEntries(
          Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
        ),
        encoding: 'utf8'
      });
    /** @param {import('node:test').TestContext} t */
    async function fixture(t) {
      ready(t);
      const root = await realpath(
        await mkdtemp(path.join(os.tmpdir(), 'hc output mutation '))
      );
      t.after(() => rm(root, { recursive: true, force: true }));
      // Audit consumers use actual client/prerendered/build bytes and all source
      // inputs. Server output and generated development/type caches are not audit
      // inputs; avoid copying them for every mutation under the fixed deadline.
      const excluded = new Set([
        path.join(base, 'web/node_modules'),
        path.join(base, 'web/.svelte-kit/output/server'),
        path.join(base, 'web/.svelte-kit/generated'),
        path.join(base, 'web/.svelte-kit/types')
      ]);
      await cp(base, root, {
        recursive: true,
        filter: (name) => !excluded.has(name)
      });
      active(t);
      const web = path.join(root, 'web');
      return { root, web, audit: () => auditOutput(web) };
    }
    /** @param {{root: string, web: string}} f */
    async function refresh(f) {
      git(f.root, 'add', '--', 'web/src');
      await generateBuildInfo(f.web);
      const bytes = await readFile(path.join(f.web, 'static/build-info.json'));
      for (const name of [
        'build/build-info.json',
        '.svelte-kit/output/client/build-info.json'
      ])
        await writeFile(path.join(f.web, name), bytes);
    }

    const sdkKey =
      'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js';
    /** @param {import('node:test').TestContext} t @param {string} from @param {string} web */
    async function copyDependencies(t, from, web) {
      active(t);
      const start = performance.now();
      await cp(
        path.join(from, 'web/node_modules'),
        path.join(web, 'node_modules'),
        {
          recursive: true,
          verbatimSymlinks: true
        }
      );
      active(t);
      t.diagnostic(
        `actual dependency copy milliseconds: ${performance.now() - start}`
      );
    }
    /** @param {import('node:test').TestContext} t @param {string} web */
    function compile(t, web) {
      active(t);
      const start = performance.now();
      try {
        execFileSync(
          process.execPath,
          [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
          { cwd: web, stdio: 'pipe', timeout: 1800000 }
        );
        active(t);
      } finally {
        t.diagnostic(
          `actual Vite compilation milliseconds: ${performance.now() - start}`
        );
      }
    }
    await test(
      'actual output fixture setup',
      { timeout: 1800000 },
      async (t) => {
        // Compile actual current source once in isolation. No install/native subprocess.
        for (const name of git(
          source,
          'ls-files',
          '--cached',
          '--others',
          '--exclude-standard',
          '-z',
          '--',
          'web',
          '.gitignore',
          'core/Cargo.lock',
          'radroots.lib.source-lock.v1.toml'
        )
          .split('\0')
          .filter(Boolean)) {
          const stat = await lstat(path.join(source, name));
          assert.ok(
            stat.isFile() && stat.nlink === 1 && stat.size <= 8 * 1024 * 1024,
            'Only safe actual source fixture inputs'
          );
          await mkdir(path.dirname(path.join(base, name)), { recursive: true });
          await writeFile(
            path.join(base, name),
            await readFile(path.join(source, name))
          );
        }
        git(base, 'init', '-q');
        git(base, 'add', '.');
        git(
          base,
          '-c',
          'user.name=Output Test',
          '-c',
          'user.email=output@example.invalid',
          'commit',
          '-qm',
          'actual inputs'
        );
        await copyDependencies(t, source, path.join(base, 'web'));
        await generateBuildInfo(path.join(base, 'web'));
        compile(t, path.join(base, 'web'));
        setupReady = true;
      }
    );
    await test(
      'HCP115 exact shared-editor predecessor remains independently compilable',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        const historical = {
          'src/routes/sell/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import ListingEditor from '../../lib/components/ListingEditor.svelte';\n</script>\n\n<ListingEditor route={{ mode: 'new' }} />\n",
            sha256:
              '3e99a6121e9ea317d30e631d9edfb05e6596301678f9ae0538e446f0438ae3f4'
          },
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';\n  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';\n  const route = $derived({\n    mode: 'draft',\n    draftId: page.params.draftId ?? ''\n  } satisfies ListingEditorRoute);\n</script>\n\n<ListingEditor {route} />\n",
            sha256:
              '98935ebb01f950aee5da850758801c28303b042ba219759c4fb04c4974376a76'
          },
          'src/routes/products/[naddr=naddr]/edit/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';\n  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';\n  const route = $derived({\n    mode: 'edit',\n    naddr: page.params.naddr ?? ''\n  } satisfies ListingEditorRoute);\n</script>\n\n<ListingEditor {route} />\n",
            sha256:
              '5c50dc2ba4c5734ffe4bd1c2e406e6699581b374fab3849d5c5a0072d9bad1e6'
          },
          'src/lib/components/ListingEditor.svelte': {
            source:
              '<script lang="ts">\n  import { getContext, onMount } from \'svelte\';\n  import CapabilityGate from \'./CapabilityGate.svelte\';\n  import Notice from \'./Notice.svelte\';\n  import { foodUnits } from \'../contracts/food-availability-v1/values.ts\';\n  import type { PublicDraftForm } from \'../contracts/local-records.ts\';\n  import {\n    IDENTITY_VIEW_CONTEXT,\n    identityViewSnapshot,\n    subscribeIdentityView,\n    connectIdentityView,\n    disconnectIdentityView,\n    type IdentityViewContext\n  } from \'../runtime/view-context.ts\';\n  import {\n    openListingEditor,\n    listingEditorSnapshot,\n    editListingEditor,\n    closeListingEditor,\n    subscribeListingEditor,\n    type ListingEditor as Editor,\n    type ListingEditorRoute,\n    type ListingEditorSnapshot\n  } from \'../publishing/listing-editor.ts\';\n  import { internalHref } from \'../navigation-url.ts\';\n  let { route }: { route: ListingEditorRoute } = $props();\n  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);\n  let identity = $state(identityViewSnapshot(context)),\n    mounted = $state(false),\n    editor = $state.raw<Editor | undefined>(),\n    snapshot = $state<ListingEditorSnapshot>(),\n    unavailable = $state(false);\n  type Form = {\n    -readonly [Key in keyof PublicDraftForm]: PublicDraftForm[Key];\n  };\n  const blankForm = (): Form => ({\n    title: \'\',\n    description: \'\',\n    location: \'\',\n    amount: \'\',\n    currency: \'\',\n    unit: \'\',\n    quantity: \'\',\n    contactType: \'\',\n    contactValue: \'\'\n  });\n  let form = $state<Form>(blankForm());\n  const admittedAuthor = $derived(\n    identity.identity.state !== \'guest\' && identity.identity.state !== \'pending\'\n      ? identity.identity.publicKey\n      : undefined\n  );\n  const active = $derived(mounted && identity.mounted);\n  const visible = $derived(\n    snapshot &&\n      editor &&\n      identity.mounted &&\n      identity.identity.state !== \'guest\' &&\n      identity.identity.state !== \'pending\' &&\n      identity.identity.publicKey === snapshot.owner &&\n      listingEditorSnapshot(editor)\n  );\n  onMount(() => {\n    mounted = true;\n    const off = subscribeIdentityView(context, (next) => {\n      identity = next;\n    });\n    return () => {\n      mounted = false;\n      off();\n    };\n  });\n  $effect(() => {\n    const resource = route,\n      author = admittedAuthor;\n    if (!active || !author) {\n      editor = undefined;\n      snapshot = undefined;\n      form = blankForm();\n      unavailable = false;\n      return;\n    }\n    const cancellation = new AbortController();\n    let alive = true,\n      owned: Editor | undefined,\n      off = () => {};\n    editor = undefined;\n    snapshot = undefined;\n    form = blankForm();\n    unavailable = false;\n    void openListingEditor(context, resource, cancellation.signal).then(\n      (result) => {\n        if (!alive) {\n          if (result.status === \'ready\') closeListingEditor(result.editor);\n          return;\n        }\n        if (result.status !== \'ready\') {\n          unavailable = true;\n          return;\n        }\n        owned = result.editor;\n        editor = owned;\n        const next = listingEditorSnapshot(owned);\n        if (!next) {\n          unavailable = true;\n          return;\n        }\n        snapshot = next;\n        form = { ...next.form };\n        off = subscribeListingEditor(owned, () => {\n          snapshot = owned && listingEditorSnapshot(owned);\n          if (!snapshot) {\n            unavailable = true;\n            form = blankForm();\n          }\n        });\n      }\n    );\n    return () => {\n      alive = false;\n      cancellation.abort();\n      off();\n      if (owned) closeListingEditor(owned);\n      form = blankForm();\n    };\n  });\n  function changed() {\n    const original = editor;\n    if (!original || !listingEditorSnapshot(original)?.editable) return;\n    void editListingEditor(original, { ...form });\n  }\n  function connect() {\n    return connectIdentityView(context);\n  }\n  function disconnect() {\n    disconnectIdentityView(context);\n  }\n  const title = $derived(\n    route.mode === \'new\'\n      ? \'New listing\'\n      : route.mode === \'draft\'\n        ? \'Continue draft\'\n        : \'Edit listing\'\n  );\n</script>\n\n<svelte:head\n  ><title>{title} \u00b7 HarvestCircle</title><meta\n    name="robots"\n    content="noindex"\n  /></svelte:head\n>\n<div class="page page--reading stack">\n  <a\n    href={internalHref(\n      route.mode === \'edit\' ? `/products/${route.naddr}` : \'/selling\'\n    )}>{route.mode === \'edit\' ? \'Back to listing\' : \'Back to Selling\'}</a\n  >\n  <h1>{title}</h1>\n  {#if !visible}\n    <CapabilityGate\n      identity={identity.identity}\n      required="connection"\n      focusTarget="protected"\n      onconnect={mounted ? connect : undefined}\n      ondisconnect={disconnect}\n    />\n    {#if unavailable}<Notice\n        tone="warning"\n        text={route.mode === \'draft\'\n          ? \'This draft is not available here.\'\n          : route.mode === \'edit\'\n            ? \'Supported local listing values are not available for editing. Keep this listing read-only.\'\n            : \'Browser storage is unavailable. Your existing local data has not been reset.\'}\n      />{:else if identity.identity.state !== \'guest\' && identity.identity.state !== \'pending\'}<p\n        role="status"\n      >\n        Loading local values.\n      </p>{/if}\n  {:else}\n    {#if !visible.editable}<Notice\n        tone="info"\n        text="These retained local values are read-only until the current listing is checked."\n      />{:else}<p>\n        Changes are saved in this browser. Browser storage is not a backup.\n      </p>{/if}\n    <form class="stack" onsubmit={(event) => event.preventDefault()} novalidate>\n      <div class="field">\n        <label class="label" for="listing-title">Product name</label><input\n          id="listing-title"\n          type="text"\n          value={form.title}\n          oninput={(event) => {\n            form = { ...form, title: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <div class="field">\n        <label class="label" for="listing-description">Description</label\n        ><textarea\n          id="listing-description"\n          rows="6"\n          value={form.description}\n          oninput={(event) => {\n            form = { ...form, description: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}></textarea>\n      </div>\n      <div class="field">\n        <label class="label" for="listing-location">Public area</label>\n        <p id="listing-location-hint" class="text-small">\n          A town or collection area is enough.\n        </p>\n        <input\n          id="listing-location"\n          type="text"\n          aria-describedby="listing-location-hint"\n          value={form.location}\n          oninput={(event) => {\n            form = { ...form, location: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <div class="cluster">\n        <div class="field">\n          <label class="label" for="listing-amount">Price</label><input\n            id="listing-amount"\n            type="text"\n            inputmode="decimal"\n            value={form.amount}\n            oninput={(event) => {\n              form = { ...form, amount: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n        </div>\n        <div class="field">\n          <label class="label" for="listing-currency">Currency</label><input\n            id="listing-currency"\n            type="text"\n            maxlength="3"\n            value={form.currency}\n            oninput={(event) => {\n              form = { ...form, currency: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n        </div>\n        <div class="field">\n          <label class="label" for="listing-unit">Per</label><select\n            id="listing-unit"\n            value={form.unit}\n            onchange={(event) => {\n              form = { ...form, unit: event.currentTarget.value };\n              changed();\n            }}\n            disabled={!visible.editable}\n            ><option value="">Choose a unit</option\n            >{#each foodUnits as unit (unit)}<option value={unit}>{unit}</option\n              >{/each}</select\n          >\n        </div>\n      </div>\n      <div class="field">\n        <label class="label" for="listing-quantity"\n          >Advertised quantity (optional)</label\n        >\n        <p id="listing-quantity-unit">\n          {form.unit || \'Uses the selected price unit\'}\n        </p>\n        <input\n          id="listing-quantity"\n          type="text"\n          inputmode="decimal"\n          aria-describedby="listing-quantity-unit"\n          value={form.quantity}\n          oninput={(event) => {\n            form = { ...form, quantity: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <details>\n        <summary>Add public contact details (optional)</summary>\n        <div class="stack">\n          <p>Only add contact details you want to include publicly.</p>\n          <div class="field">\n            <label class="label" for="listing-contact-type"\n              >Public contact type</label\n            ><select\n              id="listing-contact-type"\n              value={form.contactType}\n              onchange={(event) => {\n                const value = event.currentTarget.value;\n                if (\n                  value === \'\' ||\n                  value === \'email\' ||\n                  value === \'phone\' ||\n                  value === \'https\'\n                ) {\n                  form = { ...form, contactType: value };\n                  changed();\n                }\n              }}\n              disabled={!visible.editable}\n              ><option value="">No public contact</option><option value="email"\n                >Email</option\n              ><option value="phone">Phone</option><option value="https"\n                >Website</option\n              ></select\n            >\n          </div>\n          <div class="field">\n            <label class="label" for="listing-contact-value"\n              >Public contact details</label\n            ><input\n              id="listing-contact-value"\n              type="text"\n              value={form.contactValue}\n              oninput={(event) => {\n                form = { ...form, contactValue: event.currentTarget.value };\n                changed();\n              }}\n              readonly={!visible.editable}\n            />\n          </div>\n        </div>\n      </details>\n    </form>\n    {#if visible.editable}<p role="status">\n        {visible.state === \'saved\'\n          ? \'Saved in this browser.\'\n          : visible.state === \'saving\'\n            ? \'Saving in this browser.\'\n            : visible.state === \'failed\'\n              ? \'Not saved. Keep your current input here.\'\n              : \'Not yet saved.\'}\n      </p>\n      {#if visible.failure}<Notice\n          tone="warning"\n          text="Saving did not complete. Keep your current input. Existing local data has not been reset."\n        />{/if}{/if}\n  {/if}\n</div>\n',
            sha256:
              'fed47cd7257ab3fe1878ef5779b74a4a3e01404d93956806477943190ffb2b18'
          },
          'src/lib/publishing/listing-editor.ts': {
            source:
              "import {\n  exactLocalFields,\n  type PublicDraftForm\n} from '../contracts/local-records.ts';\nimport { canonicalLocalId } from '../private-handles.ts';\nimport { decodeProductReference } from '../nostr/references.ts';\nimport { inspectLocalTemplate } from '../nostr/local-template.ts';\nimport { projectFoodParts } from '../contracts/food-availability-v1/read.ts';\nimport { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';\nimport {\n  openBrowserDatabase,\n  closeBrowserDatabase,\n  type BrowserDatabase\n} from '../persistence/database.ts';\nimport {\n  createPublicPublicationRepository,\n  publicPublicationDraftRepository,\n  publicPublicationQuotaRepository,\n  readPublicListingIntent\n} from '../persistence/public-publications.ts';\nimport {\n  inspectPublicStorage,\n  publicInventorySnapshot\n} from '../persistence/quota.ts';\nimport { publicRecordSnapshot } from '../persistence/records.ts';\nimport {\n  readPublicDraft,\n  publicDraftRepositoryOwner,\n  type PublicDraftRepository\n} from '../persistence/drafts.ts';\nimport {\n  createPublicDraftController,\n  publicDraftControllerSnapshot,\n  editPublicDraft,\n  flushPublicDraft,\n  closePublicDraftController,\n  type PublicDraftController\n} from './draft-controller.ts';\nimport {\n  identityViewSession,\n  identityViewMatchesOwner,\n  type IdentityViewContext\n} from '../runtime/view-context.ts';\nimport {\n  identitySessionSnapshot,\n  subscribeIdentityInvalidation\n} from '../runtime/identity-session.ts';\n\ndeclare const editorBrand: unique symbol;\nexport type ListingEditor = Readonly<{ [editorBrand]: true }>;\nexport type ListingEditorRoute =\n  | Readonly<{ mode: 'new' }>\n  | Readonly<{ mode: 'draft'; draftId: string }>\n  | Readonly<{ mode: 'edit'; naddr: string }>;\nexport type ListingEditorSnapshot = Readonly<{\n  mode: ListingEditorRoute['mode'];\n  owner: string;\n  form: PublicDraftForm;\n  editable: boolean;\n  draftId: string | null;\n  state: 'editing' | 'saving' | 'saved' | 'failed' | 'read-only';\n  failure: string | null;\n  coordinate: string | null;\n  sourceEventId: string | null;\n}>;\ntype Editor = Readonly<{\n  current(): boolean;\n  retire(): void;\n  snapshot(): ListingEditorSnapshot | undefined;\n  edit(form: unknown): Promise<boolean>;\n  flush(): Promise<boolean>;\n  subscribe(listener: () => void): () => void;\n}>;\nconst editors = new WeakMap<ListingEditor, Editor>();\nconst active = new WeakMap<\n  IdentityViewContext,\n  Readonly<{ stamp: symbol; retire(): void }>\n>();\nconst emptyForm: PublicDraftForm = {\n  title: '',\n  description: '',\n  location: '',\n  amount: '',\n  currency: '',\n  unit: '',\n  quantity: '',\n  contactType: '',\n  contactValue: ''\n};\nfunction routeSnapshot(value: unknown): ListingEditorRoute | undefined {\n  try {\n    if (exactLocalFields(value, ['mode']) && value.mode === 'new')\n      return { mode: 'new' };\n    if (\n      exactLocalFields(value, ['mode', 'draftId']) &&\n      value.mode === 'draft'\n    ) {\n      const draftId = canonicalLocalId(value.draftId);\n      return draftId ? { mode: 'draft', draftId } : undefined;\n    }\n    if (exactLocalFields(value, ['mode', 'naddr']) && value.mode === 'edit') {\n      const reference = decodeProductReference(value.naddr);\n      return reference ? { mode: 'edit', naddr: reference.naddr } : undefined;\n    }\n  } catch {\n    return;\n  }\n}\n// Owner admission precedes database opening and all protected hydration. Edit\n// mode presents validated local values; independent head authorization is later.\nexport async function openListingEditor(\n  context: IdentityViewContext,\n  input: unknown,\n  signal?: AbortSignal\n): Promise<\n  | Readonly<{ status: 'ready'; editor: ListingEditor }>\n  | Readonly<{\n      status: 'unavailable';\n      reason: 'owner' | 'resource' | 'storage' | 'source';\n    }>\n> {\n  const session = identityViewSession(context),\n    route = routeSnapshot(input);\n  if (!session || signal?.aborted)\n    return { status: 'unavailable', reason: 'owner' };\n  const identity = identitySessionSnapshot(session),\n    owner = 'publicKey' in identity ? identity.publicKey : undefined;\n  if (!owner || !identityViewMatchesOwner(context, owner))\n    return { status: 'unavailable', reason: 'owner' };\n  if (\n    !route ||\n    (route.mode === 'edit' &&\n      decodeProductReference(route.naddr)?.pubkey !== owner)\n  )\n    return { status: 'unavailable', reason: 'resource' };\n  if (typeof window === 'undefined' || !navigator.locks?.request)\n    return { status: 'unavailable', reason: 'storage' };\n  active.get(context)?.retire();\n  const stamp = Symbol(),\n    listeners = new Map<symbol, () => void>(),\n    cancellation = new AbortController();\n  let alive = true,\n    unsubscribe = () => {},\n    database: BrowserDatabase | undefined,\n    repository: PublicDraftRepository | undefined,\n    controller: PublicDraftController | undefined;\n  function current() {\n    return (\n      alive &&\n      active.get(context)?.stamp === stamp &&\n      identityViewSession(context) === session &&\n      identityViewMatchesOwner(context, owner) &&\n      (!repository || publicDraftRepositoryOwner(repository) === owner)\n    );\n  }\n  let notifying = false;\n  function notify() {\n    if (notifying) return;\n    notifying = true;\n    try {\n      for (const [key, listener] of Array.from(listeners.entries()))\n        if (listeners.get(key) === listener) {\n          try {\n            listener();\n          } catch {\n            /*Presentation observers cannot authorize or retain ownership.*/\n          }\n        }\n    } finally {\n      notifying = false;\n    }\n  }\n  function retire() {\n    if (!alive) return;\n    alive = false;\n    unsubscribe();\n    signal?.removeEventListener('abort', retire);\n    cancellation.abort();\n    if (controller) closePublicDraftController(controller);\n    if (database) closeBrowserDatabase(database);\n    if (active.get(context)?.stamp === stamp) active.delete(context);\n    notify();\n    listeners.clear();\n  }\n  active.set(context, { stamp, retire });\n  unsubscribe = subscribeIdentityInvalidation(session, retire);\n  signal?.addEventListener('abort', retire, { once: true });\n  if (signal?.aborted) retire();\n  function unavailable(reason: 'resource' | 'storage' | 'source') {\n    retire();\n    return { status: 'unavailable' as const, reason };\n  }\n  try {\n    const opened = await openBrowserDatabase(cancellation.signal);\n    if (opened.state !== 'ready') return unavailable('storage');\n    if (!current()) {\n      closeBrowserDatabase(opened.owner);\n      return unavailable('storage');\n    }\n    database = opened.owner;\n    const publications = createPublicPublicationRepository(database, owner);\n    repository = publications && publicPublicationDraftRepository(publications);\n    if (!publications || !repository || !current())\n      return unavailable('storage');\n    const drafts = repository;\n    let sourceForm: PublicDraftForm | undefined,\n      coordinate: string | null = null,\n      sourceEventId: string | null = null;\n    if (route.mode === 'edit') {\n      const reference = decodeProductReference(route.naddr),\n        quota = publicPublicationQuotaRepository(publications);\n      if (!reference || !quota) return unavailable('source');\n      const inspected = await inspectPublicStorage(quota);\n      if (!current() || !inspected.ok) return unavailable('source');\n      const inventory = publicInventorySnapshot(quota, inspected.value);\n      if (!inventory) return unavailable('source');\n      let matching:\n        Readonly<{ form: PublicDraftForm; eventId: string }> | undefined;\n      for (const item of inventory.rows) {\n        if (item.family !== 'public_operation') continue;\n        const read = await readPublicListingIntent(publications, item.id);\n        if (!current()) return unavailable('source');\n        const row =\n          read.status === 'found' &&\n          publicRecordSnapshot(read.record, owner, item.id);\n        if (\n          !row ||\n          row.family !== 'public_operation' ||\n          row.capture.kind !== 30402 ||\n          !row.artifact ||\n          row.source.type !== 'draft'\n        )\n          continue;\n        const template = inspectLocalTemplate(row.capture.wire, owner, 30402);\n        if (!template) continue;\n        const projected = projectFoodParts(template);\n        if (\n          projected.outcome !== 'focused' ||\n          projected.projection.identifier !== reference.identifier\n        )\n          continue;\n        const source = await readPublicDraft(drafts, row.source.id);\n        if (!current()) return unavailable('source');\n        if (!source.ok || source.value.revision !== row.source.revision)\n          continue;\n        const value = source.value.form,\n          p = projected.projection;\n        if (value.contactType === '' && value.contactValue !== '') continue;\n        const built = buildFoodTemplate({\n          identifier: p.identifier,\n          title: value.title,\n          description: value.description,\n          location: value.location,\n          amount: value.amount,\n          currency: value.currency,\n          unit: value.unit,\n          ...(value.quantity === '' ? {} : { quantity: value.quantity }),\n          published_at: p.published_at,\n          created_at: template.created_at,\n          status: p.status,\n          ...(value.contactType === ''\n            ? {}\n            : {\n                contact: {\n                  type: value.contactType,\n                  value: value.contactValue,\n                  public: true as const\n                }\n              })\n        });\n        if (\n          !built.ok ||\n          JSON.stringify({\n            pubkey: owner,\n            kind: 30402,\n            created_at: template.created_at,\n            tags: built.wire_parts.tags,\n            content: built.wire_parts.content\n          }) !== row.capture.wire\n        )\n          continue;\n        if (matching) return unavailable('source');\n        matching = { form: value, eventId: row.artifact.eventId };\n      }\n      if (!matching) return unavailable('source');\n      sourceForm = matching.form;\n      coordinate = route.naddr;\n      sourceEventId = matching.eventId;\n    } else {\n      const acquired = await createPublicDraftController(\n        drafts,\n        emptyForm,\n        route.mode === 'draft' ? route.draftId : undefined\n      );\n      if (!current()) {\n        if (acquired) closePublicDraftController(acquired);\n        return unavailable('resource');\n      }\n      controller = acquired;\n      if (!controller) return unavailable('resource');\n    }\n    if (!current()) return unavailable('source');\n    const value: Editor = {\n      current,\n      retire,\n      snapshot() {\n        if (!current()) {\n          retire();\n          return;\n        }\n        const local = controller && publicDraftControllerSnapshot(controller);\n        return {\n          mode: route.mode,\n          owner,\n          form: local ? local.form : { ...(sourceForm ?? emptyForm) },\n          editable: route.mode !== 'edit',\n          draftId: local?.draft?.id ?? null,\n          state: local\n            ? local.state === 'closed'\n              ? 'failed'\n              : local.state\n            : 'read-only',\n          failure: local?.failure ?? null,\n          coordinate,\n          sourceEventId\n        };\n      },\n      async edit(input) {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (!controller || route.mode === 'edit') return false;\n        if (!editPublicDraft(controller, input)) return false;\n        notify();\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      async flush() {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (!controller || route.mode === 'edit') return false;\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      subscribe(listener) {\n        if (!current()) return () => {};\n        const key = Symbol();\n        listeners.set(key, listener);\n        return () => {\n          listeners.delete(key);\n        };\n      }\n    };\n    const editor = Object.freeze({}) as ListingEditor;\n    editors.set(editor, value);\n    return { status: 'ready', editor };\n  } catch {\n    return unavailable('storage');\n  }\n}\nexport function listingEditorSnapshot(\n  editor: ListingEditor\n): ListingEditorSnapshot | undefined {\n  return editors.get(editor)?.snapshot();\n}\nexport function editListingEditor(\n  editor: ListingEditor,\n  form: unknown\n): Promise<boolean> {\n  return editors.get(editor)?.edit(form) ?? Promise.resolve(false);\n}\nexport function flushListingEditor(editor: ListingEditor): Promise<boolean> {\n  return editors.get(editor)?.flush() ?? Promise.resolve(false);\n}\nexport function closeListingEditor(editor: ListingEditor): void {\n  editors.get(editor)?.retire();\n}\nexport function subscribeListingEditor(\n  editor: ListingEditor,\n  listener: () => void\n): () => void {\n  return editors.get(editor)?.subscribe(listener) ?? (() => {});\n}\n",
            sha256:
              'f2f1be3c614c861f50293f33f5bf1d3ae1851f4b4452fb9e2f3a7b8963a1d1f9'
          }
        };
        for (const [name, value] of Object.entries(historical)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        await copyDependencies(t, base, f.web);
        await refresh(f);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.equal(Object.keys(manifest).length, 43);
        assert.ok(
          Object.values(manifest).some((row) => row.name === 'view-context')
        );
        assert.ok((await f.audit()).length > 0);
        const handles = Object.values(manifest).find(
          (row) => row.name === 'private-handles'
        );
        assert.ok(
          handles,
          'Historical HCP115 keeps its exact private-handles compiler role'
        );
        handles.imports = ['forged-compiler-edge'];
        await writeFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          JSON.stringify(manifest)
        );
        await assert.rejects(f.audit, /compiler/);
      }
    );
    await test(
      'public-only owned lifecycle remains independently compilable',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        for (const [name, value] of Object.entries(historicalPublicOnly)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        for (const [name, value] of Object.entries(historicalPublicProduct)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        await copyDependencies(t, base, f.web);
        await refresh(f);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.ok(
          !Object.values(manifest).some(
            (record) =>
              record.name === 'view-context' || record.name === 'CapabilityGate'
          )
        );
        assert.ok(
          Object.values(manifest).some((record) => record.name === 'budgets')
        );
        assert.equal((await f.audit()).length, 47);
        const manifestFile = path.join(
          f.web,
          '.svelte-kit/output/client/.vite/manifest.json'
        );
        const originalManifest = await readFile(manifestFile);
        for (const mutation of ['shared-edge', 'button-import']) {
          await t.test(
            `public-only listing compatibility rejects extra ${mutation}`,
            { timeout: 1800000 },
            async () => {
              const changed = JSON.parse(originalManifest.toString());
              const records = Object.entries(changed);
              const additional = records.find(
                ([, record]) => record.name === 'entry/payload'
              );
              const target = records.find(
                ([, record]) =>
                  record.name ===
                  (mutation === 'shared-edge' ? 'Disclosure' : 'Button')
              );
              assert.ok(additional);
              assert.ok(target);
              target[1].imports.push(additional[0]);
              try {
                await writeFile(manifestFile, JSON.stringify(changed));
                await assert.rejects(
                  f.audit(),
                  mutation === 'shared-edge'
                    ? /Invalid owned shared compiler dependencies/
                    : /Invalid owned public-only Button compiler dependencies/
                );
              } finally {
                await writeFile(manifestFile, originalManifest);
              }
            }
          );
        }
      }
    );
    await test(
      'predecessor SDK graph remains qualified with the historical search form fixture',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        // This historical graph predates identity activation. Restore its
        // immutable public-only producers before compiling the former routes.
        for (const [name, value] of Object.entries(historicalPublicOnly))
          await writeFile(path.join(f.web, name), value.source);
        // Immutable former route bytes are a labelled verification variant only.
        const historical =
          "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import SearchForm from '../../lib/components/SearchForm.svelte';\n  import {\n    normalizePublicQuery,\n    queryErrorMessage,\n    readPublicQuery\n  } from '../../lib/catalog/query-input';\n  import type { PublicQuery } from '../../lib/catalog/query-input';\n  import { searchHref } from '../../lib/routes';\n  import { internalHref } from '../../lib/navigation-url';\n  let query = $state<PublicQuery>(normalizePublicQuery(''));\n  let error = $state<string | undefined>();\n  $effect(() => {\n    query = readPublicQuery(page.url);\n    error = undefined;\n  });\n  function search(input: string) {\n    const next = normalizePublicQuery(input);\n    if (!next.ok) {\n      error = queryErrorMessage(next.error);\n      return;\n    }\n    error = undefined;\n    const target = internalHref(searchHref(next.text));\n    if (target === undefined) {\n      error = queryErrorMessage('invalid_query');\n      return;\n    }\n    globalThis.location.assign(target);\n  }\n</script>\n\n<svelte:head><title>Search food \u2014 HarvestCircle</title></svelte:head>\n<div class=\"page page--reading stack\">\n  <h1>Search food</h1>\n  <SearchForm\n    onsubmit={search}\n    initialValue={query.ok ? query.text : ''}\n    error={error ?? (query.ok ? undefined : queryErrorMessage(query.error))}\n  />\n  <p class=\"notice\">Search data is unavailable during development.</p>\n</div>\n";
        await writeFile(
          path.join(f.web, 'src/routes/search/+page.svelte'),
          historical
        );
        await writeFile(
          path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
          historicalProduct
        );
        await refresh(f);
        await copyDependencies(t, base, f.web);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.ok(
          Object.values(manifest).some((record) => record.name === 'dist')
        );
        assert.ok(
          !Object.values(manifest).some(
            (record) =>
              record.name === 'Disclosure' || record.name === 'navigation'
          )
        );
        assert.equal((await f.audit()).length, 45);
      }
    );
    await test(
      'HCP043 search graph remains qualified with the exact historical product shell',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        for (const [name, value] of Object.entries(historicalPublicOnly))
          await writeFile(path.join(f.web, name), value.source);
        await writeFile(
          path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
          historicalProduct
        );
        await refresh(f);
        await copyDependencies(t, base, f.web);
        compile(t, f.web);
        assert.equal((await f.audit()).length, 46);
      }
    );
    await test(
      'actual output qualification',
      { timeout: 1800000 },
      async (t) => {
        /** @param {string} name @param {(context: import('node:test').TestContext) => Promise<void>} run @param {number} [timeout] */
        const check = (name, run, timeout = 1800000) =>
          t.test(name, { timeout }, run);
        await check(
          'actual compiled payload passes and build dispatch invokes the guard last',
          async (t) => {
            const f = await fixture(t);
            const files = await f.audit();
            // Exact eleven-route module/page/static admission; CSS stays once-imported.
            assert.equal(files.length, 55);
            // The actual SDK payload crosses the reader scratch boundary; a reused
            // scratch buffer must still preserve every compiler/static byte exactly.
            const sizes = await Promise.all(
              files
                .filter((name) => name.endsWith('.js'))
                .map(
                  async (name) =>
                    (await lstat(path.join(f.web, 'build', name))).size
                )
            );
            assert.ok(sizes.some((bytes) => bytes > 64 * 1024));
            assert.ok(files.includes('search.html'));
            assert.equal(
              files.filter((name) => name.endsWith('.css')).length,
              1
            );
            assert.ok(
              files.some((name) =>
                /^_app\/immutable\/assets\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.css$/.test(
                  name
                )
              )
            );
            assert.ok(files.includes('200.html'));
            const pkg = JSON.parse(
              await readFile(path.join(f.web, 'package.json'), 'utf8')
            );
            assert.match(
              pkg.scripts.build,
              /vite build && node tools\/check-output\.mjs$/
            );
          }
        );

        for (const sourceName of [
          'src/routes/messages/+page.svelte',
          'src/lib/components/InboxStatus.svelte',
          'src/lib/messaging/inbox-view.ts',
          'src/lib/messaging/inbox-sync.ts',
          'src/lib/messaging/decryption-queue.ts',
          'src/lib/components/ConversationRow.svelte',
          'src/lib/messaging/inbox-list.ts',
          'src/lib/components/ConversationThread.svelte',
          'src/lib/components/MessageItem.svelte',
          'src/lib/components/ConversationOptions.svelte',
          'src/lib/messaging/cited-product.ts',
          'src/lib/nostr/enquiry-references.ts',
          'src/lib/components/MessageComposer.svelte',
          'src/lib/messaging/reply-controller.ts',
          'src/lib/messaging/rumor-plan.ts',
          'src/lib/messaging/conversation-thread.ts',
          'src/lib/persistence/private-storage.ts',
          'src/routes/messages/[conversationId=local_id]/+page.svelte'
        ])
          await check(
            'linked inbox presentation requires exact owned source ' +
              sourceName,
            async (t) => {
              const f = await fixture(t),
                file = path.join(f.web, sourceName);
              await writeFile(
                file,
                (await readFile(file, 'utf8')) +
                  '\n// changed presentation source\n'
              );
              // Match the actual altered source identity before probing the
              // independent exact producer pin. The stale identity guard stays.
              await refresh(f);
              await assert.rejects(f.audit, /inbox presentation source/);
            }
          );
        for (const mutation of [
          'missing-private-cache-directory',
          'extra-private-edge',
          'missing-private-edge',
          'private-dynamic',
          'publisher-extra-edge'
        ])
          await check(
            'actual inbox compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const find = (/** @type {string} */ name) => {
                const row = Object.entries(manifest).find(
                  ([, v]) => v.name === name
                );
                assert.ok(row);
                return row;
              };
              const [key, row] = find('InboxStatus');
              if (mutation === 'missing-private-cache-directory')
                delete manifest[key];
              if (mutation === 'extra-private-edge')
                row.imports = row.imports.concat(find('references')[0]);
              if (mutation === 'missing-private-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'private-dynamic')
                row.dynamicImports = [find('read')[0]];
              if (mutation === 'publisher-extra-edge')
                find('publishers')[1].imports = find(
                  'publishers'
                )[1].imports.concat(find('references')[0]);
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );

        for (const mutation of [
          'directory-missing-context',
          'directory-entry-role',
          'directory-duplicate-role',
          'thread-missing-edge',
          'thread-dynamic',
          'thread-entry-role',
          'reply-missing-navigation',
          'reply-missing-owner-key',
          'context-resolver-dynamic',
          'context-resolver-entry',
          'enquiry-view-dynamic',
          'enquiry-view-entry',
          'enquiry-setup-dynamic',
          'enquiry-setup-entry',
          'enquiry-gate-dynamic',
          'enquiry-gate-entry',
          'enquiry-root-missing-gate',
          'enquiry-root-missing-css',
          'enquiry-root-extra-css'
        ])
          await check(
            'actual inbox list compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) =>
                  row.name ===
                  (mutation.startsWith('enquiry-view-')
                    ? 'inbox-view'
                    : mutation.startsWith('enquiry-setup-')
                      ? 'InboxSetup'
                      : mutation.startsWith('enquiry-gate-')
                        ? 'InboxSetup'
                        : mutation.startsWith('enquiry-root-')
                          ? 'nodes/0'
                          : mutation.startsWith('context-')
                            ? 'publishers'
                            : mutation.startsWith('reply-')
                              ? 'MessageComposer'
                              : mutation.startsWith('thread-')
                                ? 'nodes/5'
                                : 'InboxStatus')
              );
              assert.ok(entry);
              const [, row] = entry;
              if (mutation === 'enquiry-root-missing-css') row.css = [];
              if (mutation === 'enquiry-root-extra-css')
                row.css = [...row.css, row.css[0]];
              if (
                mutation.startsWith('enquiry-') &&
                mutation.endsWith('-dynamic')
              )
                row.dynamicImports = [];
              if (
                mutation.startsWith('enquiry-') &&
                mutation.endsWith('-entry')
              )
                row.isEntry = true;
              if (mutation === 'enquiry-root-missing-gate')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    !['view-context', 'CapabilityGate'].includes(
                      manifest[value].name
                    )
                );
              if (mutation === 'context-resolver-dynamic')
                row.dynamicImports = [];
              if (mutation === 'context-resolver-entry') row.isEntry = true;
              if (mutation === 'directory-missing-context')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'InboxSetup'
                );
              if (mutation === 'directory-entry-role') row.isEntry = true;
              if (mutation === 'directory-duplicate-role')
                manifest['_forged_directory.js'] = { ...row };
              if (mutation === 'thread-missing-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'thread-dynamic')
                row.dynamicImports = [
                  Object.keys(manifest).find(
                    (key) => manifest[key].name === 'read'
                  )
                ];
              if (mutation === 'thread-entry-role') row.isEntry = false;
              if (mutation === 'reply-missing-navigation')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'navigation'
                );
              if (mutation === 'reply-missing-owner-key') {
                const key = Object.keys(manifest).find(
                  (key) => manifest[key].name === 'public-key'
                );
                assert.ok(
                  key && row.imports.includes(key),
                  'Mutate an actual current owner-key compiler edge'
                );
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'public-key'
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|duplicate|inventory/);
            }
          );

        for (const name of [
          'components/InboxSetup.svelte',
          'messaging/inbox-setup-view.ts',
          'runtime/view-context.ts'
        ])
          await check(
            'linked inbox setup requires exact owned source ' + name,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/lib', name);
              await writeFile(
                file,
                (await readFile(file, 'utf8')) + '\n// changed source\n'
              );
              await refresh(f);
              await assert.rejects(f.audit, /inbox compiler source/);
            }
          );
        for (const name of [
          'public-key',
          'public-runtime',
          'AccountGate',
          'Disclosure'
        ])
          await check(
            'linked inbox setup rejects changed exact compiler edge ' + name,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) => row.name === name
              );
              assert.ok(entry, 'Missing actual linked inbox compiler role');
              entry[1].imports = ['forged-compiler-edge'];
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler/);
            }
          );
        await check(
          'owned root preserves its lifecycle across a 64 KiB read boundary',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'src/routes/+layout.svelte');
            const layout = await readFile(file, 'utf8');
            // A large harmless comment after the real lifecycle must not overwrite its
            // first read chunk when the file reader reuses a scratch buffer.
            const expanded = layout.replace(
              '</script>',
              '/*' + 'owned reader boundary '.repeat(4000) + '*/\n</script>'
            );
            assert.ok(Buffer.byteLength(expanded) > 64 * 1024);
            await writeFile(file, expanded);
            await refresh(f);
            assert.equal((await f.audit()).length, 55);
          }
        );

        for (const mutation of [
          'unknown-name',
          'missing-public-key',
          'missing-dist',
          'missing-negentropy',
          'missing-root-edge',
          'random-key',
          'wrong-src',
          'wrong-version',
          'wrong-peer',
          'wrong-basename',
          'wrong-dynamic-flag',
          'entry-flag',
          'css',
          'wrong-import',
          'chunk-src',
          'chunk-entry'
        ])
          await check(
            'owned root SDK compiler admission rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const m = JSON.parse(await readFile(file, 'utf8'));
              const root = Object.values(m).find((v) => v.name === 'nodes/0');
              const dist = Object.entries(m).find(
                ([, v]) =>
                  v.name ===
                  (Object.values(m).some((row) => row.name === 'public-runtime')
                    ? 'public-runtime'
                    : 'Disclosure')
              );
              assert.ok(
                m[sdkKey] && root && dist,
                'Actual successor SDK identities'
              );
              if (mutation === 'unknown-name')
                m[sdkKey].name = 'unapproved-sdk';
              if (
                mutation.startsWith('missing-') &&
                mutation !== 'missing-root-edge'
              ) {
                const entry = Object.entries(m).find(
                  ([, v]) =>
                    v.name ===
                    (mutation === 'missing-dist'
                      ? dist[1].name
                      : mutation.slice(8))
                );
                assert.ok(entry);
                delete m[entry[0]];
              }
              if (mutation === 'missing-root-edge') dist[1].dynamicImports = [];
              if (
                [
                  'random-key',
                  'wrong-version',
                  'wrong-peer',
                  'wrong-basename'
                ].includes(mutation)
              ) {
                const next =
                  mutation === 'random-key'
                    ? '_HCrandom.js'
                    : sdkKey.replace(
                        mutation === 'wrong-version'
                          ? '6.2.1'
                          : mutation === 'wrong-peer'
                            ? '6.0.3'
                            : 'negentropy.js',
                        mutation === 'wrong-basename' ? 'other.js' : '0.0.0'
                      );
                m[next] = m[sdkKey];
                delete m[sdkKey];
                m[next].src = next;
                dist[1].dynamicImports = dist[1].dynamicImports.map(
                  (/** @type {string} */ k) => (k === sdkKey ? next : k)
                );
              }
              if (mutation === 'wrong-src') m[sdkKey].src = '_HCwrong.js';
              if (mutation === 'wrong-dynamic-flag')
                m[sdkKey].isDynamicEntry = false;
              if (mutation === 'entry-flag') m[sdkKey].isEntry = false;
              if (mutation === 'css') m[sdkKey].css = [];
              if (mutation === 'wrong-import')
                m[sdkKey].imports = [
                  Object.keys(m).find((k) => m[k].name === 'public-key')
                ];
              if (mutation === 'chunk-src') dist[1].src = dist[0];
              if (mutation === 'chunk-entry') dist[1].isEntry = false;
              await writeFile(file, JSON.stringify(m));
              await assert.rejects(f.audit, /compiler|runtime/i);
            }
          );
        for (const mutation of [
          'copied-files-only',
          'comment-only',
          'string-only',
          'shadowed',
          'rebound',
          'aliased',
          'missing-cleanup',
          'missing-context',
          'wrong-context'
        ])
          await check(
            'root runtime activation rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/routes/+layout.svelte');
              let layout = await readFile(file, 'utf8');
              if (
                ['copied-files-only', 'comment-only', 'string-only'].includes(
                  mutation
                )
              ) {
                const script = layout.slice(
                  layout.indexOf('>') + 1,
                  layout.indexOf('</script>')
                );
                layout =
                  '<script lang="ts">' +
                  (mutation === 'comment-only'
                    ? '/*' + script + '*/'
                    : mutation === 'string-only'
                      ? 'const text = ' + JSON.stringify(script) + ';'
                      : '') +
                  '</script><main>Controlled root</main>';
              }
              if (mutation === 'shadowed')
                layout = layout.replace(
                  'onMount(() => {',
                  'onMount((publicContext) => {'
                );
              if (mutation === 'rebound')
                layout = layout.replace(
                  'const publicContext',
                  'let publicContext'
                );
              if (mutation === 'aliased')
                layout = layout
                  .replace(
                    '    mountPublicRuntime,',
                    '    mountPublicRuntime as otherMount,'
                  )
                  .replace(
                    'mountPublicRuntime(publicContext)',
                    'otherMount(publicContext)'
                  );
              if (mutation === 'missing-cleanup')
                layout = layout.replace(
                  'return () => closePublicRuntime(publicContext);',
                  'return () => {};'
                );
              if (mutation === 'missing-context')
                layout = layout.replace(
                  'setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);',
                  ''
                );
              if (mutation === 'wrong-context')
                layout = layout.replace(
                  'closePublicRuntime(publicContext)',
                  'closePublicRuntime({})'
                );
              await writeFile(file, layout);
              await refresh(f);
              await assert.rejects(f.audit, /compiler|runtime/i);
            }
          );
        for (const mutation of [
          'missing-mount',
          'missing-subscription',
          'missing-unsubscribe',
          'missing-close',
          'wrong-context',
          'duplicate-context',
          'extra-mount',
          'aliased-mount',
          'shadowed',
          'missing-context',
          'rebound-context'
        ])
          await check(
            'owned identity lifecycle rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/routes/+layout.svelte');
              const original = await readFile(file, 'utf8');
              let layout = original;
              assert.ok(layout.includes('mountIdentityView(identityContext);'));
              if (mutation === 'missing-mount')
                layout = layout.replace(
                  'mountIdentityView(identityContext);',
                  ''
                );
              if (mutation === 'missing-subscription')
                layout = layout.replace(
                  'subscribeIdentityView(identityContext,',
                  'subscribeIdentityView({},'
                );
              if (mutation === 'missing-unsubscribe')
                layout = layout.replace('      off();', '');
              if (mutation === 'missing-close')
                layout = layout.replace(
                  'closeIdentityView(identityContext);',
                  ''
                );
              if (mutation === 'wrong-context')
                layout = layout.replace(
                  'closeIdentityView(identityContext)',
                  'closeIdentityView({})'
                );
              if (mutation === 'duplicate-context')
                layout = layout.replace(
                  'const identityContext = createIdentityViewContext();',
                  'const identityContext = createIdentityViewContext(); const other = createIdentityViewContext();'
                );
              if (mutation === 'extra-mount')
                layout = layout.replace(
                  'mountIdentityView(identityContext);',
                  'mountIdentityView(identityContext); mountIdentityView(identityContext);'
                );
              if (mutation === 'aliased-mount')
                layout = layout
                  .replace(
                    '    mountIdentityView,',
                    '    mountIdentityView as otherMount,'
                  )
                  .replace(
                    'mountIdentityView(identityContext);',
                    'otherMount(identityContext);'
                  );
              if (mutation === 'shadowed')
                layout = layout.replace(
                  'const off = subscribeIdentityView',
                  'const identityContext = {}; const off = subscribeIdentityView'
                );
              if (mutation === 'missing-context')
                layout = layout.replace(
                  'setContext(IDENTITY_VIEW_CONTEXT, identityContext);',
                  ''
                );
              if (mutation === 'rebound-context')
                layout = layout.replace(
                  'const identityContext =',
                  'let identityContext ='
                );
              assert.notEqual(layout, original);
              await writeFile(file, layout);
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned root runtime activation/
              );
            }
          );
        for (const mutation of [
          'missing-view',
          'missing-heads',
          'extra-view-field',
          'wrong-view-imports',
          'wrong-heads-imports',
          'missing-root-edge',
          'missing-gate-edge'
        ])
          await check(
            'owned identity compiler topology rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              /** @param {string} name */
              const find = (name) => {
                const entry = Object.entries(manifest).find(
                  ([, record]) => record.name === name
                );
                assert.ok(entry, 'Missing observed compiler role: ' + name);
                return entry;
              };
              const identityCompilerRole = Object.values(manifest).some(
                (record) => record.name === 'identity-session'
              )
                ? 'identity-session'
                : Object.values(manifest).some(
                      (record) => record.name === 'public-runtime'
                    )
                  ? 'public-runtime'
                  : 'heads';
              const [identityKey, identity] = find(
                  Object.values(manifest).some(
                    (record) => record.name === 'view-context'
                  )
                    ? 'view-context'
                    : 'CapabilityGate'
                ),
                [headsKey, heads] = find(identityCompilerRole);
              if (mutation === 'missing-view') delete manifest[identityKey];
              if (mutation === 'missing-heads') delete manifest[headsKey];
              if (mutation === 'extra-view-field') identity.dynamicImports = [];
              if (mutation === 'wrong-view-imports') identity.imports = [];
              if (mutation === 'wrong-heads-imports') heads.imports = [];
              if (
                mutation === 'missing-root-edge' ||
                mutation === 'missing-gate-edge'
              ) {
                const [, consumer] = find(
                  mutation === 'missing-root-edge' ? 'nodes/0' : 'AccountGate'
                );
                consumer.imports = consumer.imports.filter(
                  /** @param {string} dependency */
                  (dependency) => dependency !== identityKey
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler/i);
            }
          );
        for (const name of [
          'runtime/public-runtime.ts',
          'nostr/request-scope.ts',
          'nostr/public-pool.ts',
          'nostr/public-store.ts',
          'nostr/request-result.ts',
          'nostr/ingress.ts',
          'nostr/verified-envelope.ts',
          'nostr/envelope-bounds.ts',
          'nostr/exports.ts',
          'catalog/observations.ts',
          'config/budgets.ts',
          'config/relays.ts',
          'config/deployment-relays.ts'
        ])
          await check(
            'root runtime admission requires owned ' + name,
            async (t) => {
              const f = await fixture(t);
              await rm(path.join(f.web, 'src/lib', name));
              await refresh(f);
              await assert.rejects(f.audit);
            }
          );
        for (const routeSource of [
          'search/+page.svelte',
          'messages/+page.svelte'
        ])
          await check(
            routeSource +
              ' admission requires its owned source and exact module/page counterparts',
            async (t) => {
              const f = await fixture(t);
              const source = path.join(f.web, 'src/routes', routeSource);
              const original = await readFile(source);
              await rm(source);
              git(f.root, 'add', '--', 'web/src/routes/' + routeSource);
              await generateBuildInfo(f.web);
              const metadata = await readFile(
                path.join(f.web, 'static/build-info.json')
              );
              await writeFile(
                path.join(f.web, 'build/build-info.json'),
                metadata
              );
              await writeFile(
                path.join(f.web, '.svelte-kit/output/client/build-info.json'),
                metadata
              );
              await assert.rejects(f.audit, /Undeclared compiler module/);
              await writeFile(source, original);
            }
          );
        for (const mutation of [
          'unknown-name',
          'duplicate',
          'missing-module',
          'missing-page',
          'changed-page'
        ]) {
          await check(
            'exact search compiler admission rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, record]) => record.name === 'routes'
              );
              assert.ok(entry, 'The actual shared routes module must exist');
              const [key, module] = entry;
              if (mutation === 'unknown-name')
                module.name = 'unapproved-routes';
              if (mutation === 'duplicate') manifest.duplicate = { ...module };
              if (mutation === 'missing-module') delete manifest[key];
              if (mutation === 'missing-page')
                await rm(path.join(f.web, 'build/search.html'));
              if (mutation === 'changed-page')
                await writeFile(
                  path.join(f.web, 'build/search.html'),
                  '<h1>Different source</h1>'
                );
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit);
            }
          );
        }
        for (const mutation of ['missing', 'changed']) {
          await check(
            'full-route static robots rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const target = path.join(f.web, 'build/robots.txt');
              if (mutation === 'missing') await rm(target);
              else await writeFile(target, 'User-agent: *\nAllow: /\n');
              await assert.rejects(f.audit);
            }
          );
        }
        await check(
          'full-route compiler rejects an unowned thirteenth node',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const entry = Object.entries(manifest).find(
              ([, record]) => record.name === 'nodes/12'
            );
            assert.ok(entry);
            entry[1].name = 'nodes/13';
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(f.audit, /Undeclared compiler module/);
          }
        );
        await check(
          'actual shared budgets chunk has owned consumers and counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'Button'
            );
            assert.equal(entries.length, 1);
            const [key, budget] = entries[0];
            assert.deepEqual(Object.keys(budget).sort(), [
              'file',
              'imports',
              'name'
            ]);
            const client = Object.entries(manifest).find(
              ([, record]) => record.name === 'client'
            );
            assert.ok(client);
            assert.deepEqual(budget.imports, [client[0]]);
            for (const name of [
              Object.values(manifest).some(
                (row) => row.name === 'public-runtime'
              )
                ? 'public-runtime'
                : 'Disclosure',
              'nodes/9',
              'routes'
            ])
              assert.ok(
                Object.values(manifest)
                  .find((record) => record.name === name)
                  .imports.includes(key)
              );
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', budget.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', budget.file)
              )
            );
            assert.ok((await f.audit()).includes(budget.file));
          }
        );
        for (const name of [
          'ListingEditor',
          'public-runtime',
          'nodes/8',
          'nodes/10',
          'nodes/12'
        ]) {
          for (const mutation of [
            'missing',
            'missing-edge',
            'extra-edge',
            'duplicate-edge',
            'permuted-edges',
            'dynamic',
            'entry',
            'css'
          ]) {
            await check(
              'exact listing editor compiler rejects ' + name + ' ' + mutation,
              async (t) => {
                const f = await fixture(t),
                  file = path.join(
                    f.web,
                    '.svelte-kit/output/client/.vite/manifest.json'
                  );
                const manifest = JSON.parse(await readFile(file, 'utf8'));
                const entry = Object.entries(manifest).find(
                  ([, row]) => row.name === name
                );
                assert.ok(entry);
                const [key, row] = entry;
                if (mutation === 'missing') delete manifest[key];
                if (mutation === 'missing-edge')
                  row.imports = row.imports.slice(1);
                if (mutation === 'extra-edge')
                  row.imports = row.imports.concat(
                    Object.keys(manifest).find(
                      (key) => manifest[key].name === 'AccountGate'
                    )
                  );
                if (mutation === 'duplicate-edge')
                  row.imports = row.imports.concat(row.imports[0]);
                if (mutation === 'permuted-edges')
                  row.imports = [...row.imports].reverse();
                if (mutation === 'dynamic')
                  row.dynamicImports = [row.imports[0]];
                if (mutation === 'entry') {
                  if (row.isEntry === true) delete row.isEntry;
                  else row.isEntry = true;
                }
                if (mutation === 'css') row.css = [];
                await writeFile(file, JSON.stringify(manifest));
                await assert.rejects(f.audit, /compiler|references|import/);
              }
            );
          }
        }
        for (const producer of [
          'src/routes/sell/+page.svelte',
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
          'src/routes/products/[naddr=naddr]/edit/+page.svelte',
          'src/lib/components/ListingEditor.svelte',
          'src/lib/publishing/listing-editor.ts',
          'src/lib/components/ListingReview.svelte',
          'src/lib/publishing/editor-workflow.ts',
          'src/lib/components/ConflictNotice.svelte',
          'src/lib/publishing/edit-source.ts'
        ]) {
          await check(
            'listing editor rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// owned editor source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned listing editor source/
              );
            }
          );
        }
        for (const route of [
          'src/routes/sell/+page.svelte',
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
          'src/routes/products/[naddr=naddr]/edit/+page.svelte'
        ]) {
          await check(
            'listing editor rejects partial three-route activation ' + route,
            async (t) => {
              const f = await fixture(t);
              const historical = historicalPublicOnly[route];
              assert.equal(
                createHash('sha256').update(historical.source).digest('hex'),
                historical.sha256
              );
              await writeFile(path.join(f.web, route), historical.source);
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned listing editor activation/
              );
            }
          );
        }
        for (const producer of [
          'src/routes/products/[naddr=naddr]/+page.svelte',
          'src/lib/catalog/product-view.ts',
          'src/lib/components/ProductEnquiry.svelte',
          'src/lib/messaging/product-enquiry.ts',
          'src/lib/messaging/product-request-controller.ts',
          'src/lib/messaging/enquiry-readiness.ts',
          'src/lib/messaging/resolve-inbox.ts',
          'src/lib/components/CapabilityGate.svelte',
          'src/lib/messaging/enquiry-context.ts',
          'src/lib/components/ListingFacts.svelte',
          'src/lib/components/PublisherIdentity.svelte',
          'src/lib/contracts/food-availability-v1/contact-read.ts',
          'src/lib/navigation-copy.ts',
          'src/lib/runtime/public-runtime.ts',
          'src/lib/catalog/publishers.ts',
          'src/lib/catalog/resolve-head.ts'
        ]) {
          await check(
            'real product compiler rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned product presentation source/
              );
            }
          );
        }
        for (const mutation of [
          'permuted-edges',
          'extra-edge',
          'duplicate-edge'
        ]) {
          await check(
            'product compiler dependency order: ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const find = (/** @type {string} */ name) => {
                const row = Object.entries(manifest).find(
                  ([, v]) => v.name === name
                );
                assert.ok(row);
                return row;
              };
              for (const name of ['nodes/7', 'nodes/9']) {
                const row = find(name)[1];
                row.imports = [...row.imports].reverse();
                if (mutation === 'extra-edge')
                  row.imports = row.imports.concat(find('AccountGate')[0]);
                if (mutation === 'duplicate-edge')
                  row.imports = row.imports.concat(row.imports[0]);
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );
        }
        for (const mutation of [
          'missing',
          'extra-edge',
          'missing-edge',
          'duplicate-edge',
          'dynamic',
          'entry',
          'css'
        ]) {
          await check(
            'inline request shared composer compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) => row.name === 'MessageComposer'
              );
              assert.ok(entry);
              const [key, row] = entry;
              if (mutation === 'missing') delete manifest[key];
              if (mutation === 'extra-edge')
                row.imports = row.imports.concat(
                  Object.keys(manifest).find(
                    (name) => manifest[name].name === 'AccountGate'
                  )
                );
              if (mutation === 'missing-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'duplicate-edge')
                row.imports = row.imports.concat(row.imports[0]);
              if (mutation === 'dynamic') row.dynamicImports = [];
              if (mutation === 'entry') row.isEntry = true;
              if (mutation === 'css') row.css = [];
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );
        }
        for (const mutation of [
          'missing-publishers',
          'publisher-budget',
          'publisher-shared',
          'publisher-dynamic',
          'product-shared',
          'product-publisher',
          'product-budget',
          'product-dynamic',
          'search-publisher'
        ]) {
          await check(
            'real product compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const records = Object.entries(manifest);
              const find = (/** @type {string} */ name) => {
                const row = records.find(([, v]) => v.name === name);
                assert.ok(row);
                return row;
              };
              const [publisherKey, publisher] = find('publishers');
              if (mutation === 'missing-publishers')
                delete manifest[publisherKey];
              else if (mutation === 'publisher-dynamic')
                publisher.dynamicImports = [find('client-entry')[0]];
              else if (mutation === 'product-dynamic')
                find('nodes/7')[1].dynamicImports = [find('client-entry')[0]];
              else if (
                mutation === 'publisher-budget' ||
                mutation === 'publisher-shared'
              )
                publisher.imports = publisher.imports.filter(
                  (/** @type {string} */ key) =>
                    key !==
                    find(
                      mutation === 'publisher-budget'
                        ? 'Button'
                        : Object.values(manifest).some(
                              (row) => row.name === 'public-runtime'
                            )
                          ? 'public-runtime'
                          : 'Disclosure'
                    )[0]
                );
              else {
                const target =
                  mutation === 'search-publisher'
                    ? 'nodes/9'
                    : mutation === 'product-publisher'
                      ? 'MessageComposer'
                      : 'nodes/7';
                const dependency =
                  mutation === 'product-shared'
                    ? 'Disclosure'
                    : mutation === 'product-budget'
                      ? 'Button'
                      : 'publishers';
                find(target)[1].imports = find(target)[1].imports.filter(
                  (/** @type {string} */ key) => key !== find(dependency)[0]
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|budgets/);
            }
          );
        }
        for (const producer of [
          'src/routes/search/+page.svelte',
          'src/lib/catalog/search-view.ts',
          'src/lib/components/ListingRow.svelte',
          'src/lib/components/SourceStatus.svelte',
          'src/lib/navigation-scroll.ts',
          'src/lib/components/primitives/Disclosure.svelte'
        ]) {
          await check(
            'real search compiler rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned search presentation source/
              );
            }
          );
        }
        for (const mutation of [
          'missing-shared',
          'missing-navigation',
          'root-edge',
          'search-edge',
          'budget-edge',
          'negentropy-edge',
          'sdk-backedge',
          'navigation-edge',
          'navigation-import',
          'navigation-dynamic',
          'shared-import'
        ]) {
          await check('real search compiler rejects ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const records = Object.entries(manifest);
            const find = (/** @type {string} */ name) => {
              const entry = records.find(([, v]) => v.name === name);
              assert.ok(entry, 'Missing actual compiler record ' + name);
              return entry;
            };
            const [sharedKey, shared] = find(
              Object.values(manifest).some(
                (row) => row.name === 'public-runtime'
              )
                ? 'public-runtime'
                : 'Disclosure'
            );
            const [budgetKey] = find('Button');
            if (mutation === 'missing-shared') delete manifest[sharedKey];
            else if (mutation === 'missing-navigation')
              delete manifest[find('navigation')[0]];
            else if (mutation === 'root-edge')
              find('nodes/0')[1].imports = find('nodes/0')[1].imports.filter(
                (/** @type {string} */ k) => k !== sharedKey
              );
            else if (mutation === 'search-edge')
              find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
                (/** @type {string} */ k) => k !== sharedKey
              );
            else if (mutation === 'budget-edge')
              shared.imports = shared.imports.filter(
                (/** @type {string} */ k) => k !== budgetKey
              );
            else if (mutation === 'sdk-backedge')
              find('negentropy')[1].imports = [];
            else if (mutation === 'navigation-edge')
              find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
                (/** @type {string} */ k) => k !== find('navigation')[0]
              );
            else if (mutation === 'navigation-import')
              find('navigation')[1].imports = [find('client')[0]];
            else if (mutation === 'navigation-dynamic')
              find('navigation')[1].dynamicImports = [find('client-entry')[0]];
            else if (mutation === 'shared-import')
              shared.imports = shared.imports.filter(
                (/** @type {string} */ k) => k !== find('public-key')[0]
              );
            else shared.dynamicImports = [];
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(f.audit, /compiler|SDK|budgets/);
          });
        }
        for (const mutation of [
          'unknown-name',
          'duplicate',
          'missing-module',
          'extra-field',
          'missing-root-edge',
          'missing-routes-edge',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('budgets module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const entry = Object.entries(manifest).find(
              ([, record]) => record.name === 'Button'
            );
            assert.ok(entry);
            const [key, budget] = entry;
            if (mutation === 'unknown-name') budget.name = 'unapproved-budgets';
            if (mutation === 'duplicate')
              manifest['_HCduplicate.js'] = { ...budget };
            if (mutation === 'missing-module') delete manifest[key];
            if (mutation === 'extra-field') budget.dynamicImports = [];
            if (
              mutation === 'missing-root-edge' ||
              mutation === 'missing-routes-edge'
            ) {
              const name =
                mutation === 'missing-root-edge' ? 'nodes/9' : 'routes';
              const consumer = Object.values(manifest).find(
                (record) => record.name === name
              );
              consumer.imports = consumer.imports.filter(
                /** @param {string} dependency */
                (dependency) => dependency !== key
              );
            }
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', budget.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', budget.file),
                'changed counterpart'
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              /Undeclared compiler module|Unresolved compiler import|Invalid owned budgets compiler|Invalid owned listing editor compiler graph|Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        await check(
          'actual route-state compiler module has an owned client counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'state'
            );
            assert.equal(entries.length, 1);
            const [key, state] = entries[0];
            assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
            assert.ok(
              state.imports.some(
                /** @param {string} dependency */
                (dependency) => manifest[dependency].name === 'client.svelte'
              )
            );
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', state.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', state.file)
              )
            );
            assert.ok((await f.audit()).includes(state.file));
          }
        );
        for (const mutation of [
          'unknown-name',
          'duplicate-state',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('state module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const state = Object.values(manifest).find(
              (record) => record.name === 'state'
            );
            assert.ok(
              state,
              'The actual route-state module must exist before mutation'
            );
            if (mutation === 'unknown-name') state.name = 'state-unapproved';
            if (mutation === 'duplicate-state')
              manifest['_HCduplicate.js'] = { ...state };
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', state.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', state.file),
                Buffer.concat([
                  await readFile(path.join(f.web, 'build', state.file)),
                  Buffer.from('changed counterpart')
                ])
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              mutation.endsWith('name') || mutation === 'duplicate-state'
                ? /Undeclared compiler module/
                : /Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        await check(
          'actual shared Button compiler module has an owned client counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'Button'
            );
            assert.equal(entries.length, 1);
            const [key, button] = entries[0];
            assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
            assert.ok(
              button.imports.some(
                /** @param {string} dependency */
                (dependency) => manifest[dependency].name === 'client'
              )
            );
            for (const consumer of ['nodes/0', 'nodes/2']) {
              assert.ok(
                Object.values(manifest).some(
                  (record) =>
                    record.name === consumer && record.imports.includes(key)
                )
              );
            }
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', button.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', button.file)
              )
            );
            assert.ok((await f.audit()).includes(button.file));
          }
        );
        for (const mutation of [
          'unknown-name',
          'duplicate-Button',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('Button module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const state = Object.values(manifest).find(
              (record) => record.name === 'Button'
            );
            assert.ok(
              state,
              'The actual shared Button module must exist before mutation'
            );
            if (mutation === 'unknown-name') state.name = 'Button-unapproved';
            if (mutation === 'duplicate-Button')
              manifest['_HCduplicate.js'] = { ...state };
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', state.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', state.file),
                Buffer.concat([
                  await readFile(path.join(f.web, 'build', state.file)),
                  Buffer.from('changed counterpart')
                ])
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              mutation.endsWith('name') || mutation === 'duplicate-Button'
                ? /Undeclared compiler module/
                : /Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        for (const name of [
          'notes.txt',
          '_app/immutable/chunks/innocent.ABC12345.js',
          'docs/parent.html',
          'state.css',
          'source.map',
          'contracts/vector.json'
        ]) {
          await check(
            `undeclared copied output is rejected: ${name}`,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'build', name);
              await mkdir(path.dirname(file), { recursive: true });
              await writeFile(file, 'ordinary public text');
              await assert.rejects(f.audit);
            }
          );
        }
        for (const payload of [
          Buffer.from('SQLite format 3\0'),
          Buffer.from([0x7f, 0x45, 0x4c, 0x46]),
          Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
          Buffer.from('PK\x03\x04'),
          Buffer.from('MZnative'),
          Buffer.from('generated UniFFI bindings'),
          Buffer.from('HC_TEST_ONLY_SECRET'),
          Buffer.from('nsec1' + 'a'.repeat(60)),
          Buffer.from('-----BEGIN PRIVATE KEY-----'),
          Buffer.from('private_transcript: controlled disclosure'),
          Buffer.from('class TestSigner {}')
        ]) {
          await check(
            `manifest-admitted contamination ${payload.subarray(0, 18).toString('hex')}`,
            async (t) => {
              const f = await fixture(t);
              const manifest = JSON.parse(
                await readFile(
                  path.join(
                    f.web,
                    '.svelte-kit/output/client/.vite/manifest.json'
                  ),
                  'utf8'
                )
              );
              const name = Object.values(manifest).find(
                (entry) => entry.name === 'client'
              ).file;
              // Poison both sides: byte mapping alone does not qualify contents.
              for (const directory of ['build', '.svelte-kit/output/client'])
                await writeFile(path.join(f.web, directory, name), payload);
              await assert.rejects(f.audit, /contamination|binary|text/);
            }
          );
        }
        for (const name of [
          'index.html',
          '200.html',
          '_app/version.json',
          'build-info.json'
        ]) {
          await check(`missing/tampered owned output: ${name}`, async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'build', name);
            const original = await readFile(file);
            await rm(file);
            await assert.rejects(f.audit);
            await writeFile(
              file,
              Buffer.concat([original, Buffer.from('tampered')])
            );
            await assert.rejects(f.audit);
          });
        }
        for (const mutation of [
          'unknown',
          'private',
          'revision',
          'dirty',
          'lock',
          'config',
          'source',
          'generated'
        ]) {
          await check(`provenance fails closed: ${mutation}`, async (t) => {
            const f = await fixture(t);
            const metadata = JSON.parse(
              await readFile(path.join(f.web, 'build/build-info.json'), 'utf8')
            );
            if (mutation === 'unknown') metadata.web_source.unknown = 'extra';
            if (mutation === 'private')
              metadata.private_transcript = 'controlled disclosure';
            if (mutation === 'revision')
              metadata.web_source.revision = 'a'.repeat(40);
            if (mutation === 'dirty')
              metadata.web_source.dirty = !metadata.web_source.dirty;
            if (mutation === 'lock')
              await writeFile(
                path.join(f.web, 'pnpm-lock.yaml'),
                'changed lock'
              );
            if (mutation === 'config')
              await writeFile(
                path.join(f.web, 'vite.config.ts'),
                'changed config'
              );
            if (mutation === 'source')
              await writeFile(
                path.join(f.web, 'src/app.html'),
                'changed source'
              );
            if (mutation === 'generated')
              git(f.root, 'add', '-f', 'web/static/build-info.json');
            if (
              ['unknown', 'private', 'revision', 'dirty'].includes(mutation)
            ) {
              for (const directory of [
                'build',
                'static',
                '.svelte-kit/output/client'
              ])
                await writeFile(
                  path.join(f.web, directory, 'build-info.json'),
                  JSON.stringify(metadata)
                );
            }
            await assert.rejects(f.audit);
          });
        }
        for (const kind of [
          'symlink',
          'hardlink',
          'fifo',
          'directory',
          'oversize',
          'parent'
        ]) {
          await check(`unsafe output refuses ${kind} promptly`, async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'build/index.html');
            await rm(file);
            if (kind === 'symlink') await symlink('../src/app.html', file);
            if (kind === 'hardlink')
              await link(path.join(f.web, 'src/app.html'), file);
            if (kind === 'fifo') execFileSync('mkfifo', [file]);
            if (kind === 'directory') await mkdir(file);
            if (kind === 'oversize')
              await writeFile(file, Buffer.alloc(8 * 1024 * 1024 + 1));
            if (kind === 'parent') {
              await rm(path.join(f.web, 'build'), { recursive: true });
              await symlink(
                path.join(base, 'web/build'),
                path.join(f.web, 'build')
              );
            }
            await assert.rejects(f.audit);
          });
        }
        await check(
          'compiler manifest cannot authorize arbitrary resources, escapes or unresolved imports',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const original = await readFile(file, 'utf8');
            for (const mutation of [
              'extra',
              'escape',
              'reference',
              'unknown'
            ]) {
              const manifest = JSON.parse(original);
              const record = Object.values(manifest)[0];
              if (mutation === 'extra')
                manifest.extra = {
                  name: 'provider',
                  file: '_app/immutable/chunks/provider.ABC12345.js'
                };
              if (mutation === 'escape') record.file = '../outside.js';
              if (mutation === 'reference') record.imports = ['absent'];
              if (mutation === 'unknown') record.secret = 'private';
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit);
            }
          }
        );

        await check(
          'generated provenance must remain ignored even when freshly regenerated',
          async (t) => {
            const f = await fixture(t);
            await writeFile(path.join(f.web, '.gitignore'), '');
            await generateBuildInfo(f.web);
            await assert.rejects(f.audit, /must be ignored/);
          }
        );
        for (const name of [
          'static/build-info.json',
          '.svelte-kit/output/client/.vite/manifest.json',
          '.svelte-kit/output/prerendered/pages/index.html',
          '.svelte-kit/output/prerendered/pages/search.html',
          'src/routes/search/+page.svelte',
          'static/robots.txt',
          '.svelte-kit/output/prerendered/pages/messages.html'
        ]) {
          for (const kind of ['fifo', 'symlink', 'hardlink']) {
            await check(
              `unsafe compiler/static counterpart ${name}: ${kind}`,
              async (t) => {
                const f = await fixture(t);
                const file = path.join(f.web, name);
                await rm(file);
                if (kind === 'fifo') execFileSync('mkfifo', [file]);
                if (kind === 'symlink')
                  await symlink(path.join(base, 'web/src/app.html'), file);
                if (kind === 'hardlink')
                  await link(path.join(f.web, 'src/app.html'), file);
                await assert.rejects(f.audit);
              }
            );
          }
        }
        await check(
          'empty output directory and excessive depth are rejected',
          async (t) => {
            const f = await fixture(t);
            await mkdir(path.join(f.web, 'build/extra'));
            await assert.rejects(f.audit);
            await rm(path.join(f.web, 'build/extra'), { recursive: true });
            await mkdir(path.join(f.web, 'build/a/b/c/d/e/f/g/h/i'), {
              recursive: true
            });
            await assert.rejects(f.audit);
          }
        );
        await check(
          'forged public output failures do not echo private poisoned bytes',
          async (t) => {
            const f = await fixture(t);
            const marker = 'CONTROLLED_SENSITIVE_DO_NOT_ECHO';
            const file = path.join(f.web, 'static/build-info.json');
            await writeFile(file, marker);
            await assert.rejects(
              f.audit,
              (error) => !String(error).includes(marker)
            );
          }
        );
        await check(
          'output entry and aggregate-byte bounds reject oversized inventories',
          async (t) => {
            const f = await fixture(t);
            for (let i = 0; i < 513; i++)
              await writeFile(path.join(f.web, 'build', `tiny-${i}.js`), 'x');
            // Confine the actual audit's allocation measurement to a fresh child;
            // prior source/output fixture copies must not enter this baseline/peak.
            const measurement = JSON.parse(
              execFileSync(
                process.execPath,
                [
                  '--input-type=module',
                  '-e',
                  `
          const { auditOutput } = await import(process.argv[2]);
          const baseline = process.memoryUsage().arrayBuffers;
          let peak = baseline, rejection;
          const sample = () => { peak = Math.max(peak, process.memoryUsage().arrayBuffers); };
          const timer = setInterval(sample, 1);
          try { await auditOutput(process.argv[1]); }
          catch (error) { rejection = error instanceof Error ? error.message : 'unexpected'; }
          finally { sample(); clearInterval(timer); }
          if (rejection !== 'Unbounded output inventory') throw new Error('Unexpected inventory measurement rejection');
          process.stdout.write(JSON.stringify({ baseline, peak, growth: peak - baseline, rejection }));
        `,
                  f.web,
                  new URL('../../tools/check-output.mjs', import.meta.url).href
                ],
                {
                  cwd: f.web,
                  encoding: 'utf8',
                  timeout: 1800000,
                  maxBuffer: 4096
                }
              )
            );
            assert.deepEqual(Object.keys(measurement).sort(), [
              'baseline',
              'growth',
              'peak',
              'rejection'
            ]);
            assert.equal(measurement.rejection, 'Unbounded output inventory');
            for (const name of ['baseline', 'peak', 'growth'])
              assert.ok(
                Number.isSafeInteger(measurement[name]) &&
                  measurement[name] >= 0
              );
            assert.equal(
              measurement.growth,
              measurement.peak - measurement.baseline
            );
            t.diagnostic(
              'isolated actual inventory buffers: ' +
                JSON.stringify(measurement)
            );
            assert.ok(
              measurement.growth < 64 * 1024 * 1024,
              'Tiny files must not retain a maximum-sized buffer per entry: ' +
                JSON.stringify(measurement)
            );
            for (let i = 0; i < 513; i++)
              await rm(path.join(f.web, 'build', `tiny-${i}.js`));
            for (let i = 0; i < 33; i++)
              await writeFile(
                path.join(f.web, 'build', `large-${i}.js`),
                Buffer.alloc(1024 * 1024, 97)
              );
            await assert.rejects(f.audit, /Unbounded output bytes/);
          }
        );
      }
    );
    await test(
      'actual compiler admits nonempty approved theme/app CSS',
      { timeout: 1800000 },
      async (t) => {
        ready(t);
        const layout = path.join(base, 'web/src/routes/+layout.svelte');
        await writeFile(
          layout,
          (await readFile(layout, 'utf8')).replace(
            '<script lang="ts">',
            "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';"
          )
        );
        await writeFile(
          path.join(base, 'web/src/theme.css'),
          ':root { --hc-accent: #336633; }\n'
        );
        await writeFile(
          path.join(base, 'web/src/app.css'),
          'main { color: var(--hc-accent); }\n'
        );
        await generateBuildInfo(path.join(base, 'web'));
        compile(t, path.join(base, 'web'));
        const files = await auditOutput(await realpath(path.join(base, 'web')));
        assert.ok(files.some((name) => name.endsWith('.css')));
      }
    );
  } finally {
    // Await the complete serial sequence before reclaiming its shared build.
    await rm(base, { recursive: true, force: true });
  }
}
