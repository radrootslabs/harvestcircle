import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ListingRow from '../../../src/lib/components/ListingRow.svelte';
import SourceStatus from '../../../src/lib/components/SourceStatus.svelte';
import type { ListingView } from '../../../src/lib/catalog/listing-view.ts';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import { finalizeEvent } from 'applesauce-core/helpers';
function listing(): ListingView {
  const key = crypto.getRandomValues(new Uint8Array(32));
  let pubkey: string;
  try {
    pubkey = finalizeEvent(
      { kind: 0, created_at: 1, tags: [], content: '{}' },
      key
    ).pubkey;
  } finally {
    key.fill(0);
  }
  return {
    eventId: 'a'.repeat(64),
    createdAt: 1700000000,
    identifier: 'listing',
    href:
      '/products/' +
      encodeProductReference({ kind: 30402, pubkey, identifier: 'listing' }),
    title: '<script>Carrots</script>',
    summary: 'Seller summary',
    content: 'Text',
    location: 'Public area',
    price: {
      amount: '9007199254740993.123456789' as ListingView['price']['amount'],
      currency: 'CAD' as ListingView['price']['currency'],
      unit: 'kg'
    },
    quantity: null,
    publisher: {
      pubkey,
      label: '<img src=https://tracking.example>',
      assertedName: true
    },
    lastKnown: true,
    coverage: 'partial'
  };
}
describe('truthful search rows and persistent source notices', () => {
  it('renders advertised decimal/unit/area/unknown quantity and asserted publisher as text', () => {
    const value = listing(),
      html = render(ListingRow, { props: { listing: value } }).body;
    expect(html).toContain('9007199254740993.123456789');
    expect(html).toContain('CAD');
    expect(html).toContain('kg');
    expect(html).toContain('Public area');
    expect(html).toContain('Advertised quantity not specified');
    expect(html).toContain(value.publisher.pubkey);
    expect(html).toContain('&lt;script>');
    expect(html).toContain('&lt;img');
    expect(html).not.toMatch(/<script|<img|onclick|certified/);
    expect(html).toContain('href="' + value.href + '"');
    expect(html).toContain('<details');
  });
  it('renders supplied quantity without calculating a comparison price or inventing stock', () => {
    const value = {
      ...listing(),
      quantity: {
        amount: '20' as NonNullable<ListingView['quantity']>['amount'],
        unit: 'kg' as const
      }
    };
    const html = render(ListingRow, { props: { listing: value } }).body;
    expect(html).toMatch(/20\s+kg/);
    expect(html).not.toContain('not specified');
  });
  it('keeps failed-source uncertainty visible outside its details and separates unavailable from bounded empty', () => {
    const html = render(SourceStatus, {
      props: { refresh: 'partial', gap: false, scopes: [] }
    }).body;
    expect(html).toContain('These results may be incomplete.');
    expect(html.indexOf('may be incomplete')).toBeLessThan(
      html.indexOf('<details')
    );
    const unavailable = render(SourceStatus, {
      props: { refresh: 'unavailable', gap: false, scopes: [] }
    }).body;
    expect(unavailable).toContain('Search sources are unavailable.');
    expect(unavailable).not.toContain('No matches');
    const gap = render(SourceStatus, {
      props: { refresh: 'partial', gap: true, scopes: [] }
    }).body;
    expect(gap).toContain('same timestamp');
    expect(gap).not.toContain('complete search');
  });
});

it('does not invent a source failure for cancelled, deadline or capped work', () => {
  for (const refresh of ['cancelled', 'deadline', 'limit', 'error'] as const) {
    const html = render(SourceStatus, {
      props: { refresh, gap: false, scopes: [] }
    }).body;
    expect(html).toContain('These results may be incomplete.');
    expect(html).not.toContain('One source did not respond.');
  }
});

it('renders an admitted numeric timestamp beyond Date range without crashing or changing it', () => {
  const value = { ...listing(), createdAt: Number.MAX_SAFE_INTEGER };
  const html = render(ListingRow, { props: { listing: value } }).body;
  expect(html).toContain(String(value.createdAt));
});

const mounted = vi.hoisted<{
  callbacks: (() => void | (() => void))[];
  context: unknown;
}>(() => ({ callbacks: [], context: undefined }));
vi.mock('svelte', async (original) => ({
  ...(await original<typeof import('svelte')>()),
  getContext: () => mounted.context,
  onMount: (callback: () => void | (() => void)) => {
    mounted.callbacks.push(callback);
  }
}));
vi.mock('$app/state', () => ({
  page: { url: new URL('https://harvestcircle.example/search?q=carrots') }
}));
vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
describe('actual search route mounted initialization', () => {
  let runtime: typeof import('../../../src/lib/runtime/public-runtime.ts');
  let context: ReturnType<typeof runtime.createPublicRuntimeContext>;
  let cleanup: void | (() => void);
  const unhandled: unknown[] = [];
  const record = (error: unknown) => {
    unhandled.push(error);
  };
  beforeEach(async () => {
    vi.resetModules();
    vi.stubGlobal('window', {});
    mounted.callbacks.length = 0;
    unhandled.length = 0;
    cleanup = undefined;
    runtime = await import('../../../src/lib/runtime/public-runtime.ts');
    context = runtime.createPublicRuntimeContext();
    mounted.context = context;
    process.on('unhandledRejection', record);
  });
  afterEach(() => {
    if (typeof cleanup === 'function') cleanup();
    runtime.closePublicRuntime(context);
    process.off('unhandledRejection', record);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  async function mountCallback() {
    const { render } = await import('svelte/server');
    const { default: SearchPage } =
      await import('../../../src/routes/search/+page.svelte');
    const html = render(SearchPage, {
      context: new Map([[runtime.PUBLIC_RUNTIME_CONTEXT, context]])
    }).body;
    expect(html).toContain('Search sources are unavailable.');
    expect(mounted.callbacks.length).toBeGreaterThan(0);
    cleanup = mounted.callbacks[0]();
  }
  const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
  it('handles a genuine unavailable public projection without requests, timers or an unhandled rejection', async () => {
    runtime.mountPublicRuntime(context);
    const { getPublicStore, closePublicStore } =
      await import('../../../src/lib/nostr/public-store.ts');
    const store = getPublicStore();
    expect(store).toBeDefined();
    closePublicStore(store!);
    const { RelayPool } = await import('applesauce-relay/pool');
    const request = vi.spyOn(RelayPool.prototype, 'req');
    const timer = vi.spyOn(globalThis, 'setTimeout');
    const create = vi.spyOn(runtime, 'createPublicView');
    await mountCallback();
    await settle();
    expect(create).toHaveBeenCalledTimes(1);
    expect(create.mock.results[0].value).toMatchObject({
      message: 'public_projection_unavailable'
    });
    expect(request).not.toHaveBeenCalled();
    expect(timer).not.toHaveBeenCalled();
    expect(unhandled).toEqual([]);
  });
  it('disposes exactly the partially created view when search-owner construction fails', async () => {
    const shared = runtime.mountPublicRuntime(context)!;
    const other = runtime.createPublicView(shared);
    const views = await import('../../../src/lib/catalog/search-view.ts');
    vi.spyOn(views, 'createSearchView').mockImplementation(() => {
      throw Error('fixture_search_owner_failed');
    });
    const create = vi.spyOn(runtime, 'createPublicView');
    const dispose = vi.spyOn(runtime, 'disposePublicView');
    await mountCallback();
    await settle();
    expect(create).toHaveBeenCalledTimes(1);
    expect(dispose).toHaveBeenCalledExactlyOnceWith(
      create.mock.results[0].value
    );
    expect(runtime.publicViewProjectionAvailable(other)).toBe(true);
    expect(unhandled).toEqual([]);
    runtime.disposePublicView(other);
  });
  it('handles rejected readiness without acquiring a view', async () => {
    vi.spyOn(runtime, 'publicRuntimeReady').mockRejectedValue(
      Error('fixture_ready_failed')
    );
    const create = vi.spyOn(runtime, 'createPublicView');
    await mountCallback();
    await settle();
    expect(create).not.toHaveBeenCalled();
    expect(unhandled).toEqual([]);
  });
  it('keeps late genuine readiness after page disposal inert', async () => {
    const create = vi.spyOn(runtime, 'createPublicView');
    await mountCallback();
    expect(typeof cleanup).toBe('function');
    if (typeof cleanup === 'function') cleanup();
    cleanup = undefined;
    runtime.mountPublicRuntime(context);
    await settle();
    expect(create).not.toHaveBeenCalled();
    expect(unhandled).toEqual([]);
  });
});
