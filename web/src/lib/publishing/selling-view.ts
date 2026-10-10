import {
  identityViewSession,
  identityViewMatchesOwner,
  subscribeIdentityView,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import { identitySessionSnapshot } from '../runtime/identity-session.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from '../persistence/database.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  publicPublicationQuotaRepository,
  readPublicListingIntent
} from '../persistence/public-publications.ts';
import { listPublicDrafts } from '../persistence/drafts.ts';
import {
  inspectPublicStorage,
  publicInventorySnapshot
} from '../persistence/quota.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import { encodeProductReference } from '../nostr/references.ts';
import { internalHref } from '../navigation-url.ts';
import {
  projectFoodParts,
  type FoodProjection
} from '../contracts/food-availability-v1/read.ts';
import {
  readListingOperationView,
  listingOperationSnapshot,
  closeListingOperationView,
  type ListingOperationView
} from './listing-operation-view.ts';
import {
  openOwnedEditSource,
  ownedEditSourceSnapshot,
  closeOwnedEditSource
} from './edit-source.ts';
import {
  createPublicView,
  beginPublicViewRun,
  subscribePublicView,
  disposePublicView,
  publicViewOwnsRun,
  publicViewProjectionAvailable,
  publicViewRunCurrent,
  type PublicRuntime,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  createPublicHeadCandidate,
  publicHeadKey,
  publicHeadSnapshot,
  selectPublicHead,
  type PublicHead
} from '../catalog/heads.ts';
import {
  createPublicViewHeadResolver,
  resolveHeads,
  headResolutionSnapshot,
  closeHeadResolver,
  type HeadResolver
} from '../catalog/resolve-head.ts';
import {
  publicRequestScopeSnapshot,
  publicRunSnapshot,
  type PublicRequest,
  type PublicRun
} from '../nostr/request-scope.ts';
import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';

declare const viewBrand: unique symbol;
export type SellingView = Readonly<{ [viewBrand]: true }>;
export type SellingRow = Readonly<{
  key: string;
  title: string;
  status: string;
  provenance: string;
  href?: string;
  editHref?: string;
  draftHref?: string;
  operations: readonly ListingOperationView[];
}>;
export type SellingSnapshot = Readonly<{
  attention: readonly SellingRow[];
  drafts: readonly SellingRow[];
  listings: readonly SellingRow[];
  remote: 'checking' | 'bounded' | 'partial' | 'unavailable';
  local: 'ready' | 'unavailable';
}>;
type Controller = {
  snapshot(): SellingSnapshot | undefined;
  subscribe(listener: () => void): () => void;
  close(): void;
};
type Group = {
  key: string;
  food: FoodProjection;
  createdAt: number;
  eventId: string;
  removal: boolean;
  operations: ListingOperationView[];
  attention: boolean;
  supportedEventId?: string;
  draftId?: string;
};
const views = new WeakMap<SellingView, Controller>();
const active = new WeakMap<IdentityViewContext, Controller>();

// Account-local presentation only. Neither a row, link nor operation disclosure
// grants signing, delivery, adopted-source or current-head permission.
export async function openSellingView(
  context: IdentityViewContext,
  runtime?: PublicRuntime
): Promise<SellingView | undefined> {
  const identity = identityViewSession(context);
  const observed = identity && identitySessionSnapshot(identity);
  const owner =
    observed && 'publicKey' in observed ? observed.publicKey : undefined;
  if (!identity || !owner || !identityViewMatchesOwner(context, owner)) return;
  active.get(context)?.close();
  let alive = true,
    database: BrowserDatabase | undefined;
  let publicView: PublicView | undefined, run: PublicRun | undefined;
  let resolver: HeadResolver | undefined, discovery: PublicRequest | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let offIdentity = () => {};
  let local: SellingSnapshot['local'] = 'unavailable';
  let remote: SellingSnapshot['remote'] = runtime ? 'checking' : 'unavailable';
  const listeners = new Map<symbol, () => void>();
  const groups = new Map<string, Group>();
  const draftRows = new Map<string, SellingRow>();
  let operationViews = Array.from<ListingOperationView>([]);
  const heads = new Map<string, PublicHead>();
  const linkedDrafts = new Map<string, true>();
  const current = () =>
    alive &&
    active.get(context) === controller &&
    identityViewSession(context) === identity &&
    identityViewMatchesOwner(context, owner);
  function notify() {
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Presentation cannot interrupt owned cleanup. */
      }
    }
  }
  function close() {
    if (!alive) return;
    alive = false;
    offIdentity();
    if (timer !== undefined) clearTimeout(timer);
    for (const view of operationViews) closeListingOperationView(view);
    try {
      if (resolver) closeHeadResolver(resolver);
    } finally {
      try {
        if (publicView) disposePublicView(publicView);
      } finally {
        if (database) closeBrowserDatabase(database);
      }
    }
    if (active.get(context) === controller) active.delete(context);
    groups.clear();
    draftRows.clear();
    heads.clear();
    notify();
    listeners.clear();
  }
  const hrefFor = (identifier: string) => {
    const naddr = encodeProductReference({
      kind: 30402,
      pubkey: owner,
      identifier
    });
    return naddr ? internalHref(`/products/${naddr}`) : undefined;
  };
  function snapshot(): SellingSnapshot | undefined {
    if (!current()) return;
    let attention = Array.from<SellingRow>([]),
      listings = Array.from<SellingRow>([]);
    const rows =
      resolver &&
      publicView &&
      run &&
      publicViewOwnsRun(publicView, run) &&
      publicViewProjectionAvailable(publicView)
        ? headResolutionSnapshot(resolver)
        : [];
    const remoteByIdentifier = new Map(
      rows.map((row) => [row.state.head.identifier, row])
    );
    for (const group of groups.values()) {
      const known = remoteByIdentifier.get(group.food.identifier);
      const food = known?.state.food ?? group.food;
      const suppressed = known?.deletion.outcome === 'suppressed';
      const href = hrefFor(food.identifier);
      const canEdit = !!(
        href &&
        group.supportedEventId &&
        !group.removal &&
        !suppressed &&
        (!known ||
          (known.state.head.id === group.supportedEventId &&
            known.state.display === 'focused_known'))
      );
      const row: SellingRow = {
        key: group.key,
        title: food.title,
        status: group.removal
          ? 'Removal request retained locally'
          : suppressed
            ? 'Listing no longer shown; delivery records retained'
            : known?.state.food
              ? food.status === 'sold'
                ? 'Marked sold'
                : 'Marked available'
              : 'Saved public operation; current availability unconfirmed',
        provenance: 'Local publication evidence · Remote discovery is bounded',
        ...(href ? { href } : {}),
        ...(canEdit ? { editHref: internalHref(`${href}/edit`) } : {}),
        ...(group.attention &&
        group.draftId &&
        draftRows.get(group.draftId)?.draftHref
          ? { draftHref: draftRows.get(group.draftId)?.draftHref }
          : {}),
        operations: group.operations.slice()
      };
      if (group.attention) attention = attention.concat(row);
      else listings = listings.concat(row);
      remoteByIdentifier.delete(group.food.identifier);
    }
    for (const row of remoteByIdentifier.values()) {
      if (
        !row.state.food ||
        row.state.head.pubkey !== owner ||
        row.state.display !== 'focused_known' ||
        row.deletion.outcome === 'suppressed'
      )
        continue;
      const href = hrefFor(row.state.food.identifier);
      if (!href) continue;
      listings = listings.concat({
        key: row.key,
        title: row.state.food.title,
        status:
          row.state.food.status === 'sold' ? 'Marked sold' : 'Marked available',
        provenance:
          'Bounded remote discovery · Read-only; no supported retained prototype source',
        href,
        operations: []
      });
    }
    return {
      attention,
      drafts: Array.from(draftRows, ([id, row]) =>
        linkedDrafts.has(id) ? undefined : row
      ).filter((row): row is SellingRow => !!row),
      listings,
      local,
      remote
    };
  }
  const controller: Controller = {
    snapshot,
    subscribe(listener) {
      const stamp = Symbol();
      listeners.set(stamp, listener);
      return () => {
        listeners.delete(stamp);
      };
    },
    close
  };
  active.set(context, controller);
  offIdentity = subscribeIdentityView(context, () => {
    if (!current()) close();
  });
  try {
    const opened = await openBrowserDatabase();
    if (opened.state !== 'ready') {
      close();
      return;
    }
    if (!current()) {
      closeBrowserDatabase(opened.owner);
      close();
      return;
    }
    database = opened.owner;
    const repository = createPublicPublicationRepository(database, owner);
    const drafts = repository && publicPublicationDraftRepository(repository);
    const quota = repository && publicPublicationQuotaRepository(repository);
    if (!repository || !drafts || !quota) {
      close();
      return;
    }
    const saved = await listPublicDrafts(drafts);
    if (!current() || !saved.ok) {
      close();
      return;
    }
    for (const draft of saved.value) {
      const href = internalHref(`/selling/drafts/${draft.id}`);
      if (href)
        draftRows.set(draft.id, {
          key: `draft:${draft.id}`,
          title: draft.form.title || 'Untitled draft',
          status: 'Saved in this browser · Not published',
          provenance: 'Local draft',
          draftHref: href,
          operations: []
        });
    }
    const inspected = await inspectPublicStorage(quota);
    if (!current() || !inspected.ok) {
      close();
      return;
    }
    const inventory = publicInventorySnapshot(quota, inspected.value);
    if (!inventory) {
      close();
      return;
    }
    for (const item of inventory.rows) {
      if (item.family !== 'public_operation') continue;
      const read = await readPublicListingIntent(repository, item.id);
      if (!current() || read.status !== 'found') {
        close();
        return;
      }
      const record = publicRecordSnapshot(read.record, owner, item.id);
      if (!record || record.family !== 'public_operation') {
        close();
        return;
      }
      if (record.capture.kind !== 30402 && record.capture.kind !== 5) continue;
      const template = inspectLocalTemplate(
        record.capture.wire,
        owner,
        record.capture.kind
      );
      const base =
        record.capture.kind === 5 && record.source.type === 'public_head'
          ? verifyEnvelope(record.source.wire)
          : undefined;
      const source = base?.ok ? verifiedEnvelopeSnapshot(base.value) : template;
      const projected = source && projectFoodParts(source);
      if (!template || !projected || projected.outcome !== 'focused') continue;
      const key = JSON.stringify([
        30402,
        owner,
        projected.projection.identifier
      ]);
      const presented = await readListingOperationView(
        repository,
        identity,
        item.id
      );
      if (!current() || presented.status !== 'found') {
        if (presented.status === 'found')
          closeListingOperationView(presented.view);
        close();
        return;
      }
      operationViews = operationViews.concat(presented.view);
      const facts = listingOperationSnapshot(presented.view);
      if (!facts) {
        close();
        return;
      }
      let group = groups.get(key);
      if (!group) {
        group = {
          key,
          food: projected.projection,
          createdAt: template.created_at,
          eventId: record.capture.hash,
          removal: template.kind === 5,
          attention: false,
          operations: [],
          ...(record.source.type === 'draft'
            ? { draftId: record.source.id }
            : {})
        };
        groups.set(key, group);
      }
      if (
        template.created_at > group.createdAt ||
        (template.created_at === group.createdAt &&
          record.capture.hash < group.eventId)
      ) {
        group = {
          ...group,
          food: projected.projection,
          createdAt: template.created_at,
          eventId: record.capture.hash,
          removal: template.kind === 5,
          ...(record.source.type === 'draft'
            ? { draftId: record.source.id }
            : {})
        };
      }
      group = {
        ...group,
        operations: group.operations.concat(presented.view),
        attention:
          group.attention ||
          facts.needsAction ||
          !record.artifact ||
          facts.targets.some(
            (target) => !target.accepted || target.readback !== 'confirmed'
          )
      };
      groups.set(key, group);
      const capturedSource = record.source;
      if (
        capturedSource.type === 'draft' &&
        saved.value.some(
          (draft) =>
            draft.id === capturedSource.id &&
            draft.revision === capturedSource.revision
        )
      )
        linkedDrafts.set(capturedSource.id, true);
      if (record.artifact && record.capture.kind === 30402) {
        const proof = verifyEnvelope(record.artifact.wire);
        const head = proof.ok && createPublicHeadCandidate(proof.value);
        if (head) {
          const prior = heads.get(publicHeadKey(head));
          heads.set(publicHeadKey(head), selectPublicHead(prior, head).head);
        }
      }
    }
    // Reuse the original retained-source verifier without starting a network
    // run or adopting a remote source. Edit's route performs its own fresh
    // independent current-head checks; this metadata grants no effect custody.
    for (const group of groups.values()) {
      if (group.attention && group.draftId && draftRows.has(group.draftId))
        linkedDrafts.set(group.draftId, true);
      const naddr = encodeProductReference({
        kind: 30402,
        pubkey: owner,
        identifier: group.food.identifier
      });
      const source =
        naddr && (await openOwnedEditSource(repository, context, naddr));
      if (!current()) {
        if (source) closeOwnedEditSource(source);
        close();
        return;
      }
      const supported = source ? ownedEditSourceSnapshot(source) : undefined;
      if (supported)
        groups.set(group.key, {
          ...group,
          supportedEventId: supported.sourceEventId
        });
      if (source) closeOwnedEditSource(source);
    }
    local = 'ready';
    if (runtime) {
      try {
        publicView = createPublicView(runtime);
        run = beginPublicViewRun(publicView);
        resolver = createPublicViewHeadResolver(publicView, run, {
          nowSeconds: () => Math.floor(Date.now() / 1000)
        });
        discovery = subscribePublicView(
          publicView,
          run,
          'head',
          [
            {
              kinds: [30402],
              authors: [owner],
              limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
            }
          ],
          (proof) => {
            if (!current()) return;
            const head = createPublicHeadCandidate(proof);
            if (!head || publicHeadSnapshot(head).pubkey !== owner) return;
            const key = publicHeadKey(head);
            if (
              !heads.has(key) &&
              heads.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun
            )
              return;
            heads.set(key, selectPublicHead(heads.get(key), head).head);
          }
        );
        function observe() {
          if (!current()) return;
          if (
            !publicView ||
            !run ||
            !resolver ||
            !discovery ||
            !publicViewOwnsRun(publicView, run) ||
            !publicViewProjectionAvailable(publicView)
          ) {
            remote = 'unavailable';
            notify();
            return;
          }
          try {
            if (publicViewRunCurrent(publicView, run))
              resolveHeads(
                resolver,
                Array.from(heads.values()).slice(
                  0,
                  PUBLIC_QUERY_BUDGETS.coordinatesPerRun
                )
              );
            const rows = headResolutionSnapshot(resolver);
            const source = publicRequestScopeSnapshot(discovery);
            remote =
              source.state === 'eose' &&
              source.result.coverage === 'bounded-eose' &&
              rows.every((row) => row.coverage === 'bounded-eose')
                ? 'bounded'
                : publicRunSnapshot(run).activeRequests > 0
                  ? 'checking'
                  : 'partial';
          } catch {
            remote = 'partial';
          }
          notify();
          if (
            current() &&
            publicRunSnapshot(run).active &&
            publicRunSnapshot(run).activeRequests > 0
          )
            timer = setTimeout(observe, 250);
        }
        observe();
      } catch {
        remote = 'unavailable';
      }
    }
    if (!current()) {
      close();
      return;
    }
    const token = Object.freeze({}) as SellingView;
    views.set(token, controller);
    return token;
  } catch {
    close();
    return;
  }
}
export function sellingViewSnapshot(
  view: SellingView
): SellingSnapshot | undefined {
  return views.get(view)?.snapshot();
}
export function subscribeSellingView(
  view: SellingView,
  listener: () => void
): () => void {
  return views.get(view)?.subscribe(listener) ?? (() => {});
}
export function closeSellingView(view: SellingView): void {
  views.get(view)?.close();
}
