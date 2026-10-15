import { makeCleanupFixture } from './inbox-probe-cleanup.ts';
import {
  validateInboxOperatorQualification,
  inboxOperatorQualificationSnapshot
} from '../../../src/lib/config/inbox-operators.ts';
import {
  captureInboxProbeNonceReader,
  captureRecoveredInboxProbeNonceReader,
  readInboxProbeNonce,
  readRecoveredInboxProbeNonce
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import {
  commitInboxProbeNonceVerification,
  type InboxProbeNonceAcknowledgement
} from '../../../src/lib/persistence/inbox-probes.ts';
import {
  qualifyInboxAccess,
  qualifyRemovedInboxAccess,
  observeQualifiedInboxAccess,
  inboxAccessQualificationSnapshot,
  type InboxAccessQualification
} from '../../../src/lib/messaging/inbox-access-qualification.ts';
import { inboxAccessConstructionBinding } from '../../../src/lib/messaging/inbox-access-controller.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import { captureUnlockedSession } from '../../../src/lib/messaging/unlocked-session.ts';
import {
  captureInboxSync,
  startInboxSync,
  stopInboxSync,
  inboxSyncSnapshot,
  type InboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
export {
  validateInboxOperatorQualification,
  inboxOperatorQualificationWire
} from '../../../src/lib/config/inbox-operators.ts';
export {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
export * from './inbox-probe-cleanup.ts';
export * from '../../../src/lib/messaging/inbox-access-qualification.ts';
export {
  captureInboxSync,
  inboxSyncSnapshot,
  startInboxSync,
  stopInboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
export { captureUnlockedSession } from '../../../src/lib/messaging/unlocked-session.ts';
export { closePrivateSession } from '../../../src/lib/runtime/private-session.ts';
export async function makeQualificationFixture(authRequired = false) {
  const base = await makeCleanupFixture();
  if (authRequired) {
    const wire = inboxOperatorQualificationSnapshot(base.host.qualification)!;
    const operator = validateInboxOperatorQualification(
      JSON.stringify({
        ...wire,
        relays: wire.relays.map((relay) =>
          relay.origin === 'wss://inbox.example.org'
            ? { ...relay, auth: 'explicit_required' }
            : relay
        )
      })
    );
    if (!operator)
      throw Error('actual explicitly required operator declaration');
    base.host.qualification = operator;
  }
  let qualification: InboxAccessQualification | undefined;
  let latestNonce: InboxProbeNonceAcknowledgement | undefined;
  let latestObservations: ReturnType<typeof base.observations> | undefined;
  let sync: InboxSync | undefined;
  const views: ReturnType<typeof createPublicView>[] = [];
  return {
    ...base,
    qualification: () => qualification,
    async renewQualificationNonce(recovered: boolean, origin: string) {
      const observation = await (recovered
        ? base.read(origin)
        : base.originalRead(origin));
      if (!observation || observation.status !== 'readback')
        throw Error('actual later readback');
      if ((await base.appendRead(observation)).status !== 'saved')
        throw Error('actual later read append');
      const a = base.actors();
      const verified = recovered
        ? await readRecoveredInboxProbeNonce(
            captureRecoveredInboxProbeNonceReader(
              base.repository,
              a.identity,
              a.session,
              base.host.policy,
              observation,
              'reviewed_inbox_probe_nonce_decryption'
            )!
          )
        : await readInboxProbeNonce(
            captureInboxProbeNonceReader(
              base.repository,
              a.identity,
              a.session,
              base.host.policy,
              observation,
              'reviewed_inbox_probe_nonce_decryption'
            )!
          );
      if (!('proof' in verified)) throw Error('actual later nonce proof');
      const saved = await commitInboxProbeNonceVerification(
        base.repository,
        a.session,
        base.host.policy,
        verified.proof
      );
      if (!saved.acknowledgement) throw Error('actual later nonce native fact');
      latestNonce = saved.acknowledgement;
      latestObservations = [...base.observations(), observation];
      return saved.status;
    },
    async startOwnInbox() {
      if (!qualification) throw Error('actual qualification missing');
      const runtime = base.sharedPublicRuntime();
      const a = base.actors(),
        view = createPublicView(runtime),
        resolver = resolveInboxPreference(
          view,
          beginPublicViewRun(view),
          base.host.owner
        );
      views.push(view);
      await settleInboxResolver(resolver);
      const unlocked = captureUnlockedSession(
        a.session,
        'reviewed_messages_unlock'
      );
      if (!unlocked) throw Error('actual unlocked scope');
      sync = captureInboxSync(
        base.repository,
        a.session,
        base.host.policy,
        resolver,
        unlocked,
        observeQualifiedInboxAccess(qualification),
        'reviewed_foreground_inbox'
      );
      return !!sync && startInboxSync(sync, 'reviewed_foreground_inbox');
    },
    syncSnapshot: () => sync && inboxSyncSnapshot(sync),
    stopOwnInbox() {
      if (sync) stopInboxSync(sync);
    },
    close() {
      if (sync) stopInboxSync(sync);
      for (const view of views) disposePublicView(view);
      base.close();
    },
    context() {
      const a = base.actors(),
        scope = inboxAccessConstructionBinding(
          a.access,
          a.session,
          base.host.policy
        );
      if (!scope) throw Error('actual C7 scope missing');
      return {
        owner: scope.owner,
        session: scope.session,
        preferenceId: scope.preferenceId,
        readTargets: scope.readTargets.slice(),
        archiveTargets: scope.archiveTargets.slice()
      };
    },
    async qualifyAccess(
      observations = latestObservations ?? base.observations()
    ) {
      const a = base.actors(),
        result = await qualifyInboxAccess(
          base.repository,
          a.session,
          base.host.policy,
          a.access,
          base.host.qualification,
          latestNonce ?? base.nonce()!,
          observations,
          'reviewed_qualify_inbox_access'
        );
      if (result.qualification) qualification = result.qualification;
      return result;
    },
    qualifyRemoved() {
      const a = base.actors(),
        result = qualifyRemovedInboxAccess(
          base.repository,
          a.session,
          base.host.policy,
          a.access,
          base.host.qualification,
          base.removal()!,
          'reviewed_qualify_inbox_access'
        );
      if (result.qualification) qualification = result.qualification;
      return result;
    },
    snapshot: () =>
      qualification && inboxAccessQualificationSnapshot(qualification),
    observation(
      context: Parameters<ReturnType<typeof observeQualifiedInboxAccess>>[0]
    ) {
      return (
        qualification && observeQualifiedInboxAccess(qualification)(context)
      );
    }
  };
}
