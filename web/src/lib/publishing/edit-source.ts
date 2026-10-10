import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';
import {
  projectFoodParts,
  type FoodProjection
} from '../contracts/food-availability-v1/read.ts';
import type { PublicDraftForm } from '../contracts/local-records.ts';
import { decodeProductReference } from '../nostr/references.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  publicPublicationOwner,
  publicPublicationDraftRepository,
  publicPublicationQuotaRepository,
  readPublicListingIntent,
  type PublicPublicationRepository
} from '../persistence/public-publications.ts';
import {
  inspectPublicStorage,
  publicInventorySnapshot
} from '../persistence/quota.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import { readPublicDraft } from '../persistence/drafts.ts';
import {
  identityViewSession,
  identityViewMatchesOwner,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import { subscribeIdentityInvalidation } from '../runtime/identity-session.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  publicViewOwnsRun,
  publicViewRunCurrent,
  publicViewProjectionAvailable,
  type PublicRuntime,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  createPublicViewHeadResolver,
  resolveHeads,
  headResolutionSnapshot,
  closeHeadResolver,
  type HeadResolver
} from '../catalog/resolve-head.ts';
import { createPublicHeadCandidate, publicHeadKey } from '../catalog/heads.ts';

declare const sourceBrand: unique symbol;
export type OwnedEditSource = Readonly<{ [sourceBrand]: true }>;
export type OwnedEditSourceSnapshot = Readonly<{
  owner: string;
  coordinate: string;
  sourceEventId: string;
  sourceCommand: string;
  publishedAtSeconds: number;
  form: PublicDraftForm;
  status:
    'read-only' | 'checking' | 'ready' | 'partial' | 'conflict' | 'withdrawn';
  current?: FoodProjection;
  currentEventId?: string;
  definitiveAbsence: false;
}>;
type Source = {
  snapshot(): OwnedEditSourceSnapshot | undefined;
  authorize():
    | Readonly<{
        repository: PublicPublicationRepository;
        owner: string;
        coordinate: string;
        baseWire: string;
        sourceCommand: string;
        publishedAtSeconds: number;
        current(): boolean;
      }>
    | undefined;
  close(): void;
};
const sources = new WeakMap<OwnedEditSource, Source>();
// Retained prototype custody precedes network work. A public/Tera event, a
// snapshot, or an arbitrary same-author signature is not adopted as local source.
export async function openOwnedEditSource(
  repository: PublicPublicationRepository,
  context: IdentityViewContext,
  value: unknown,
  runtime?: PublicRuntime,
  signal?: AbortSignal
): Promise<OwnedEditSource | undefined> {
  let view: PublicView | undefined, resolver: HeadResolver | undefined;
  let closed = false,
    off = () => {};
  function close() {
    if (closed) return;
    closed = true;
    off();
    signal?.removeEventListener('abort', close);
    try {
      if (resolver) closeHeadResolver(resolver);
    } finally {
      if (view) disposePublicView(view);
    }
  }
  try {
    const owner = publicPublicationOwner(repository),
      session = identityViewSession(context),
      reference = decodeProductReference(value),
      drafts = publicPublicationDraftRepository(repository),
      quota = publicPublicationQuotaRepository(repository);
    if (
      !owner ||
      !session ||
      !reference ||
      reference.pubkey !== owner ||
      !drafts ||
      !quota ||
      signal?.aborted ||
      !identityViewMatchesOwner(context, owner)
    )
      return;
    const current = () =>
      !closed &&
      !signal?.aborted &&
      identityViewSession(context) === session &&
      identityViewMatchesOwner(context, owner) &&
      publicPublicationOwner(repository) === owner;
    off = subscribeIdentityInvalidation(session, close);
    signal?.addEventListener('abort', close, { once: true });
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
    let retained:
      | Readonly<{
          form: PublicDraftForm;
          wire: string;
          command: string;
          publishedAt: number;
          eventId: string;
        }>
      | undefined;
    for (const item of inventory.rows) {
      if (item.family !== 'public_operation') continue;
      const read = await readPublicListingIntent(repository, item.id);
      if (!current()) {
        close();
        return;
      }
      const row =
        read.status === 'found' &&
        publicRecordSnapshot(read.record, owner, item.id);
      // Incomplete critical storage is not an absence that permits adoption.
      if (!row || row.family !== 'public_operation') {
        close();
        return;
      }
      if (
        row.capture.kind !== 30402 ||
        !row.artifact ||
        row.source.type !== 'draft'
      )
        continue;
      const template = inspectLocalTemplate(row.capture.wire, owner, 30402),
        projected = template && projectFoodParts(template);
      if (
        !template ||
        !projected ||
        projected.outcome !== 'focused' ||
        projected.projection.identifier !== reference.identifier
      )
        continue;
      const source = await readPublicDraft(drafts, row.source.id);
      if (!current()) {
        close();
        return;
      }
      if (!source.ok || source.value.revision !== row.source.revision) continue;
      const form = source.value.form,
        food = projected.projection;
      if (form.contactType === '' && form.contactValue !== '') continue;
      const built = buildFoodTemplate({
        identifier: food.identifier,
        title: form.title,
        description: form.description,
        location: form.location,
        amount: form.amount,
        currency: form.currency,
        unit: form.unit,
        ...(form.quantity === '' ? {} : { quantity: form.quantity }),
        published_at: food.published_at,
        created_at: template.created_at,
        status: food.status,
        ...(form.contactType === ''
          ? {}
          : {
              contact: {
                type: form.contactType,
                value: form.contactValue,
                public: true as const
              }
            })
      });
      if (
        !built.ok ||
        JSON.stringify({
          pubkey: owner,
          kind: 30402,
          created_at: template.created_at,
          tags: built.wire_parts.tags,
          content: built.wire_parts.content
        }) !== row.capture.wire
      )
        continue;
      const verified = verifyEnvelope(row.artifact.wire),
        signed = verified.ok && verifiedEnvelopeSnapshot(verified.value);
      if (
        !signed ||
        signed.id !== row.artifact.eventId ||
        signed.id !== row.capture.hash ||
        JSON.stringify({
          pubkey: signed.pubkey,
          kind: signed.kind,
          created_at: signed.created_at,
          tags: signed.tags,
          content: signed.content
        }) !== row.capture.wire
      )
        continue;
      if (retained) {
        close();
        return;
      }
      retained = {
        form,
        wire: row.artifact.wire,
        command: row.id,
        publishedAt: food.published_at,
        eventId: signed.id
      };
    }
    if (!retained || !current()) {
      close();
      return;
    }
    const base = retained,
      proof = verifyEnvelope(base.wire);
    const head = proof.ok && createPublicHeadCandidate(proof.value);
    if (!head) {
      close();
      return;
    }
    const key = publicHeadKey(head);
    let run: ReturnType<typeof beginPublicViewRun> | undefined;
    if (runtime) {
      // A separate view/run and exact coordinate+deletion requests are independent
      // of keyword search. Genuine retained proof seeds the ordinary selector.
      view = createPublicView(runtime);
      run = beginPublicViewRun(view);
      resolver = createPublicViewHeadResolver(view, run, {
        nowSeconds: () => Math.floor(Date.now() / 1000)
      });
      resolveHeads(resolver, [head]);
    }
    const networkOwned = () =>
      !!(
        view &&
        run &&
        publicViewOwnsRun(view, run) &&
        publicViewProjectionAvailable(view)
      );
    function snapshot(): OwnedEditSourceSnapshot | undefined {
      if (!current()) {
        close();
        return;
      }
      const row =
        resolver && networkOwned()
          ? headResolutionSnapshot(resolver).find((row) => row.key === key)
          : undefined;
      const scopes = row && [row.headSources, row.deletionSources];
      const complete = !!(
        row &&
        view &&
        run &&
        publicViewRunCurrent(view, run) &&
        row.coverage === 'bounded-eose' &&
        !row.lastKnown &&
        scopes?.every(
          (scope) =>
            scope?.state === 'eose' && scope.result.coverage === 'bounded-eose'
        )
      );
      const competing = row && row.state.head.id !== base.eventId;
      const status: OwnedEditSourceSnapshot['status'] = !runtime
        ? 'read-only'
        : row?.deletion.outcome === 'suppressed'
          ? 'withdrawn'
          : competing
            ? 'conflict'
            : complete &&
                row?.state.food &&
                row.state.display === 'focused_known'
              ? 'ready'
              : scopes?.some((scope) => scope?.state === 'active')
                ? 'checking'
                : 'partial';
      return {
        owner: owner!,
        coordinate: reference!.naddr,
        sourceEventId: base.eventId,
        sourceCommand: base.command,
        publishedAtSeconds: base.publishedAt,
        form: { ...base.form },
        status,
        ...(row?.state.food ? { current: row.state.food } : {}),
        ...(row ? { currentEventId: row.state.head.id } : {}),
        definitiveAbsence: false
      };
    }
    const token = Object.freeze({}) as OwnedEditSource;
    sources.set(token, {
      snapshot,
      close,
      authorize() {
        if (snapshot()?.status !== 'ready') return;
        return {
          repository,
          owner,
          coordinate: reference.naddr,
          baseWire: base.wire,
          sourceCommand: base.command,
          publishedAtSeconds: base.publishedAt,
          current: () => snapshot()?.status === 'ready'
        };
      }
    });
    return token;
  } catch {
    close();
    return;
  }
}
export function ownedEditSourceSnapshot(
  source: OwnedEditSource
): OwnedEditSourceSnapshot | undefined {
  return sources.get(source)?.snapshot();
}
// Internal original edit custody only, never a signing/delivery lease.
export function authorizeOwnedEditSource(source: OwnedEditSource) {
  return sources.get(source)?.authorize();
}
export function closeOwnedEditSource(source: OwnedEditSource): void {
  sources.get(source)?.close();
}
