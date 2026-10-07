import {
  approvedSigningIdentity,
  bindApprovedResponse,
  type ApprovedPublicSigning
} from '../nostr/approved-signing.ts';
import {
  capturedArtifactSnapshot,
  type CapturedArtifact
} from '../persistence/artifact-records.ts';
import {
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';

declare const lateBrand: unique symbol;
export type LatePublicArtifact = Readonly<{ [lateBrand]: true }>;
type Saved = Readonly<{
  owner: string;
  id: string;
  revision: number;
  recordWire: string;
  session: symbol;
  hash: string;
  artifact: CapturedArtifact;
}>;
const results = new WeakMap<LatePublicArtifact, Saved>();
// Pure exact public binding, not proof of SDK admission or current authority.
// The real adapter retains raw responses inside an admitted signature callback.
// Its same-job verified artifact may be reclassified if the identity continuation
// loses freshness. Provider objects and mutable verification caches never survive.
export function bindLatePublicResponse(
  approval: ApprovedPublicSigning,
  response: unknown,
  session: unknown
): LatePublicArtifact | undefined {
  const identity = approvedSigningIdentity(approval);
  if (!identity || typeof session !== 'symbol') return undefined;
  const artifact = bindApprovedResponse(approval, response),
    view = artifact && capturedArtifactSnapshot(artifact);
  if (!artifact || !view) return undefined;
  const token = Object.freeze({}) as LatePublicArtifact;
  results.set(token, {
    owner: identity.owner,
    id: identity.id,
    revision: identity.revision,
    recordWire: identity.recordWire,
    session,
    hash: view.hash,
    artifact
  });
  return token;
}
// Internal original-authority metadata. This does not revive a session or lease.
export function latePublicResultIdentity(token: LatePublicArtifact) {
  const saved = results.get(token);
  return saved
    ? {
        owner: saved.owner,
        id: saved.id,
        revision: saved.revision,
        recordWire: saved.recordWire,
        session: saved.session
      }
    : undefined;
}
export function latePublicResultSnapshot(token: LatePublicArtifact) {
  const saved = results.get(token);
  return saved
    ? {
        owner: saved.owner,
        id: saved.id,
        revision: saved.revision,
        hash: saved.hash
      }
    : undefined;
}
// Namespace-scoped mechanical review only. The session owner separately checks
// observed author; fresh resume/readiness/IDB CAS and publication remain distinct.
export function reviewLatePublicArtifact(
  token: LatePublicArtifact,
  original: PublicRecordHandle,
  owner: unknown,
  id: unknown,
  review: unknown
): CapturedArtifact | undefined {
  const saved = results.get(token);
  if (
    !saved ||
    review !== 'review_original_late_artifact' ||
    owner !== saved.owner ||
    id !== saved.id ||
    publicRecordWire(original, owner, id) !== saved.recordWire
  )
    return undefined;
  return saved.artifact;
}
