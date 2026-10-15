import type { RelayPolicy } from '../config/relays.ts';
import {
  inboxOperatorQualificationSnapshot,
  inboxOperatorQualificationWire,
  type InboxOperatorQualification
} from '../config/inbox-operators.ts';
import {
  inboxAccessConstructionBinding,
  inboxAccessControllerSourceSnapshot,
  subscribeInboxAccessControllerClose,
  type InboxAccessController
} from './inbox-access-controller.ts';
import type { OwnInboxAccessContext, ObserveInboxAccess } from './readiness.ts';
import {
  captureInboxProbeCleanupSelection,
  inboxProbeCleanupSelectionBinding,
  type InboxProbeNonceAcknowledgement
} from '../persistence/inbox-probes.ts';
import {
  inboxProbeRemovalAcknowledgementBinding,
  type InboxProbeRemovalAcknowledgement
} from '../persistence/inbox-probe-cleanup.ts';
import {
  captureInboxProbeQualificationFence,
  inboxProbeQualificationFenceCurrent,
  inboxProbeCleanupReady
} from '../nostr/private-publisher.ts';
import {
  privateSessionOwnership,
  recheckPrivateSession,
  subscribePrivateSessionClose,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  observePrivateRecord,
  privateStorageOwner,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from '../persistence/private-storage.ts';
import { privateRecordWire } from '../persistence/private-records.ts';

declare const qualificationBrand: unique symbol;
export type InboxAccessQualification = Readonly<{ [qualificationBrand]: true }>;
export type InboxAccessQualificationResult = Readonly<{
  status: 'qualified' | 'invalid' | 'busy' | 'stopped' | PrivateStorageFailure;
  qualification?: InboxAccessQualification;
}>;
type Metadata = Readonly<{
  owner: string;
  command: string;
  eventId: string;
  preferenceId: string;
  readTargets: readonly string[];
  archiveTargets: readonly string[];
  operatorWire: string;
}>;
type Scope = Metadata &
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    policy: RelayPolicy;
    access: InboxAccessController;
    operator: InboxOperatorQualification;
    identitySession: symbol;
    authority: 'source_fixture' | 'operator';
    current(): boolean;
  }>;
type Action = Readonly<{
  scope: Scope;
  current(): boolean;
  pending(): boolean;
  result(): InboxAccessQualification | undefined;
  begin(): boolean;
  finish(): void;
  stop(): void;
  issue(): InboxAccessQualification;
}>;
const actions = new WeakMap<object, Action>();
const qualifications = new WeakMap<InboxAccessQualification, Action>();
function same(a: readonly string[], b: readonly string[]) {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}
function makeScope(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  operator: InboxOperatorQualification,
  metadata: Metadata
): Scope | undefined {
  const actual = inboxAccessConstructionBinding(access, session, policy),
    owner = privateSessionOwnership(session),
    declaration = inboxOperatorQualificationSnapshot(operator);
  if (
    !actual ||
    !owner ||
    !declaration ||
    declaration.authority === 'unconfigured' ||
    privateStorageOwner(repository) !== metadata.owner ||
    owner.owner !== metadata.owner ||
    actual.owner !== metadata.owner ||
    actual.session !== owner.session ||
    actual.preferenceId !== metadata.preferenceId ||
    !same(actual.readTargets, metadata.readTargets) ||
    !same(actual.archiveTargets, metadata.archiveTargets) ||
    inboxOperatorQualificationWire(operator, policy) !== metadata.operatorWire
  )
    return undefined;
  const targets = Array.from(
    new Set([...actual.readTargets, ...actual.archiveTargets])
  );
  if (
    !targets.every((origin) => {
      const row = declaration.relays.find((value) => value.origin === origin);
      return (
        row &&
        row.auth !== 'unsupported' &&
        row.recipientRead === 'supported' &&
        row.namedArchiveAck === 'supported' &&
        row.exactReadback === 'supported' &&
        row.retention === 'operator_defined'
      );
    })
  )
    return undefined;
  function current() {
    const snapshot = inboxAccessControllerSourceSnapshot(access);
    return (
      actual!.current() &&
      owner!.current() &&
      inboxOperatorQualificationWire(operator, policy) ===
        metadata.operatorWire &&
      !!snapshot &&
      !snapshot.busy &&
      !snapshot.cleanupRequired &&
      snapshot.state !== 'stopped' &&
      targets.every((origin) => {
        const required = declaration!.relays.find(
            (value) => value.origin === origin
          )!,
          auth = snapshot.auth.find((value) => value.origin === origin);
        return required.auth === 'explicit_required'
          ? auth?.state === 'accepted'
          : !auth || auth.state === 'accepted';
      }) &&
      actual!.current() &&
      owner!.current()
    );
  }
  return current()
    ? {
        ...metadata,
        readTargets: metadata.readTargets.slice(),
        archiveTargets: metadata.archiveTargets.slice(),
        repository,
        session,
        policy,
        access,
        operator,
        identitySession: owner.session,
        authority: declaration.authority,
        current
      }
    : undefined;
}
function compatible(action: Action, scope: Scope) {
  const saved = action.scope;
  return (
    saved.repository === scope.repository &&
    saved.session === scope.session &&
    saved.policy === scope.policy &&
    saved.access === scope.access &&
    saved.operator === scope.operator &&
    saved.command === scope.command &&
    saved.eventId === scope.eventId &&
    saved.operatorWire === scope.operatorWire
  );
}
// The currentness port is created only by the two authenticated factories below;
// no caller currentness, epoch or coverage callback enters this private owner.
function createAction(scope: Scope, sourceCurrent: () => boolean): Action {
  let stopped = false,
    pending = false,
    token: InboxAccessQualification | undefined;
  const listeners = new Map<symbol, () => void>();
  function current() {
    return !stopped && scope.current() && sourceCurrent();
  }
  function stop() {
    stopped = true;
    for (const [id, off] of listeners) {
      off();
      listeners.delete(id);
    }
  }
  const saved: Action = {
    scope,
    current,
    pending: () => pending,
    result: () => token,
    begin() {
      if (pending || !current()) return false;
      pending = true;
      return true;
    },
    finish() {
      pending = false;
    },
    stop,
    issue() {
      token ??= Object.freeze({}) as InboxAccessQualification;
      qualifications.set(token, saved);
      return token;
    }
  };
  listeners.set(Symbol(), subscribePrivateSessionClose(scope.session, stop));
  listeners.set(
    Symbol(),
    subscribeInboxAccessControllerClose(scope.access, stop)
  );
  if (!current()) stop();
  return saved;
}
function result(action: Action): InboxAccessQualificationResult | undefined {
  const token = action.result();
  if (!action.current()) return { status: 'stopped' };
  if (token) return { status: 'qualified', qualification: token };
  if (action.pending()) return { status: 'busy' };
  return undefined;
}

// Explicit qualification, without a new probe, network attempt or native write.
export async function qualifyInboxAccess(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  operator: InboxOperatorQualification,
  nonce: InboxProbeNonceAcknowledgement,
  observations: unknown,
  review: unknown
): Promise<InboxAccessQualificationResult> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_qualify_inbox_access'
  )
    return { status: 'invalid' };
  try {
    const selection = captureInboxProbeCleanupSelection(
        repository,
        session,
        policy,
        nonce,
        observations,
        'reviewed_inbox_probe_cleanup_selection'
      ),
      binding =
        selection &&
        inboxProbeCleanupSelectionBinding(
          selection,
          repository,
          session,
          policy
        );
    if (
      !selection ||
      !binding ||
      binding.access !== access ||
      binding.operator !== operator
    )
      return { status: 'invalid_scope' };
    const scope = makeScope(repository, session, policy, access, operator, {
      owner: binding.record.owner,
      command: binding.record.id,
      eventId: binding.record.self!.eventId,
      preferenceId: binding.record.binding.preferenceId,
      readTargets: binding.record.binding.readTargets,
      archiveTargets: binding.record.binding.archiveTargets,
      operatorWire: binding.record.binding.operatorWire
    });
    if (!scope) return { status: 'invalid_scope' };
    let action = actions.get(nonce);
    if (action && !compatible(action, scope))
      return { status: 'invalid_scope' };
    if (!action) {
      const fence = captureInboxProbeQualificationFence(
        repository,
        session,
        policy,
        selection
      );
      if (!fence) {
        // A later append must not let this same old nonce resample authority.
        actions.set(
          nonce,
          createAction(scope, () => false)
        );
        return { status: 'stopped' };
      }
      action = createAction(scope, () =>
        inboxProbeQualificationFenceCurrent(fence)
      );
      actions.set(nonce, action);
    }
    const cached = result(action);
    if (cached) return cached;
    const saved = action;
    if (!saved.begin()) return { status: saved.current() ? 'busy' : 'stopped' };
    try {
      if (!navigator.locks?.request) return { status: 'unavailable' };
      return await navigator.locks.request(
        'harvestcircle:owner:' + scope.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock): Promise<InboxAccessQualificationResult> => {
          if (!lock) return { status: 'busy' };
          if (!saved.current()) return { status: 'stopped' };
          if (!inboxProbeCleanupReady(repository, session, policy, selection))
            return { status: saved.current() ? 'busy' : 'stopped' };
          if (!(await recheckPrivateSession(session)) || !saved.current())
            return { status: saved.current() ? 'busy' : 'stopped' };
          const job =
            await withIdlePrivateLocalStorage<InboxAccessQualificationResult>(
              session,
              async () => {
                if (!saved.current()) return { status: 'stopped' };
                if (!binding.begin()) return { status: 'busy' };
                try {
                  const native = await observePrivateRecord(
                    repository,
                    'private_sends',
                    scope.command
                  );
                  if (!native.ok) return { status: native.reason };
                  if (!saved.current()) return { status: 'stopped' };
                  if (
                    native.value.wire !== binding.wire ||
                    privateRecordWire(
                      native.value.handle,
                      scope.owner,
                      scope.command
                    ) !== binding.wire
                  )
                    return { status: 'conflict' };
                  if (!saved.current()) return { status: 'stopped' };
                  return { status: 'qualified', qualification: saved.issue() };
                } finally {
                  binding.finish();
                }
              }
            );
          if (!saved.current()) return { status: 'stopped' };
          return job.ok ? job.value : { status: 'busy' };
        }
      );
    } finally {
      saved.finish();
    }
  } catch {
    return { status: 'unknown_completion' };
  }
}

// This path consumes source-authenticated pre-disposal admission coverage as
// well as actual successful native removal. Mere genuine cleanup is insufficient.
export function qualifyRemovedInboxAccess(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  operator: InboxOperatorQualification,
  removal: InboxProbeRemovalAcknowledgement,
  review: unknown
): InboxAccessQualificationResult {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_qualify_inbox_access'
  )
    return { status: 'invalid' };
  try {
    const binding = inboxProbeRemovalAcknowledgementBinding(
        removal,
        repository,
        session,
        policy
      ),
      coverage = binding?.qualification;
    if (
      !binding ||
      !coverage ||
      binding.access !== access ||
      binding.operator !== operator ||
      coverage.access !== access ||
      coverage.operator !== operator ||
      !coverage.current()
    )
      return { status: 'invalid_scope' };
    const scope = makeScope(
      repository,
      session,
      policy,
      access,
      operator,
      binding
    );
    if (!scope) return { status: 'invalid_scope' };
    let action = actions.get(removal);
    if (action && !compatible(action, scope))
      return { status: 'invalid_scope' };
    if (!action) {
      action = createAction(scope, () => {
        const fresh = inboxProbeRemovalAcknowledgementBinding(
          removal,
          repository,
          session,
          policy
        );
        return (
          !!fresh?.qualification &&
          fresh.access === access &&
          fresh.operator === operator &&
          fresh.qualification.current() &&
          fresh.current()
        );
      });
      actions.set(removal, action);
    }
    const cached = result(action);
    if (cached) return cached;
    return action.current()
      ? { status: 'qualified', qualification: action.issue() }
      : { status: 'stopped' };
  } catch {
    return { status: 'invalid' };
  }
}

export function inboxAccessQualificationSnapshot(
  token: InboxAccessQualification
) {
  const saved = qualifications.get(token);
  return saved?.current()
    ? {
        owner: saved.scope.owner,
        command: saved.scope.command,
        preferenceId: saved.scope.preferenceId,
        authority: saved.scope.authority,
        sourceOnly: true
      }
    : undefined;
}
export function stopInboxAccessQualification(
  token: InboxAccessQualification
): void {
  qualifications.get(token)?.stop();
}
// Existing projections receive only an informational view of a genuine proof.
// No public object or caller current callback can enter the qualification map.
export function observeQualifiedInboxAccess(
  token: InboxAccessQualification
): ObserveInboxAccess {
  return (context: OwnInboxAccessContext) => {
    try {
      const saved = qualifications.get(token);
      if (!saved?.current()) return undefined;
      const scope = saved.scope;
      if (
        context.owner !== scope.owner ||
        context.session !== scope.identitySession ||
        context.preferenceId !== scope.preferenceId ||
        !same(scope.readTargets, context.readTargets) ||
        !same(scope.archiveTargets, context.archiveTargets) ||
        !saved.current()
      )
        return undefined;
      return {
        owner: scope.owner,
        session: scope.identitySession,
        preferenceId: scope.preferenceId,
        readTargets: scope.readTargets.slice(),
        archiveTargets: scope.archiveTargets.slice(),
        receive: 'qualified_exercised',
        archive: 'qualified_exercised',
        current: saved.current
      };
    } catch {
      return undefined;
    }
  };
}
