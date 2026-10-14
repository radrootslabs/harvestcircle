import {
  inboxProbePublicationBinding,
  recoveredInboxProbePublicationBinding,
  verifyInboxProbeAcknowledgement,
  type InboxProbeAcknowledgement
} from '../persistence/inbox-probes.ts';
import {
  inboxProbeActionOwner,
  type InboxProbeIdentity
} from './inbox-access.ts';
import type { RecoveredInboxProbe } from './self-recovery-reader.ts';
import {
  inboxAccessControllerSnapshot,
  type InboxAccessController
} from '../messaging/inbox-access-controller.ts';
import { privatePoolProbeRetryIdle } from './private-pool.ts';
import {
  privateSessionOwnership,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  pairedDeliveryTargetAccepted,
  verifyPairedDeliveryAcknowledgement,
  type PairedDeliveryAcknowledgement
} from '../persistence/private-sends.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
import type { PairedDeliveryContext } from '../messaging/prepare-send.ts';
import {
  inboxRoutePlanSnapshot,
  recheckInboxRoutePlan
} from '../messaging/inbox-routing.ts';
import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';
import {
  PRIVATE_TRANSPORT_BUDGETS,
  PRIVATE_PUBLICATION_BUDGETS,
  LOCAL_PERSISTENCE_BUDGETS
} from '../config/budgets.ts';
import { newLocalId } from '../private-handles.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';
declare const publicationBrand: unique symbol;
export type PrivatePublication = Readonly<{ [publicationBrand]: true }>;
export type PrivateDeliveryRole = 'peer' | 'self_archive';
declare const actionBrand: unique symbol;
export type PrivatePublicationAction = Readonly<{ [actionBrand]: true }>;
type NetworkAttempt = Readonly<{
  actionId: string;
  attempt: number;
  remaining(): number;
  finish(): void;
}>;
type Action = {
  session: PrivateSession;
  receipt: PairedDeliveryAcknowledgement;
  context: PairedDeliveryContext;
  current(): boolean;
  begin(
    role: PrivateDeliveryRole,
    origin: string,
    eventId: string
  ): NetworkAttempt | undefined;
};
const actions = new WeakMap<PrivatePublicationAction, Action>();
function immutablePair(receipt: PairedDeliveryAcknowledgement) {
  const row = pairedDeliveryAcknowledgementSnapshot(receipt);
  return (
    row &&
    JSON.stringify({
      owner: row.owner,
      id: row.id,
      peer: row.peer,
      rumorHash: row.rumorHash,
      createdAt: row.createdAt,
      self: row.self,
      peerArtifact: row.peerArtifact,
      deliveryPlan: row.deliveryPlan
    })
  );
}
// Source-owned meter shared by ordinary actions and the original probe action.
// Provider/native custody waits happen before begin; no caller clock or reset.
function networkMeter(probe: boolean) {
  const actionId = newLocalId();
  if (!actionId) return undefined;
  const counts = new Map<string, number>();
  let used = 0,
    running = false,
    broken = false,
    last = -1;
  function clock() {
    const now = performance.now();
    if (!Number.isFinite(now) || now < 0 || now < last) {
      broken = true;
      return undefined;
    }
    last = now;
    return now;
  }
  return {
    current: () => !broken,
    idle: () => !running,
    probeAvailable: (origin: string, eventId: string) =>
      probe &&
      !broken &&
      !running &&
      used < PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds &&
      (counts.get(JSON.stringify([origin, eventId])) ?? 0) <
        PRIVATE_PUBLICATION_BUDGETS.attemptsPerTargetAction,
    begin(
      role: PrivateDeliveryRole | 'access_archive' | 'access_readback',
      origin: string,
      eventId: string,
      current: () => boolean
    ): NetworkAttempt | undefined {
      if (
        broken ||
        !current() ||
        running ||
        used >= PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds
      )
        return undefined;
      const key = JSON.stringify(
          probe ? [origin, eventId] : [role, origin, eventId]
        ),
        count = counts.get(key) ?? 0;
      if (count >= PRIVATE_PUBLICATION_BUDGETS.attemptsPerTargetAction)
        return undefined;
      const start = clock();
      if (start === undefined) return undefined;
      counts.set(key, count + 1);
      running = true;
      let finished = false;
      function remaining() {
        const now = clock();
        return finished || now === undefined || !current()
          ? 0
          : Math.max(
              0,
              PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds -
                used -
                (now - start!)
            );
      }
      return {
        actionId,
        attempt: count + 1,
        remaining,
        finish() {
          if (finished) return;
          const now = clock();
          finished = true;
          running = false;
          used =
            now === undefined
              ? PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds
              : Math.min(
                  PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds,
                  used + now - start
                );
        }
      };
    }
  };
}
// One explicit action has one network meter, including all artifacts/targets.
// SDK identity approval and persisted readback happen before an attempt starts.
export function capturePrivatePublicationAction(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  review: unknown
): PrivatePublicationAction | undefined {
  if (review !== 'reviewed_private_retry') return undefined;
  const row = pairedDeliveryAcknowledgementSnapshot(receipt),
    original = immutablePair(receipt),
    ownership = privateSessionOwnership(session);
  if (!row || !original || !ownership?.current()) return undefined;
  for (const route of [
    row.deliveryPlan.routes.peer,
    row.deliveryPlan.routes.archive
  ])
    for (const origin of route.targets) {
      if (
        !capturePrivatePublication(
          repository,
          session,
          receipt,
          context,
          route.role,
          origin,
          'reviewed_private_delivery'
        )
      )
        return undefined;
    }
  const meter = networkMeter(false);
  if (!meter) return undefined;
  function current() {
    return (
      meter!.current() &&
      ownership!.current() &&
      immutablePair(receipt) === original &&
      recheckInboxRoutePlan(
        context.plan,
        context.policy,
        context.own,
        context.other
      ) === 'unchanged'
    );
  }
  const token = Object.freeze({}) as PrivatePublicationAction;
  actions.set(token, {
    session,
    receipt,
    context,
    current,
    begin: (role, origin, eventId) =>
      meter.begin(role, origin, eventId, current)
  });
  return token;
}
type Admission = {
  session: PrivateSession;
  policy: RelayPolicy;
  owner: string;
  command: string;
  role: PrivateDeliveryRole;
  origin: string;
  eventId: string;
  repository: PrivateStorageRepository;
  receipt: PairedDeliveryAcknowledgement;
  wire: string;
  destination: string;
  current(): boolean;
  take(): boolean;
  action?: Action;
};
const permissions = new WeakMap<PrivatePublication, Admission>();
// Genuine current local pair custody plus its frozen routes is necessary, but
// this capture opens no socket and does not claim recipient delivery. Each
// permission is one effect, not arbitrary bytes, retry or rerouting authority.
export function capturePrivatePublication(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  role: PrivateDeliveryRole,
  origin: string,
  review: unknown,
  action?: PrivatePublicationAction
): PrivatePublication | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_private_delivery' ||
    (role !== 'peer' && role !== 'self_archive') ||
    typeof origin !== 'string'
  )
    return undefined;
  try {
    const ownership = privateSessionOwnership(session),
      saved = pairedDeliveryAcknowledgementSnapshot(receipt),
      budget = action && actions.get(action);
    if (
      action &&
      (!budget ||
        budget.session !== session ||
        budget.receipt !== receipt ||
        budget.context !== context ||
        !budget.current())
    )
      return undefined;
    if (!ownership || !saved || ownership.owner !== saved.owner)
      return undefined;
    const policy = context.policy,
      plan = context.plan,
      own = context.own,
      other = context.other;
    const routes = inboxRoutePlanSnapshot(plan),
      manifest = readRelayPolicy(policy);
    if (
      !routes ||
      !manifest.messagingEnabled ||
      saved.deliveryPlan.state !== 'prepared' ||
      JSON.stringify(routes) !== JSON.stringify(saved.deliveryPlan.routes) ||
      recheckInboxRoutePlan(plan, policy, own, other) !== 'unchanged'
    )
      return undefined;
    const route = role === 'peer' ? routes.peer : routes.archive;
    const artifact = role === 'peer' ? saved.peerArtifact : saved.self;
    const destination = role === 'peer' ? saved.peer : saved.owner;
    if (
      route.role !== role ||
      route.author !== destination ||
      !route.targets.includes(origin) ||
      !manifest.inbox.some(
        ({ origin: allowed, write }) => allowed === origin && write
      ) ||
      !boundedUtf8(artifact.wire, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
    )
      return undefined;
    const verified = verifyEnvelope(artifact.wire),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !event ||
      event.kind !== 1059 ||
      event.id !== artifact.eventId ||
      event.tags.length !== 1 ||
      event.tags[0].length !== 2 ||
      event.tags[0][0] !== 'p' ||
      event.tags[0][1] !== destination
    )
      return undefined;
    const original = JSON.stringify(saved);
    let consumed = false;
    function current() {
      const fresh = pairedDeliveryAcknowledgementSnapshot(receipt),
        captured = privateSessionOwnership(session);
      return (
        ownership!.current() &&
        captured?.session === ownership!.session &&
        captured.owner === ownership!.owner &&
        !!fresh &&
        JSON.stringify(fresh) === original &&
        recheckInboxRoutePlan(plan, policy, own, other) === 'unchanged'
      );
    }
    if (!current()) return undefined;
    const token = Object.freeze({}) as PrivatePublication;
    permissions.set(token, {
      session,
      policy,
      owner: saved.owner,
      command: saved.id,
      role,
      origin,
      eventId: event.id,
      repository,
      receipt,
      wire: artifact.wire,
      destination,
      current,
      ...(budget ? { action: budget } : {}),
      take() {
        if (consumed) return false;
        consumed = true;
        return true;
      }
    });
    return token;
  } catch {
    return undefined;
  }
}
export function privatePublicationSnapshot(token: PrivatePublication) {
  const saved = permissions.get(token);
  return saved?.current()
    ? {
        owner: saved.owner,
        command: saved.command,
        role: saved.role,
        origin: saved.origin,
        eventId: saved.eventId
      }
    : undefined;
}
// The owned private pool consumes this after fresh SDK identity observation and
// under the owner lock. Actual full persisted record readback precedes every
// EVENT. A detached valid signature or guessed local ID cannot mint permission.
export async function takePrivatePublication(
  token: PrivatePublication,
  session: PrivateSession,
  policy: RelayPolicy,
  origin: string
) {
  const saved = permissions.get(token);
  if (
    !saved ||
    saved.session !== session ||
    saved.policy !== policy ||
    saved.origin !== origin ||
    !saved.current() ||
    !saved.take()
  )
    return undefined;
  try {
    if (
      !(await verifyPairedDeliveryAcknowledgement(
        saved.repository,
        saved.receipt
      )) ||
      !saved.current()
    )
      return undefined;
    if (
      saved.action &&
      pairedDeliveryTargetAccepted(
        saved.receipt,
        saved.role,
        saved.origin,
        saved.eventId
      )
    )
      return undefined;
    const verified = verifyEnvelope(saved.wire),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !event ||
      event.kind !== 1059 ||
      event.id !== saved.eventId ||
      event.tags.length !== 1 ||
      event.tags[0].length !== 2 ||
      event.tags[0][0] !== 'p' ||
      event.tags[0][1] !== saved.destination ||
      !saved.current()
    )
      return undefined;
    return {
      event,
      repository: saved.repository,
      // Frozen original operation/artifact scope for metadata-only settlement.
      // It survives session loss without becoming new effect permission.
      pairWire: (() => {
        const row = pairedDeliveryAcknowledgementSnapshot(saved.receipt)!;
        return JSON.stringify({
          owner: row.owner,
          id: row.id,
          peer: row.peer,
          rumorHash: row.rumorHash,
          createdAt: row.createdAt,
          self: row.self,
          peerArtifact: row.peerArtifact
        });
      })(),
      current: () => saved.current(),
      owner: saved.owner,
      command: saved.command,
      role: saved.role,
      origin: saved.origin,
      eventId: saved.eventId,
      beginNetwork: () =>
        saved.action
          ? saved.action.begin(saved.role, saved.origin, saved.eventId)
          : undefined,
      networkMetered: !!saved.action
    };
  } catch {
    return undefined;
  }
}

declare const probeActionBrand: unique symbol;
export type InboxProbePublicationAction = Readonly<{
  [probeActionBrand]: true;
}>;
declare const probePublicationBrand: unique symbol;
export type InboxProbePublication = Readonly<{ [probePublicationBrand]: true }>;
type ProbeAction = {
  source: 'construction' | 'recovered';
  token: InboxProbePublicationAction;
  repository: PrivateStorageRepository;
  session: PrivateSession;
  policy: RelayPolicy;
  identity: InboxProbeIdentity | RecoveredInboxProbe;
  original: string;
  current(): boolean;
  meter: NonNullable<ReturnType<typeof networkMeter>>;
};
const probeActions = new WeakMap<InboxProbePublicationAction, ProbeAction>();
const identityActions = new WeakMap<
  InboxProbeIdentity | RecoveredInboxProbe,
  ProbeAction | null
>();
type ProbeMeter = NonNullable<ReturnType<typeof networkMeter>>;
type ProbeMeterOwner = {
  access: InboxAccessController;
  original: string;
  meter: ProbeMeter;
  current(): boolean;
};
// Bounded within an actual private-session lifetime, never persisted authority.
// Retired action replacement reuses the slot; it cannot grow action history.
const commandMeters = new WeakMap<
  PrivateSession,
  Map<string, ProbeMeterOwner | null>
>();
function probeMeter(
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  command: string,
  original: string,
  current: () => boolean,
  replace: boolean
): ProbeMeter | undefined {
  const registry =
    commandMeters.get(session) ?? new Map<string, ProbeMeterOwner | null>();
  commandMeters.set(session, registry);
  const previous = registry.get(command);
  if (registry.has(command)) {
    if (!previous || previous.original !== original) return undefined;
    if (previous.access === access) return previous.meter;
    const retired = inboxAccessControllerSnapshot(previous.access);
    if (
      !replace ||
      previous.current() ||
      !previous.meter.idle() ||
      retired?.state !== 'stopped' ||
      retired.cleanupRequired ||
      !privatePoolProbeRetryIdle(session, policy) ||
      !current()
    )
      return undefined;
  } else if (
    registry.size >= LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
  ) {
    return undefined;
  }
  // UUID creation can reenter. No parallel original or recovered capture may
  // allocate another meter while this exact command's slot is reserved.
  registry.set(command, null);
  const meter = networkMeter(true);
  if (
    !meter ||
    !current() ||
    (replace && !privatePoolProbeRetryIdle(session, policy))
  )
    return undefined;
  registry.set(command, { access, original, meter, current });
  return meter;
}
function probeImmutable(
  row: NonNullable<ReturnType<typeof inboxProbePublicationBinding>>['record']
) {
  return JSON.stringify({
    owner: row.owner,
    id: row.id,
    rumorHash: row.rumorHash,
    createdAt: row.createdAt,
    binding: row.binding,
    self: row.self
  });
}
export function captureInboxProbePublicationAction(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  acknowledgement: InboxProbeAcknowledgement,
  review: unknown
): InboxProbePublicationAction | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_probe_network'
  )
    return undefined;
  try {
    const binding = inboxProbePublicationBinding(
      repository,
      session,
      acknowledgement,
      policy
    );
    if (!binding || !binding.record.self) return undefined;
    const access = inboxProbeActionOwner(binding.identity, session, policy);
    if (!access || !binding.current()) return undefined;
    const original = probeImmutable(binding.record),
      prior = identityActions.get(binding.identity);
    if (identityActions.has(binding.identity))
      return prior &&
        prior.repository === repository &&
        prior.session === session &&
        prior.policy === policy &&
        prior.original === original &&
        prior.current()
        ? prior.token
        : undefined;
    // Reserve this original identity before callback-capable UUID creation.
    // Reentrant capture refuses; failed admission never grants a new meter.
    identityActions.set(binding.identity, null);
    const meter = probeMeter(
      session,
      policy,
      access,
      binding.record.id,
      original,
      binding.current,
      false
    );
    if (!meter || !binding.current()) return undefined;
    const token = Object.freeze({}) as InboxProbePublicationAction;
    const captured: ProbeAction = {
      source: 'construction',
      token,
      repository,
      session,
      policy,
      identity: binding.identity,
      original,
      current: () => meter.current() && binding.current(),
      meter
    };
    identityActions.set(binding.identity, captured);
    probeActions.set(token, captured);
    return token;
  } catch {
    return undefined;
  }
}

// Fresh deliberate Retry adopts the same ciphertext, not a construction
// identity. Admission is local readonly work; EVENT/REQ need later permissions.
export async function captureRecoveredInboxProbeRetryAction(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  acknowledgement: InboxProbeAcknowledgement,
  review: unknown
): Promise<InboxProbePublicationAction | undefined> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_original_inbox_probe_retry'
  )
    return undefined;
  let identity: RecoveredInboxProbe | undefined,
    retryBusy = false;
  try {
    const binding = recoveredInboxProbePublicationBinding(
      repository,
      session,
      acknowledgement,
      policy
    );
    if (!binding || !binding.record.self) return undefined;
    identity = binding.identity;
    const original = probeImmutable(binding.record),
      previous = identityActions.get(identity);
    if (identityActions.has(identity))
      return previous &&
        previous.repository === repository &&
        previous.session === session &&
        previous.policy === policy &&
        previous.original === original &&
        previous.current()
        ? previous.token
        : undefined;
    identityActions.set(identity, null);
    if (!navigator.locks?.request) return undefined;
    const result = await navigator.locks.request(
      'harvestcircle:owner:' + binding.record.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) {
          retryBusy = true;
          return undefined;
        }
        if (!binding.current()) return undefined;
        const idle = await withIdlePrivateLocalStorage(session, async () => {
          if (!binding.current()) return undefined;
          if (!privatePoolProbeRetryIdle(session, policy)) {
            retryBusy = true;
            return undefined;
          }
          if (
            !(await verifyInboxProbeAcknowledgement(
              repository,
              session,
              acknowledgement
            )) ||
            !binding.current() ||
            !privatePoolProbeRetryIdle(session, policy)
          )
            return undefined;
          const meter = probeMeter(
            session,
            policy,
            binding.access,
            binding.record.id,
            original,
            binding.current,
            true
          );
          // UUID/currentness callbacks finish before separate final native
          // custody observation. A failed admission cannot mint a permission.
          if (
            !meter ||
            ![
              ...binding.record.binding.archiveTargets,
              ...binding.record.binding.readTargets
            ].some((origin) =>
              meter.probeAvailable(origin, binding.record.self!.eventId)
            ) ||
            !(await verifyInboxProbeAcknowledgement(
              repository,
              session,
              acknowledgement
            )) ||
            !binding.current() ||
            !privatePoolProbeRetryIdle(session, policy)
          )
            return undefined;
          const token = Object.freeze({}) as InboxProbePublicationAction;
          const captured: ProbeAction = {
            source: 'recovered',
            token,
            repository,
            session,
            policy,
            identity: binding.identity,
            original,
            current: () => meter.current() && binding.current(),
            meter
          };
          identityActions.set(binding.identity, captured);
          probeActions.set(token, captured);
          return token;
        });
        if (!idle.ok) retryBusy = true;
        return idle.ok ? idle.value : undefined;
      }
    );
    return result;
  } catch {
    return undefined;
  } finally {
    // Only actual pre-admission busy can explicitly retry this same proof.
    if (retryBusy && identity && identityActions.get(identity) === null)
      identityActions.delete(identity);
  }
}
type ProbePermission = {
  role: 'access_archive' | 'access_readback';
  action: ProbeAction;
  acknowledgement: InboxProbeAcknowledgement;
  wire: string;
  predecessorWire: string;
  owner: string;
  command: string;
  origin: string;
  eventId: string;
  current(): boolean;
  take(): boolean;
};
const probePermissions = new WeakMap<InboxProbePublication, ProbePermission>();
declare const probeReadbackBrand: unique symbol;
export type InboxProbeReadback = Readonly<{ [probeReadbackBrand]: true }>;
const probeReadbacks = new WeakMap<InboxProbeReadback, ProbePermission>();
function captureProbePermission(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  acknowledgement: InboxProbeAcknowledgement,
  action: InboxProbePublicationAction,
  origin: string,
  role: ProbePermission['role']
): ProbePermission | undefined {
  if (
    typeof window === 'undefined' ||
    (role !== 'access_archive' && role !== 'access_readback')
  )
    return undefined;
  try {
    const captured = probeActions.get(action),
      bind =
        captured?.source === 'recovered'
          ? recoveredInboxProbePublicationBinding
          : inboxProbePublicationBinding,
      binding = bind(repository, session, acknowledgement, policy);
    if (
      !captured ||
      !binding ||
      captured.repository !== repository ||
      captured.session !== session ||
      captured.policy !== policy ||
      captured.identity !== binding.identity ||
      !captured.current() ||
      captured.original !== probeImmutable(binding.record) ||
      !binding.record.self ||
      (captured.source === 'recovered' &&
        !captured.meter.probeAvailable(origin, binding.record.self.eventId)) ||
      !(
        role === 'access_archive'
          ? binding.record.binding.archiveTargets
          : binding.record.binding.readTargets
      ).includes(origin)
    )
      return undefined;
    if (
      !readRelayPolicy(policy).inbox.some(
        ({ origin: allowed, read, write }) =>
          allowed === origin && (role === 'access_archive' ? write : read)
      )
    )
      return undefined;
    const artifact = binding.record.self,
      verified = verifyEnvelope(artifact.wire),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !event ||
      event.kind !== 1059 ||
      event.id !== artifact.eventId ||
      JSON.stringify(event.tags) !==
        JSON.stringify([['p', binding.record.owner]])
    )
      return undefined;
    const predecessorWire = binding.wire;
    let consumed = false;
    function current() {
      const fresh = bind(repository, session, acknowledgement, policy);
      return (
        captured!.current() &&
        !!fresh &&
        (captured!.source !== 'recovered' ||
          consumed ||
          captured!.meter.probeAvailable(origin, artifact.eventId)) &&
        fresh.identity === captured!.identity &&
        fresh.wire === predecessorWire &&
        probeImmutable(fresh.record) === captured!.original
      );
    }
    return {
      role,
      action: captured,
      acknowledgement,
      wire: artifact.wire,
      predecessorWire,
      owner: binding.record.owner,
      command: binding.record.id,
      origin,
      eventId: event.id,
      current,
      take() {
        if (consumed) return false;
        consumed = true;
        return true;
      }
    };
  } catch {
    return undefined;
  }
}
export function captureInboxProbePublication(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  acknowledgement: InboxProbeAcknowledgement,
  action: InboxProbePublicationAction,
  origin: string,
  review: unknown
): InboxProbePublication | undefined {
  if (review !== 'reviewed_inbox_probe_publication') return undefined;
  const saved = captureProbePermission(
    repository,
    session,
    policy,
    acknowledgement,
    action,
    origin,
    'access_archive'
  );
  if (!saved) return undefined;
  const token = Object.freeze({}) as InboxProbePublication;
  probePermissions.set(token, saved);
  return token;
}
export function captureInboxProbeReadback(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  acknowledgement: InboxProbeAcknowledgement,
  action: InboxProbePublicationAction,
  origin: string,
  review: unknown
): InboxProbeReadback | undefined {
  if (review !== 'reviewed_inbox_probe_readback') return undefined;
  const saved = captureProbePermission(
    repository,
    session,
    policy,
    acknowledgement,
    action,
    origin,
    'access_readback'
  );
  if (!saved) return undefined;
  const token = Object.freeze({}) as InboxProbeReadback;
  probeReadbacks.set(token, saved);
  return token;
}
export function inboxProbeReadbackSnapshot(permission: InboxProbeReadback) {
  const saved = probeReadbacks.get(permission);
  return saved?.current()
    ? {
        owner: saved.owner,
        command: saved.command,
        role: 'access_readback' as const,
        origin: saved.origin,
        eventId: saved.eventId
      }
    : undefined;
}
export function inboxProbePublicationSnapshot(
  permission: InboxProbePublication
) {
  const saved = probePermissions.get(permission);
  return saved?.current()
    ? {
        owner: saved.owner,
        command: saved.command,
        role: 'access_archive' as const,
        origin: saved.origin,
        eventId: saved.eventId
      }
    : undefined;
}
async function takeProbePermission(
  saved: ProbePermission | undefined,
  session: PrivateSession,
  policy: RelayPolicy,
  origin: string
) {
  if (
    !saved ||
    saved.action.session !== session ||
    saved.action.policy !== policy ||
    saved.origin !== origin ||
    !saved.current() ||
    !saved.take()
  )
    return undefined;
  if (
    !(await verifyInboxProbeAcknowledgement(
      saved.action.repository,
      session,
      saved.acknowledgement
    )) ||
    !saved.current()
  )
    return undefined;
  const verified = verifyEnvelope(saved.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
  if (
    !event ||
    event.kind !== 1059 ||
    event.id !== saved.eventId ||
    JSON.stringify(event.tags) !== JSON.stringify([['p', saved.owner]]) ||
    !saved.current()
  )
    return undefined;
  return {
    event,
    repository: saved.action.repository,
    acknowledgement: saved.acknowledgement,
    predecessorWire: saved.predecessorWire,
    current: () => saved.current(),
    owner: saved.owner,
    command: saved.command,
    role: saved.role,
    origin,
    eventId: saved.eventId,
    networkMetered: true,
    beginNetwork: () =>
      saved.action.meter.begin(saved.role, origin, saved.eventId, () =>
        saved.current()
      )
  };
}

export async function takeInboxProbePublication(
  permission: InboxProbePublication,
  session: PrivateSession,
  policy: RelayPolicy,
  origin: string
) {
  const admitted = await takeProbePermission(
    probePermissions.get(permission),
    session,
    policy,
    origin
  );
  return admitted && { ...admitted, role: 'access_archive' as const };
}
export async function takeInboxProbeReadback(
  permission: InboxProbeReadback,
  session: PrivateSession,
  policy: RelayPolicy,
  origin: string
) {
  const admitted = await takeProbePermission(
    probeReadbacks.get(permission),
    session,
    policy,
    origin
  );
  return admitted && { ...admitted, role: 'access_readback' as const };
}
