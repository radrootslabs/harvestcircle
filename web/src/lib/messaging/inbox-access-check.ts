import type { RelayPolicy } from '../config/relays.ts';
import {
  inboxOperatorQualificationSnapshot,
  inboxOperatorQualificationWire,
  type InboxOperatorQualification
} from '../config/inbox-operators.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateStorageOwner,
  type PrivateStorageRepository
} from '../persistence/private-storage.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  inboxAccessConstructionBinding,
  inboxAccessControllerSourceSnapshot,
  subscribeInboxAccessControllerClose,
  closeInboxAccessController,
  type InboxAccessController
} from './inbox-access-controller.ts';
import {
  reserveInboxProbeIdentity,
  inboxProbeSnapshot,
  stopInboxProbeIdentity,
  type InboxProbeIdentity
} from '../nostr/inbox-access.ts';
import {
  captureInboxProbeSealOperation,
  buildInboxProbeSeal,
  stopInboxProbeSeal,
  type InboxProbeSealOperation
} from '../nostr/seal-builder.ts';
import {
  buildInboxProbeGiftwrap,
  type InboxProbeGiftwrap
} from '../nostr/giftwrap-builder.ts';
import {
  commitInboxProbe,
  adoptRecoveredInboxProbe,
  commitInboxProbeArchiveAttempt,
  commitInboxProbeReadbackAttempt,
  commitInboxProbeNonceVerification,
  type InboxProbeAcknowledgement,
  type InboxProbeNonceAcknowledgement
} from '../persistence/inbox-probes.ts';
import {
  captureInboxProbePublicationAction,
  captureRecoveredInboxProbeRetryAction,
  captureInboxProbePublication,
  captureInboxProbeReadback,
  type InboxProbePublicationAction
} from '../nostr/private-publisher.ts';
import {
  getPrivatePool,
  closePrivatePool,
  privatePoolCleanupRequired,
  publishInboxProbeArchiveAttempt,
  readInboxProbeAttempt,
  inboxProbeArchiveAttemptEvidence,
  inboxProbeReadbackAttemptEvidence,
  type PrivatePool,
  type InboxProbeArchiveAttemptResult,
  type InboxProbeReadbackAttemptResult
} from '../nostr/private-pool.ts';
import {
  captureInboxProbeRecoveryReader,
  recoverInboxProbe,
  stopInboxProbeRecoveryReader,
  captureInboxProbeNonceReader,
  readInboxProbeNonce,
  stopInboxProbeNonceReader,
  captureRecoveredInboxProbeNonceReader,
  readRecoveredInboxProbeNonce,
  stopRecoveredInboxProbeNonceReader,
  type InboxProbeRecoveryReader,
  type RecoveredInboxProbe,
  type InboxProbeNonceReader,
  type RecoveredInboxProbeNonceReader,
  type InboxProbeNonceProof,
  type RecoveredInboxProbeNonceProof
} from '../nostr/self-recovery-reader.ts';
import {
  qualifyInboxAccess,
  qualifyRemovedInboxAccess,
  inboxAccessQualificationSnapshot,
  stopInboxAccessQualification,
  type InboxAccessQualification
} from './inbox-access-qualification.ts';
import {
  captureInboxProbeCleanup,
  deleteReviewedInboxProbe,
  stopInboxProbeCleanup,
  type InboxProbeCleanup,
  type InboxProbeRemovalAcknowledgement
} from '../persistence/inbox-probe-cleanup.ts';

declare const checkBrand: unique symbol;
export type InboxAccessCheck = Readonly<{ [checkBrand]: true }>;
declare const reviewBrand: unique symbol;
export type InboxAccessCheckReview = Readonly<{ [reviewBrand]: true }>;
export type InboxAccessCheckResult = Readonly<{ status: string }>;
type Mode = 'original' | 'recover';
const disclosures = Object.freeze([
  'This private inbox diagnostic may appear as a note to self or an extra event in other compatible clients.',
  'Relay operators may retain the diagnostic remotely after local removal. Local journal cleanup does not delete remote copies.',
  'After journal loss, removal or reload, the diagnostic uses ordinary bounded receive and retention costs and two-layer decryption before authenticated self-message rejection.',
  'This check is not an enquiry, reply, delivery receipt or proof that another person or client received a message.'
]);
type CheckSelection = Readonly<{
  kind: 'check';
  mode: Mode;
  origin: string;
  command?: string;
}>;
type Selection = CheckSelection | Readonly<{ kind: 'cleanup' | 'reconcile' }>;
type Snapshot = Readonly<{
  state: string;
  busy: boolean;
  cleanupRequired: boolean;
  pendingSettlement: boolean;
  sourceOnly: true;
  mode?: Mode;
  command?: string;
  reason: string | null;
}>;
type Control = Readonly<{
  repository: PrivateStorageRepository;
  identity: IdentitySession;
  session: PrivateSession;
  policy: RelayPolicy;
  operator: InboxOperatorQualification;
  current(): boolean;
  snapshot(): Snapshot;
  select(
    mode: unknown,
    origin: unknown,
    command: unknown
  ): InboxAccessCheckReview | undefined;
  selectAction(
    kind: 'cleanup' | 'reconcile'
  ): InboxAccessCheckReview | undefined;
  reviewCurrent(selection: Selection): boolean;
  run(selection: CheckSelection): Promise<InboxAccessCheckResult>;
  cleanup(review: unknown): Promise<InboxAccessCheckResult>;
  reconcile(review: unknown): Promise<InboxAccessCheckResult>;
  qualification(): InboxAccessQualification | undefined;
  stop(): boolean;
}>;
const controls = new WeakMap<InboxAccessCheck, Control>();
const currentChecks = new WeakMap<InboxAccessController, InboxAccessCheck>();
const reviews = new WeakMap<
  InboxAccessCheckReview,
  Readonly<{
    control: Control;
    selection: Selection;
  }>
>();
function applied(status: string) {
  return status === 'saved' || status === 'existing' || status === 'reconciled';
}

// Capture is source metadata only. Actual C7 association precedes passive
// session observation; this coordinator owns the exact opaque operator binding.
export function createInboxAccessCheck(
  repository: PrivateStorageRepository,
  identity: IdentitySession,
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  operator: InboxOperatorQualification
): InboxAccessCheck | undefined {
  if (typeof window === 'undefined') return undefined;
  try {
    const source = inboxAccessConstructionBinding(access, session, policy);
    if (!source || privateStorageOwner(repository) !== source.owner)
      return undefined;
    const publicOwner = identityMessagingOwnership(identity),
      owner = privateSessionOwnership(session),
      declaration = inboxOperatorQualificationSnapshot(operator),
      operatorWire = inboxOperatorQualificationWire(operator, policy);
    if (
      !publicOwner?.current() ||
      !owner?.current() ||
      publicOwner.owner !== source.owner ||
      owner.owner !== source.owner ||
      publicOwner.session !== source.session ||
      owner.session !== source.session ||
      !declaration ||
      declaration.authority === 'unconfigured' ||
      !operatorWire ||
      ![...source.readTargets, ...source.archiveTargets].every((origin) => {
        const row = declaration.relays.find((row) => row.origin === origin);
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
    const prior = currentChecks.get(access),
      previous = prior && controls.get(prior);
    if (previous)
      return previous.repository === repository &&
        previous.identity === identity &&
        previous.session === session &&
        previous.policy === policy &&
        previous.operator === operator &&
        previous.current()
        ? prior
        : undefined;
    const token = Object.freeze({}) as InboxAccessCheck;
    const cancellation = new AbortController();
    const archive = new Map<string, InboxProbeArchiveAttemptResult>();
    const listeners = new Map<symbol, () => void>();
    let stopped = false,
      busy = false,
      cleanupRequired = false,
      state = 'not_checked',
      reason: string | null = null,
      mode: Mode | undefined,
      readOrigin: string | undefined,
      command: string | undefined,
      selectedReview: InboxAccessCheckReview | undefined,
      original: InboxProbeIdentity | undefined,
      seal: InboxProbeSealOperation | undefined,
      wrap: InboxProbeGiftwrap | undefined,
      recovery: InboxProbeRecoveryReader | undefined,
      recovered: RecoveredInboxProbe | undefined,
      acknowledgement: InboxProbeAcknowledgement | undefined,
      action: InboxProbePublicationAction | undefined,
      pool: PrivatePool | undefined,
      pendingArchive:
        | Readonly<{ origin: string; result: InboxProbeArchiveAttemptResult }>
        | undefined,
      pendingRead: InboxProbeReadbackAttemptResult | undefined,
      read: InboxProbeReadbackAttemptResult | undefined,
      nonceReader: InboxProbeNonceReader | undefined,
      recoveredNonceReader: RecoveredInboxProbeNonceReader | undefined,
      nonceProof:
        InboxProbeNonceProof | RecoveredInboxProbeNonceProof | undefined,
      nonce: InboxProbeNonceAcknowledgement | undefined,
      qualified: InboxAccessQualification | undefined,
      cleanup: InboxProbeCleanup | undefined,
      removal: InboxProbeRemovalAcknowledgement | undefined;
    function current() {
      if (stopped) return false;
      const fresh =
        source!.current() &&
        publicOwner!.current() &&
        owner!.current() &&
        inboxOperatorQualificationWire(operator, policy) === operatorWire;
      if (!fresh && !stopped) {
        revoke();
        closeOwn();
      }
      return fresh && !stopped;
    }
    function qualification() {
      return current() &&
        !busy &&
        !cleanupRequired &&
        qualified &&
        inboxAccessQualificationSnapshot(qualified)
        ? qualified
        : undefined;
    }
    function result(status: string): InboxAccessCheckResult {
      reason = status === 'qualified' ? null : status;
      state =
        stopped || !current()
          ? 'stopped'
          : status === 'qualified'
            ? 'qualified'
            : 'needs_action';
      return { status };
    }
    function observeCleanup() {
      cleanupRequired =
        !!pendingArchive ||
        !!pendingRead ||
        (!!pool && privatePoolCleanupRequired(pool)) ||
        !!inboxAccessControllerSourceSnapshot(access)?.cleanupRequired;
      return !cleanupRequired;
    }
    function revoke() {
      stopped = true;
      state = 'stopped';
      cancellation.abort();
      if (selectedReview) reviews.delete(selectedReview);
      selectedReview = undefined;
      if (qualified) stopInboxAccessQualification(qualified);
      qualified = undefined;
      if (nonceReader) stopInboxProbeNonceReader(nonceReader);
      if (recoveredNonceReader)
        stopRecoveredInboxProbeNonceReader(recoveredNonceReader);
      if (recovery) stopInboxProbeRecoveryReader(recovery);
      if (seal) stopInboxProbeSeal(seal);
      if (original) stopInboxProbeIdentity(original);
      if (cleanup) stopInboxProbeCleanup(cleanup);
    }
    function closeOwn() {
      let failed = busy;
      if (pool) {
        try {
          closePrivatePool(pool);
        } catch {
          failed = true;
        }
        if (privatePoolCleanupRequired(pool)) failed = true;
      }
      cleanupRequired = failed || !!pendingArchive || !!pendingRead;
      if (!cleanupRequired)
        for (const [id, off] of listeners) {
          off();
          listeners.delete(id);
        }
      return !cleanupRequired;
    }
    function stop() {
      revoke();
      const own = closeOwn();
      const accessClosed = closeInboxAccessController(access);
      cleanupRequired = !own || !accessClosed;
      return !cleanupRequired;
    }
    function sourceClosed() {
      revoke();
      // The source is already inside its close owner. Calling close() again
      // would report its legitimate reentry as a permanent cleanup failure.
      if (!closeOwn()) throw Error('inbox_access_check_cleanup_required');
    }
    async function settle() {
      if (pendingArchive) {
        const pending = pendingArchive;
        // Authentic admitted outcomes must reach the historical append owner
        // even after Stop/head/session loss. No new traffic is authorized here.
        const saved = await commitInboxProbeArchiveAttempt(
          repository,
          pending.result
        );
        if (!applied(saved.status)) return saved.status;
        if (pending.result.status === 'accepted')
          archive.set(pending.origin, pending.result);
        pendingArchive = undefined;
      }
      if (pendingRead) {
        const pending = pendingRead;
        const saved = await commitInboxProbeReadbackAttempt(
          repository,
          pending
        );
        if (!applied(saved.status)) return saved.status;
        read = pending;
        pendingRead = undefined;
      }
      return 'saved';
    }
    function selectReview(selection: Selection) {
      if (selectedReview) reviews.delete(selectedReview);
      const review = Object.freeze({}) as InboxAccessCheckReview;
      reviews.set(review, { control, selection });
      selectedReview = review;
      return review;
    }
    function retryNonceRead(status: string) {
      if (status !== 'busy') return;
      // A used reader permanently owns its actual read observation. An explicit
      // Retry must spend a later real READ under the SAME publication action,
      // rather than recapture from that observation or reset any attempt meter.
      if (nonceReader) stopInboxProbeNonceReader(nonceReader);
      if (recoveredNonceReader)
        stopRecoveredInboxProbeNonceReader(recoveredNonceReader);
      nonceReader = undefined;
      recoveredNonceReader = undefined;
      read = undefined;
    }
    const control: Control = {
      repository,
      identity,
      session,
      policy,
      operator,
      current,
      qualification,
      stop,
      snapshot() {
        const fresh = current();
        return {
          state: !fresh
            ? 'stopped'
            : qualification()
              ? 'qualified'
              : state === 'qualified'
                ? 'needs_action'
                : state,
          busy,
          cleanupRequired,
          pendingSettlement: !!pendingArchive || !!pendingRead,
          sourceOnly: true,
          mode,
          command,
          reason
        };
      },
      select(selectedMode, origin, selectedCommand) {
        if (
          !current() ||
          busy ||
          cleanupRequired ||
          (selectedMode !== 'original' && selectedMode !== 'recover') ||
          typeof origin !== 'string' ||
          !source.readTargets.includes(origin) ||
          (readOrigin !== undefined && readOrigin !== origin) ||
          (mode && mode !== selectedMode)
        )
          return undefined;
        const id =
          selectedMode === 'recover'
            ? canonicalLocalId(selectedCommand)
            : undefined;
        if (
          (selectedMode === 'recover' && !id) ||
          (selectedMode === 'original' && selectedCommand !== undefined) ||
          (command && selectedMode === 'recover' && id !== command)
        )
          return undefined;
        return selectReview({
          kind: 'check',
          mode: selectedMode,
          origin,
          command: id
        });
      },
      reviewCurrent(selection) {
        if (busy) return false;
        // Historical append is allowed only for an authentic already-admitted
        // pending result. It neither restores live authority nor issues proof.
        if (selection.kind === 'reconcile')
          return !!pendingArchive || !!pendingRead;
        return current() && !cleanupRequired;
      },
      selectAction(kind) {
        const selection = { kind };
        return control.reviewCurrent(selection)
          ? selectReview(selection)
          : undefined;
      },
      async reconcile(consent) {
        if (consent !== 'reviewed_inbox_access_check_reconciliation')
          return { status: 'invalid' };
        if (busy) return { status: 'busy' };
        busy = true;
        try {
          const status = await settle();
          observeCleanup();
          return { status };
        } catch {
          return { status: 'unknown_completion' };
        } finally {
          busy = false;
        }
      },
      async run(selection) {
        if (!current()) return { status: 'stopped' };
        if (busy || cleanupRequired) return { status: 'busy' };
        if (qualification()) return { status: 'qualified' };
        if (mode && mode !== selection.mode) return { status: 'invalid' };
        if (readOrigin !== undefined && readOrigin !== selection.origin)
          return { status: 'invalid' };
        mode ??= selection.mode;
        readOrigin ??= selection.origin;
        busy = true;
        state = 'working';
        try {
          const settled = await settle();
          if (!current()) return result('stopped');
          if (!applied(settled)) return result(settled);
          if (!acknowledgement) {
            if (mode === 'original') {
              if (!original) {
                const reserved = await reserveInboxProbeIdentity(
                  repository,
                  session,
                  policy,
                  access,
                  operator,
                  'reviewed_inbox_probe_reservation'
                );
                if ('identity' in reserved) {
                  original = reserved.identity;
                  command = inboxProbeSnapshot(original)?.id;
                }
                if (!current()) return result('stopped');
                if (!original) return result(reserved.status);
              }
              if (!current() || !original) return result('stopped');
              if (!wrap) {
                seal ??= captureInboxProbeSealOperation(
                  session,
                  original,
                  'reviewed_inbox_probe_seal'
                );
                if (!seal) return result('unavailable');
                const sealed = await buildInboxProbeSeal(seal);
                if (!current()) return result('stopped');
                if (!('seal' in sealed)) return result(sealed.status);
                wrap = buildInboxProbeGiftwrap(
                  sealed.seal,
                  'reviewed_inbox_probe_wrap'
                );
                if (!wrap || !current()) return result('stopped');
              }
              const committed = await commitInboxProbe(
                repository,
                session,
                wrap,
                'reviewed_inbox_probe_commit'
              );
              if ('acknowledgement' in committed)
                acknowledgement = committed.acknowledgement;
              if (!current()) return result('stopped');
              if (!acknowledgement) return result(committed.status);
            } else {
              command ??= selection.command;
              if (!recovery)
                recovery = captureInboxProbeRecoveryReader(
                  repository,
                  identity,
                  session,
                  policy,
                  access,
                  operator,
                  command,
                  'reviewed_inbox_probe_recovery'
                );
              if (!recovery) return result('unavailable');
              if (!recovered) {
                const found = await recoverInboxProbe(recovery);
                if ('proof' in found) recovered = found.proof;
                if (!current()) return result('stopped');
                if (!recovered) return result(found.status);
              }
              const adopted = await adoptRecoveredInboxProbe(
                repository,
                session,
                access,
                policy,
                operator,
                recovered,
                'reviewed_original_inbox_probe_retry'
              );
              if ('acknowledgement' in adopted)
                acknowledgement = adopted.acknowledgement;
              if (!current()) return result('stopped');
              if (!acknowledgement) return result(adopted.status);
            }
          }
          if (!current()) return result('stopped');
          if (!action)
            action =
              mode === 'original'
                ? captureInboxProbePublicationAction(
                    repository,
                    session,
                    policy,
                    acknowledgement,
                    'reviewed_inbox_probe_network'
                  )
                : await captureRecoveredInboxProbeRetryAction(
                    repository,
                    session,
                    policy,
                    acknowledgement,
                    'reviewed_original_inbox_probe_retry'
                  );
          if (!current()) return result('stopped');
          if (!action) return result('unavailable');
          const targets = new Map<string, true>();
          for (const origin of [
            ...source.readTargets,
            ...source.archiveTargets
          ])
            targets.set(origin, true);
          pool ??= getPrivatePool(
            session,
            policy,
            Array.from(targets.keys()),
            token,
            'access',
            current
          );
          if (!pool || !current()) return result('stopped');
          for (const origin of source.archiveTargets) {
            if (!current()) return result('stopped');
            if (archive.has(origin)) continue;
            const permission = captureInboxProbePublication(
              repository,
              session,
              policy,
              acknowledgement,
              action,
              origin,
              'reviewed_inbox_probe_publication'
            );
            if (!permission) return result('unavailable');
            const attempt = await publishInboxProbeArchiveAttempt(
              pool,
              permission,
              cancellation.signal
            );
            if (inboxProbeArchiveAttemptEvidence(attempt))
              pendingArchive = { origin, result: attempt };
            const saved = await settle();
            if (!current()) return result('stopped');
            if (!applied(saved)) return result(saved);
            if (attempt.status !== 'accepted' || !archive.has(origin))
              return result(attempt.status);
          }
          if (!read) {
            if (!current()) return result('stopped');
            const permission = captureInboxProbeReadback(
              repository,
              session,
              policy,
              acknowledgement,
              action,
              selection.origin,
              'reviewed_inbox_probe_readback'
            );
            if (!permission) return result('unavailable');
            const attempt = await readInboxProbeAttempt(
              pool,
              permission,
              cancellation.signal
            );
            if (inboxProbeReadbackAttemptEvidence(attempt))
              pendingRead = attempt;
            const saved = await settle();
            if (!current()) return result('stopped');
            if (!applied(saved)) return result(saved);
            if (attempt.status !== 'readback' || !read)
              return result(attempt.status);
          }
          if (!nonceProof) {
            if (!current()) return result('stopped');
            if (mode === 'original') {
              nonceReader ??= captureInboxProbeNonceReader(
                repository,
                identity,
                session,
                policy,
                read,
                'reviewed_inbox_probe_nonce_decryption'
              );
              if (!nonceReader) return result('unavailable');
              const decrypted = await readInboxProbeNonce(nonceReader);
              if ('proof' in decrypted) nonceProof = decrypted.proof;
              if (!current()) return result('stopped');
              if (!nonceProof) {
                retryNonceRead(decrypted.status);
                return result(decrypted.status);
              }
            } else {
              recoveredNonceReader ??= captureRecoveredInboxProbeNonceReader(
                repository,
                identity,
                session,
                policy,
                read,
                'reviewed_inbox_probe_nonce_decryption'
              );
              if (!recoveredNonceReader) return result('unavailable');
              const decrypted =
                await readRecoveredInboxProbeNonce(recoveredNonceReader);
              if ('proof' in decrypted) nonceProof = decrypted.proof;
              if (!current()) return result('stopped');
              if (!nonceProof) {
                retryNonceRead(decrypted.status);
                return result(decrypted.status);
              }
            }
          }
          if (!nonce) {
            if (!current()) return result('stopped');
            const saved = await commitInboxProbeNonceVerification(
              repository,
              session,
              policy,
              nonceProof
            );
            if (saved.acknowledgement) nonce = saved.acknowledgement;
            if (!current()) return result('stopped');
            if (!nonce) return result(saved.status);
          }
          const issued = await qualifyInboxAccess(
            repository,
            session,
            policy,
            access,
            operator,
            nonce,
            [...archive.values(), read],
            'reviewed_qualify_inbox_access'
          );
          if (!current()) {
            if (issued.qualification)
              stopInboxAccessQualification(issued.qualification);
            return result('stopped');
          }
          if (issued.qualification) qualified = issued.qualification;
          return result(issued.status);
        } catch {
          return result(current() ? 'unknown_completion' : 'stopped');
        } finally {
          busy = false;
          if (stopped) observeCleanup();
        }
      },
      async cleanup(consent) {
        if (consent !== 'reviewed_delete_local_inbox_probe')
          return { status: 'invalid' };
        if (!current()) return { status: 'stopped' };
        if (busy || cleanupRequired) return { status: 'busy' };
        if (removal) return { status: 'removed' };
        if (!nonce || !read) return { status: 'unavailable' };
        busy = true;
        try {
          cleanup = await captureInboxProbeCleanup(
            repository,
            session,
            policy,
            nonce,
            [...archive.values(), read],
            'reviewed_inbox_probe_cleanup_selection'
          );
          if (!cleanup || !current()) return result('unavailable');
          const deleted = await deleteReviewedInboxProbe(cleanup, consent);
          // A real claim invalidates the prior journal proof even on abort.
          if (qualified && !inboxAccessQualificationSnapshot(qualified))
            qualified = undefined;
          if (deleted.acknowledgement) removal = deleted.acknowledgement;
          if (!current()) return result('stopped');
          if (!removal) return result(deleted.status);
          const issued = qualifyRemovedInboxAccess(
            repository,
            session,
            policy,
            access,
            operator,
            removal,
            'reviewed_qualify_inbox_access'
          );
          if (issued.qualification) qualified = issued.qualification;
          return result(
            issued.status === 'qualified' ? 'removed' : issued.status
          );
        } catch {
          return result('unknown_completion');
        } finally {
          busy = false;
        }
      }
    };
    controls.set(token, control);
    currentChecks.set(access, token);
    listeners.set(
      Symbol(),
      subscribeInboxAccessControllerClose(access, sourceClosed)
    );
    listeners.set(
      Symbol(),
      subscribePrivateSessionClose(session, sourceClosed)
    );
    if (!current()) {
      stop();
      return undefined;
    }
    return token;
  } catch {
    return undefined;
  }
}

export function inboxAccessCheckSnapshot(token: InboxAccessCheck) {
  return controls.get(token)?.snapshot();
}
export function inboxAccessCheckQualification(token: InboxAccessCheck) {
  return controls.get(token)?.qualification();
}
export function reviewInboxAccessCheck(
  token: InboxAccessCheck,
  mode: unknown,
  origin: unknown,
  command?: unknown
) {
  return controls.get(token)?.select(mode, origin, command);
}
export function inboxAccessCheckReviewSnapshot(token: InboxAccessCheckReview) {
  const saved = reviews.get(token);
  return saved?.control.reviewCurrent(saved.selection)
    ? { ...saved.selection, disclosures: disclosures.slice(), sourceOnly: true }
    : undefined;
}
export function runInboxAccessCheck(
  token: InboxAccessCheckReview,
  review: unknown
): Promise<InboxAccessCheckResult> {
  const saved = reviews.get(token);
  if (
    !saved ||
    saved.selection.kind !== 'check' ||
    review !== 'reviewed_inbox_access_check'
  )
    return Promise.resolve({ status: 'invalid' });
  reviews.delete(token);
  return saved.control.run(saved.selection);
}
export function cleanupInboxAccessCheck(
  token: InboxAccessCheckReview,
  review: unknown
): Promise<InboxAccessCheckResult> {
  const saved = reviews.get(token);
  if (
    !saved ||
    saved.selection.kind !== 'cleanup' ||
    review !== 'reviewed_delete_local_inbox_probe'
  )
    return Promise.resolve({ status: 'invalid' });
  reviews.delete(token);
  return saved.control.cleanup(review);
}
export function reviewInboxAccessCheckCleanup(token: InboxAccessCheck) {
  return controls.get(token)?.selectAction('cleanup');
}
export function reviewInboxAccessCheckReconciliation(token: InboxAccessCheck) {
  return controls.get(token)?.selectAction('reconcile');
}
export function reconcileInboxAccessCheck(
  token: InboxAccessCheckReview,
  review: unknown
): Promise<InboxAccessCheckResult> {
  const saved = reviews.get(token);
  if (
    !saved ||
    saved.selection.kind !== 'reconcile' ||
    review !== 'reviewed_inbox_access_check_reconciliation'
  )
    return Promise.resolve({ status: 'invalid' });
  reviews.delete(token);
  return saved.control.reconcile(review);
}
export function stopInboxAccessCheck(token: InboxAccessCheck): boolean {
  return controls.get(token)?.stop() ?? false;
}
export function retryInboxAccessCheckCleanup(
  token: InboxAccessCheck,
  review: unknown
): boolean {
  return (
    review === 'reviewed_inbox_access_check_cleanup_retry' &&
    (controls.get(token)?.stop() ?? false)
  );
}
