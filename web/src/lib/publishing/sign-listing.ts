import {
  listingReviewOwnership,
  type ListingReview
} from './prepare-listing.ts';
import {
  publicPublicationQuotaRepository,
  readPublicListingIntent
} from '../persistence/public-publications.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  preparePublicListingSigningTransition,
  preparePublicArtifactTransition,
  publicTransitionSnapshot,
  type CapturedArtifact
} from '../persistence/artifact-records.ts';
import {
  commitPublicOperationTransition,
  observePublicOperationTransition
} from '../persistence/quota.ts';
import {
  captureIdentityPublicOperation,
  identityPublicOperationOwnership,
  identityPublicOperationSnapshot,
  recheckIdentityOwner,
  signIdentityPublicOperation,
  stopIdentityPublicOperation,
  type IdentityPublicOperation
} from '../runtime/identity-session.ts';
import { runCapturedPublicEffect } from '../runtime/effect-ownership.ts';

declare const signingBrand: unique symbol;
declare const publicationBrand: unique symbol;
export type ListingSigning = Readonly<{ [signingBrand]: true }>;
export type ListingPublication = Readonly<{ [publicationBrand]: true }>;
type Ownership = NonNullable<ReturnType<typeof listingReviewOwnership>>;
type Model = Readonly<{
  original: Ownership;
  getOperation(): IdentityPublicOperation | undefined;
  setOperation(value: IdentityPublicOperation): void;
  getAttempted(): boolean;
  setAttempted(): void;
  getStopped(): boolean;
  stop(): void;
  getSigning(): boolean;
  startSigning(): void;
  finishSigning(): void;
  getMarked(): PublicRecordHandle | undefined;
  setMarked(value: PublicRecordHandle): void;
  getArtifact(): CapturedArtifact | undefined;
  setArtifact(value: CapturedArtifact): void;
  getPublication(): ListingPublication | undefined;
  setPublication(value: ListingPublication): void;
}>;
function createModel(original: Ownership): Model {
  let operation: IdentityPublicOperation | undefined;
  let signing = false;
  let attempted = false,
    stopped = false;
  let marked: PublicRecordHandle | undefined;
  let artifact: CapturedArtifact | undefined;
  let publication: ListingPublication | undefined;
  return {
    original,
    getOperation: () => operation,
    setOperation: (value) => {
      operation = value;
    },
    getAttempted: () => attempted,
    setAttempted: () => {
      attempted = true;
    },
    getStopped: () => stopped,
    stop: () => {
      stopped = true;
    },
    getSigning: () => signing,
    startSigning: () => {
      signing = true;
    },
    finishSigning: () => {
      signing = false;
    },
    getMarked: () => marked,
    setMarked: (value) => {
      marked = value;
    },
    getArtifact: () => artifact,
    setArtifact: (value) => {
      artifact = value;
    },
    getPublication: () => publication,
    setPublication: (value) => {
      publication = value;
    }
  };
}
type Publication = Readonly<{
  model: Model;
  recordWire: string;
  wire: string;
  owner: string;
  command: string;
  hash: string;
  targets: readonly string[];
  policyFingerprint: string;
}>;
const signings = new WeakMap<ListingSigning, Model>();
const commands = new WeakMap<ListingReview, ListingSigning>();
const pending = new WeakMap<ListingSigning, Promise<ListingSigningResult>>();
const publications = new WeakMap<ListingPublication, Publication>();
export type ListingSigningResult =
  | Readonly<{ status: 'prepared'; publication: ListingPublication }>
  | Readonly<{
      status:
        'invalid' | 'unavailable' | 'conflict' | 'unknown' | 'busy' | 'stopped';
    }>;

function current(model: Model): boolean {
  const operation = model.getOperation();
  return (
    !model.getStopped() &&
    (model.getSigning()
      ? model.original.readiness.signingCurrent() &&
        !!publicPublicationQuotaRepository(model.original.repository)
      : model.original.current()) &&
    (!operation ||
      !!identityPublicOperationOwnership(
        model.original.readiness.identity,
        operation
      ))
  );
}
export function beginListingSigning(
  review: ListingReview,
  consent: unknown
): ListingSigning | undefined {
  if (consent !== 'sign_reviewed_listing') return;
  const original = listingReviewOwnership(review);
  if (!original || !publicPublicationQuotaRepository(original.repository))
    return;
  const existing = commands.get(review);
  if (existing) return existing;
  const token = Object.freeze({}) as ListingSigning;
  signings.set(token, createModel(original));
  commands.set(review, token);
  return token;
}
export function listingSigningSnapshot(token: ListingSigning) {
  const model = signings.get(token);
  if (!model) return;
  const original = model.original;
  const row = publicRecordSnapshot(
    original.record,
    original.readiness.owner,
    original.command
  );
  if (!row || row.family !== 'public_operation') return;
  const heldOperation = model.getOperation();
  const operation =
    heldOperation &&
    identityPublicOperationSnapshot(original.readiness.identity, heldOperation);
  return {
    owner: row.owner,
    command: row.id,
    hash: row.capture.hash,
    stopped: model.getStopped(),
    attempted: model.getAttempted(),
    lateAvailable: operation?.lateAvailable ?? false
  };
}
// Detached disclosure is never a send capability. HCP112 must independently
// admit the original plan, reread storage and obtain final network ownership.
export function listingPublicationSnapshot(token: ListingPublication) {
  const value = publications.get(token);
  if (!value || !current(value.model)) return;
  return {
    owner: value.owner,
    command: value.command,
    hash: value.hash,
    wire: value.wire,
    targets: value.targets.slice(),
    policyFingerprint: value.policyFingerprint
  };
}
async function exactStored(
  model: Model,
  expected: PublicRecordHandle
): Promise<boolean> {
  const original = model.original,
    owner = original.readiness.owner;
  const read = await readPublicListingIntent(
    original.repository,
    original.command
  );
  return (
    current(model) &&
    read.status === 'found' &&
    publicRecordWire(read.record, owner, original.command) ===
      publicRecordWire(expected, owner, original.command)
  );
}
async function persist(model: Model): Promise<ListingSigningResult> {
  const original = model.original,
    owner = original.readiness.owner;
  const quota = publicPublicationQuotaRepository(original.repository);
  const marked = model.getMarked(),
    artifact = model.getArtifact();
  if (!quota || !marked || !artifact || !current(model))
    return { status: 'unavailable' };
  const transition = preparePublicArtifactTransition(
    marked,
    owner,
    original.command,
    artifact
  );
  if (!transition.ok) return { status: 'conflict' };
  const proposed = publicTransitionSnapshot(
    transition.value,
    owner,
    original.command
  );
  if (!proposed) return { status: 'invalid' };
  const decoded = decodePublicRecord(
    proposed.nextWire,
    owner,
    original.command
  );
  if (!decoded.ok) return { status: 'invalid' };
  if (!current(model)) return { status: 'stopped' };
  const saved = await commitPublicOperationTransition(quota, transition.value);
  if (!current(model)) return { status: 'unknown' };
  if (!saved.ok) {
    if (saved.reason !== 'unknown_completion')
      return {
        status: saved.reason === 'conflict' ? 'conflict' : 'unavailable'
      };
    const observed = await observePublicOperationTransition(
      quota,
      transition.value
    );
    if (observed.state !== 'committed' || !current(model))
      return { status: observed.state === 'conflict' ? 'conflict' : 'unknown' };
  }
  if (!(await exactStored(model, decoded.value))) return { status: 'unknown' };
  const row = publicRecordSnapshot(decoded.value, owner, original.command);
  if (!row || row.family !== 'public_operation' || !row.artifact)
    return { status: 'invalid' };
  const token = Object.freeze({}) as ListingPublication;
  publications.set(token, {
    model,
    recordWire: proposed.nextWire,
    wire: row.artifact.wire,
    owner,
    command: row.id,
    hash: row.artifact.eventId,
    targets: row.capture.targets.slice(),
    policyFingerprint: row.capture.policyFingerprint
  });
  model.setPublication(token);
  return { status: 'prepared', publication: token };
}
async function execute(model: Model): Promise<ListingSigningResult> {
  try {
    const original = model.original,
      identity = original.readiness.identity,
      owner = original.readiness.owner;
    const quota = publicPublicationQuotaRepository(original.repository);
    if (
      !quota ||
      !current(model) ||
      typeof window === 'undefined' ||
      !navigator.locks?.request
    )
      return { status: 'unavailable' };
    const publication = model.getPublication();
    if (publication) {
      const value = publications.get(publication);
      if (!value) return { status: 'invalid' };
      const rechecked = await recheckIdentityOwner(identity);
      if (!current(model) || rechecked.state === 'guest')
        return { status: 'unavailable' };
      const read = await readPublicListingIntent(
        original.repository,
        original.command
      );
      if (
        !current(model) ||
        read.status !== 'found' ||
        publicRecordWire(read.record, owner, original.command) !==
          value.recordWire
      )
        return { status: 'conflict' };
      return { status: 'prepared', publication };
    }
    if (model.getArtifact()) {
      await recheckIdentityOwner(identity);
      if (!current(model)) return { status: 'unavailable' };
      // Retained exact verified artifact retry is storage-only; never a prompt.
      return await navigator.locks.request(
        'harvestcircle:owner:' + owner,
        { mode: 'exclusive', ifAvailable: true },
        (lock) =>
          lock ? persist(model) : Promise.resolve({ status: 'busy' as const })
      );
    }
    if (model.getAttempted()) return { status: 'unknown' };
    if (!(await exactStored(model, original.record)))
      return { status: 'conflict' };
    const operation = captureIdentityPublicOperation(
      identity,
      original.record,
      original.command,
      'reviewed_captured_operation'
    );
    if (!operation) return { status: 'unavailable' };
    model.setOperation(operation);
    const ownership = identityPublicOperationOwnership(identity, operation);
    if (!ownership || !current(model)) return { status: 'unavailable' };
    const result = await runCapturedPublicEffect(
      quota,
      original.record,
      original.command,
      { ...ownership, current: () => current(model) && ownership.current() },
      'reviewed_prepared_listing_operation',
      async (lease): Promise<ListingSigningResult> => {
        const marker = preparePublicListingSigningTransition(
          original.record,
          owner,
          original.command
        );
        if (!marker.ok || !current(model)) return { status: 'invalid' };
        const next = publicTransitionSnapshot(
          marker.value,
          owner,
          original.command
        );
        if (!next) return { status: 'invalid' };
        model.setAttempted();
        const saved = await commitPublicOperationTransition(
          quota,
          marker.value
        );
        if (!saved.ok || !current(model))
          return {
            status:
              saved.ok || saved.reason === 'unknown_completion'
                ? 'unknown'
                : 'unavailable'
          };
        const decoded = decodePublicRecord(
          next.nextWire,
          owner,
          original.command
        );
        if (!decoded.ok || !(await exactStored(model, decoded.value)))
          return { status: 'unknown' };
        model.setMarked(decoded.value);
        model.startSigning();
        let signed;
        try {
          signed = await signIdentityPublicOperation(
            identity,
            operation,
            lease
          );
        } finally {
          model.finishSigning();
        }
        if (signed.status !== 'signed' || !current(model))
          return {
            status:
              signed.status === 'unknown' || !current(model)
                ? 'unknown'
                : 'invalid'
          };
        model.setArtifact(signed.artifact);
        return await persist(model);
      }
    );
    return result.status === 'completed'
      ? result.value
      : {
          status:
            result.status === 'busy'
              ? 'busy'
              : result.status === 'unknown' || result.status === 'retained'
                ? 'unknown'
                : 'unavailable'
        };
  } catch {
    return { status: 'unknown' };
  }
}
export function runListingSigning(
  token: ListingSigning,
  consent: unknown
): Promise<ListingSigningResult> {
  const model = signings.get(token);
  if (!model || consent !== 'sign_reviewed_listing')
    return Promise.resolve({ status: 'invalid' });
  const active = pending.get(token);
  if (active) return active;
  const work = execute(model).finally(() => {
    if (pending.get(token) === work) pending.delete(token);
  });
  pending.set(token, work);
  return work;
}
export function stopListingSigning(token: ListingSigning): void {
  const model = signings.get(token);
  if (!model) return;
  model.stop();
  const operation = model.getOperation();
  if (operation)
    stopIdentityPublicOperation(model.original.readiness.identity, operation);
}
