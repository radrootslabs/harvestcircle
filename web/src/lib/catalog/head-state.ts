import {
  projectFoodParts,
  type FoodProjection
} from '../contracts/food-availability-v1/read.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  createPublicHeadCandidate,
  publicHeadEnvelope,
  publicHeadSnapshot,
  selectPublicHead,
  type PublicHead,
  type PublicHeadDecision,
  type PublicHeadSnapshot
} from './heads.ts';
import {
  assessFutureTimestamp,
  wallClockAvailable,
  type WallClock
} from './clock-policy.ts';

declare const stateBrand: unique symbol;
export type FoodHeadState = Readonly<{ readonly [stateBrand]: true }>;
export type ClockDirection =
  'initial' | 'forward' | 'backward' | 'unchanged' | 'unavailable';
export type FoodHeadDisplay =
  'focused_known' | 'unsupported' | 'future_quarantined' | 'clock_unavailable';
export type FoodHeadStateSnapshot = Readonly<{
  head: PublicHeadSnapshot;
  classification: 'focused' | 'unsupported';
  display: FoodHeadDisplay;
  assessedAtSeconds: number | null;
  clockDirection: ClockDirection;
  unsupportedReason: string | undefined;
  food: FoodProjection | undefined;
}>;
interface Owner {
  readonly head: PublicHead;
  readonly nowSeconds: () => number;
  readonly lastValidClock: number | undefined;
  readonly view: FoodHeadStateSnapshot;
}
const owners = new WeakMap<FoodHeadState, Owner>();
function ownerOf(state: FoodHeadState): Owner {
  const owner = owners.get(state);
  if (!owner) throw new Error('food_head_state_invalid');
  return owner;
}
function assess(
  selectedHead: PublicHead,
  nowSeconds: () => number,
  lastValidClock: number | undefined
): Owner {
  const head = publicHeadSnapshot(selectedHead);
  const event = verifiedEnvelopeSnapshot(publicHeadEnvelope(selectedHead));
  if (!event) throw new Error('food_head_proof_invalid');
  const projected = projectFoodParts(event);
  const focused = projected.outcome === 'focused';
  let now: number | undefined;
  try {
    const sample = nowSeconds();
    if (wallClockAvailable(sample)) now = sample;
  } catch {
    /* Unavailable clock denies display, never discards new evidence. */
  }
  const clockDirection: ClockDirection =
    now === undefined
      ? 'unavailable'
      : lastValidClock === undefined
        ? 'initial'
        : now > lastValidClock
          ? 'forward'
          : now < lastValidClock
            ? 'backward'
            : 'unchanged';
  const timing =
    now === undefined
      ? 'clock_unavailable'
      : assessFutureTimestamp(head.created_at, now);
  const display: FoodHeadDisplay =
    timing !== 'within_policy'
      ? timing
      : focused
        ? 'focused_known'
        : 'unsupported';
  const view: FoodHeadStateSnapshot = {
    head,
    classification: focused ? 'focused' : 'unsupported',
    display,
    assessedAtSeconds: now ?? null,
    clockDirection,
    unsupportedReason:
      projected.outcome === 'rejected'
        ? projected.error.code
        : projected.outcome === 'excluded'
          ? projected.partition
          : undefined,
    food:
      display === 'focused_known' && projected.outcome === 'focused'
        ? projected.projection
        : undefined
  };
  return {
    head: selectedHead,
    nowSeconds,
    lastValidClock: now ?? lastValidClock,
    view
  };
}
export function createFoodHeadState(
  head: PublicHead,
  clock: WallClock
): FoodHeadState {
  if (publicHeadSnapshot(head).kind !== 30402)
    throw new Error('food_head_kind_invalid');
  const readClock = clock.nowSeconds;
  if (typeof readClock !== 'function')
    throw new Error('food_head_clock_invalid');
  const token = Object.freeze({}) as FoodHeadState;
  owners.set(
    token,
    assess(head, () => readClock(), undefined)
  );
  return token;
}
// No profile admission can run before HCP033 selects the generic winner.
export function advanceFoodHeadState(
  state: FoodHeadState,
  proof: VerifiedEnvelope
): PublicHeadDecision | 'ignored' {
  const owner = ownerOf(state),
    candidate = createPublicHeadCandidate(proof);
  if (!candidate) return 'ignored';
  const selected = selectPublicHead(owner.head, candidate);
  if (selected.decision === 'applied') {
    // Reserve the protocol winner before invoking caller-controlled clock code.
    // Until assessment settles, expose its evidence with no display authority.
    const pending: Owner = {
      ...assess(selected.head, () => NaN, owner.lastValidClock),
      nowSeconds: owner.nowSeconds
    };
    owners.set(state, pending);
    publishAssessment(state, pending);
  }
  return selected.decision;
}
// Called by a bounded explicit refresh, never an automatic wall-clock timer.
export function refreshFoodHeadState(state: FoodHeadState): void {
  publishAssessment(state, ownerOf(state));
}
function publishAssessment(state: FoodHeadState, owner: Owner): void {
  const measured = assess(owner.head, owner.nowSeconds, owner.lastValidClock);
  // A nested advance or refresh owns its replacement; never publish stale data.
  if (owners.get(state) === owner) owners.set(state, measured);
}
export function foodHeadStateHead(state: FoodHeadState): PublicHead {
  return ownerOf(state).head;
}
export function foodHeadStateSnapshot(
  state: FoodHeadState
): FoodHeadStateSnapshot {
  const view = ownerOf(state).view;
  return {
    ...view,
    head: { ...view.head },
    food:
      view.food === undefined
        ? undefined
        : (JSON.parse(JSON.stringify(view.food)) as FoodProjection)
  };
}
