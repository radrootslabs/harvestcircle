import {
  makeRecoveryFixture,
  openRecoveryContext
} from './inbox-probe-recovery.ts';
import { RelayPool } from 'applesauce-relay';
import { finalize } from 'rxjs';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver,
  inboxResolutionSnapshot
} from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxAccessController,
  closeInboxAccessController,
  inboxAccessControllerSnapshot,
  type InboxAccessController
} from '../../../src/lib/messaging/inbox-access-controller.ts';
import { privateSessionSnapshot } from '../../../src/lib/runtime/private-session.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  reserveInboxProbeIdentity,
  inboxProbeSnapshot,
  type InboxProbeIdentity
} from '../../../src/lib/nostr/inbox-access.ts';
import {
  captureInboxProbeSealOperation,
  buildInboxProbeSeal
} from '../../../src/lib/nostr/seal-builder.ts';
import { buildInboxProbeGiftwrap } from '../../../src/lib/nostr/giftwrap-builder.ts';
import {
  commitInboxProbe,
  adoptRecoveredInboxProbe,
  inboxProbeAcknowledgementSnapshot,
  inboxProbePublicationBinding,
  recoveredInboxProbePublicationBinding,
  commitInboxProbeArchiveAttempt,
  commitInboxProbeReadbackAttempt,
  type InboxProbeAcknowledgement
} from '../../../src/lib/persistence/inbox-probes.ts';
import {
  captureInboxProbeRecoveryReader,
  recoverInboxProbe,
  stopInboxProbeRecoveryReader,
  type InboxProbeRecoveryReader,
  type RecoveredInboxProbe
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import {
  captureInboxProbePublicationAction,
  captureRecoveredInboxProbeRetryAction,
  captureInboxProbePublication,
  captureInboxProbeReadback,
  type InboxProbePublicationAction
} from '../../../src/lib/nostr/private-publisher.ts';
import {
  getPrivatePool,
  closePrivatePool,
  privatePoolProbeRetryIdle,
  publishInboxProbeArchiveAttempt,
  readInboxProbeAttempt,
  type PrivatePool,
  type InboxProbeArchiveAttemptResult,
  type InboxProbeReadbackAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
export * from './inbox-probe-recovery.ts';
export {
  makeFixture as makeOrdinaryRecoveryFixture,
  resumeEncryptedPreparation,
  resumeRecoveredRumor
} from './resume-preparation.ts';
export {
  adoptRecoveredInboxProbe,
  captureRecoveredInboxProbeRetryAction,
  recoveredInboxProbePublicationBinding,
  inboxProbePublicationBinding,
  privatePoolProbeRetryIdle,
  commitInboxProbe,
  captureInboxProbePublicationAction,
  captureInboxProbePublication,
  captureInboxProbeReadback
};
export {
  authenticateInboxAccess,
  respondInboxAccessAuthentication,
  inboxAccessControllerSnapshot
} from '../../../src/lib/messaging/inbox-access-controller.ts';
export {
  closePrivateSession,
  subscribePrivateSessionClose,
  withIdlePrivateLocalStorage
} from '../../../src/lib/runtime/private-session.ts';

// Every actor and durable effect below is an actual source factory. The reused
// host owns only ephemeral test crypto/native fixture access, never readiness.
export async function makeRetryFixture() {
  const host = await makeRecoveryFixture(),
    publicContext = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(publicContext, host.policy);
  if (!runtime) throw Error('actual shared public discovery');
  const shared = { context: publicContext, runtime };
  let actor: Awaited<ReturnType<typeof openRecoveryContext>> | undefined,
    access: InboxAccessController | undefined,
    replacementView: ReturnType<typeof createPublicView> | undefined,
    originalIdentity: InboxProbeIdentity | undefined,
    originalAck: InboxProbeAcknowledgement | undefined,
    originalAction: InboxProbePublicationAction | undefined,
    reader: InboxProbeRecoveryReader | undefined,
    proof: RecoveredInboxProbe | undefined,
    acknowledgement: InboxProbeAcknowledgement | undefined,
    action: InboxProbePublicationAction | undefined,
    command: string | undefined,
    pool: PrivatePool | undefined;
  const repository = host.repository;
  function currentActor() {
    if (!actor || !access) throw Error('actual current private actors missing');
    return { ...actor, access };
  }
  function acquire() {
    const a = currentActor();
    pool ??= getPrivatePool(
      a.session,
      host.policy,
      [
        'wss://inbox.example.org',
        'wss://peer.example.org',
        'wss://archive.example.org'
      ],
      access,
      'access'
    );
    if (!pool) throw Error('actual current private scope unavailable');
    return pool;
  }
  function stopCurrentController() {
    if (reader) stopInboxProbeRecoveryReader(reader);
    if (access) closeInboxAccessController(access);
    if (pool) closePrivatePool(pool);
    pool = undefined;
    if (replacementView) disposePublicView(replacementView);
    replacementView = undefined;
    reader = undefined;
    proof = undefined;
    acknowledgement = undefined;
    action = undefined;
  }
  return {
    host,
    repository,
    actors: currentActor,
    originalIdentity: () => originalIdentity,
    originalAck: () => originalAck,
    proof: () => proof,
    acknowledgement: () => acknowledgement,
    action: () => action,
    command: () => command,
    pool: () => pool,
    async open() {
      actor = await openRecoveryContext(
        host.owner,
        host.policy,
        host.qualification,
        crypto.randomUUID(),
        shared
      );
      access = actor.access;
    },
    async prepareOriginal() {
      if (reader) stopInboxProbeRecoveryReader(reader);
      reader = undefined;
      proof = undefined;
      acknowledgement = undefined;
      action = undefined;
      const a = currentActor(),
        reserved = await reserveInboxProbeIdentity(
          repository,
          a.session,
          host.policy,
          a.access,
          host.qualification,
          'reviewed_inbox_probe_reservation'
        );
      if (!('identity' in reserved)) return reserved;
      originalIdentity = reserved.identity;
      const selected = inboxProbeSnapshot(originalIdentity);
      if (!selected) throw Error('actual original reserved identity');
      command = selected.id;
      const operation = captureInboxProbeSealOperation(
        a.session,
        originalIdentity,
        'reviewed_inbox_probe_seal'
      );
      if (!operation) throw Error('actual original seal admission');
      const sealed = await buildInboxProbeSeal(operation);
      if (!('seal' in sealed)) return sealed;
      const wrap = buildInboxProbeGiftwrap(
        sealed.seal,
        'reviewed_inbox_probe_wrap'
      );
      if (!wrap) throw Error('actual original wrap admission');
      const result = await commitInboxProbe(
        repository,
        a.session,
        wrap,
        'reviewed_inbox_probe_commit'
      );
      if ('acknowledgement' in result) originalAck = result.acknowledgement;
      originalAction = undefined;
      return result;
    },
    async recover() {
      const a = currentActor();
      if (!command) throw Error('actual original selection missing');
      reader ??= captureInboxProbeRecoveryReader(
        repository,
        a.identity,
        a.session,
        host.policy,
        a.access,
        host.qualification,
        command,
        'reviewed_inbox_probe_recovery'
      );
      if (!reader) return undefined;
      const result = await recoverInboxProbe(reader);
      if ('proof' in result) proof = result.proof;
      return result;
    },
    async adopt(review: unknown = 'reviewed_original_inbox_probe_retry') {
      const a = currentActor();
      if (!proof) throw Error('actual recovered proof missing');
      const result = await adoptRecoveredInboxProbe(
        repository,
        a.session,
        a.access,
        host.policy,
        host.qualification,
        proof,
        review
      );
      if ('acknowledgement' in result) acknowledgement = result.acknowledgement;
      return result;
    },
    async captureAction(
      review: unknown = 'reviewed_original_inbox_probe_retry'
    ) {
      const a = currentActor();
      if (!acknowledgement)
        throw Error('actual recovered acknowledgement missing');
      const selected = await captureRecoveredInboxProbeRetryAction(
        repository,
        a.session,
        host.policy,
        acknowledgement,
        review
      );
      if (selected) action = selected;
      return selected;
    },
    async originalPublish(
      origin = 'wss://inbox.example.org',
      signal = new AbortController().signal
    ) {
      const a = currentActor();
      if (!originalAck) throw Error('actual original acknowledgement missing');
      originalAction ??= captureInboxProbePublicationAction(
        repository,
        a.session,
        host.policy,
        originalAck,
        'reviewed_inbox_probe_network'
      );
      const permission =
        originalAction &&
        captureInboxProbePublication(
          repository,
          a.session,
          host.policy,
          originalAck,
          originalAction,
          origin,
          'reviewed_inbox_probe_publication'
        );
      return permission
        ? publishInboxProbeArchiveAttempt(acquire(), permission, signal)
        : undefined;
    },
    async originalRead(origin = 'wss://inbox.example.org') {
      const a = currentActor();
      if (!originalAck || !originalAction) return undefined;
      const permission = captureInboxProbeReadback(
        repository,
        a.session,
        host.policy,
        originalAck,
        originalAction,
        origin,
        'reviewed_inbox_probe_readback'
      );
      return permission
        ? readInboxProbeAttempt(
            acquire(),
            permission,
            new AbortController().signal
          )
        : undefined;
    },
    async publish(
      origin = 'wss://inbox.example.org',
      signal = new AbortController().signal
    ) {
      const a = currentActor();
      if (!acknowledgement || !action) return undefined;
      const permission = captureInboxProbePublication(
        repository,
        a.session,
        host.policy,
        acknowledgement,
        action,
        origin,
        'reviewed_inbox_probe_publication'
      );
      return permission
        ? publishInboxProbeArchiveAttempt(acquire(), permission, signal)
        : undefined;
    },
    async read(
      origin = 'wss://inbox.example.org',
      signal = new AbortController().signal
    ) {
      const a = currentActor();
      if (!acknowledgement || !action) return undefined;
      const permission = captureInboxProbeReadback(
        repository,
        a.session,
        host.policy,
        acknowledgement,
        action,
        origin,
        'reviewed_inbox_probe_readback'
      );
      return permission
        ? readInboxProbeAttempt(acquire(), permission, signal)
        : undefined;
    },
    append: (result: InboxProbeArchiveAttemptResult) =>
      commitInboxProbeArchiveAttempt(repository, result),
    appendRead: (result: InboxProbeReadbackAttemptResult) =>
      commitInboxProbeReadbackAttempt(repository, result),
    snapshot: () =>
      acknowledgement && inboxProbeAcknowledgementSnapshot(acknowledgement),
    originalBinding: () =>
      acknowledgement &&
      inboxProbePublicationBinding(
        repository,
        currentActor().session,
        acknowledgement,
        host.policy
      ),
    recoveredBinding: () =>
      acknowledgement &&
      recoveredInboxProbePublicationBinding(
        repository,
        currentActor().session,
        acknowledgement,
        host.policy
      ),
    idle: () => privatePoolProbeRetryIdle(currentActor().session, host.policy),
    installNetworkClock() {
      const original = performance.now.bind(performance);
      let now = original(),
        observations = 0;
      performance.now = () => {
        if (/at clock(?:\d+)? \(/.test(new Error().stack ?? '')) {
          observations++;
          return now;
        }
        return original();
      };
      return {
        advance: (value: number) => {
          now += value;
        },
        observations: () => observations,
        restore: () => {
          performance.now = original;
        }
      };
    },
    installAttemptTeardownFailure() {
      const original = Object.getOwnPropertyDescriptor(
          RelayPool.prototype,
          'relay'
        )!.value as RelayPool['relay'],
        restorations: (() => void)[] = [];
      let failing = true,
        faults = 0;
      RelayPool.prototype.relay = function (
        ...args: Parameters<RelayPool['relay']>
      ) {
        const relay = original.apply(this, args),
          event = relay.event.bind(relay);
        relay.event = (...args: Parameters<typeof relay.event>) =>
          event(...args).pipe(
            finalize(() => {
              if (failing) {
                faults++;
                throw Error('actual controlled SDK teardown failure');
              }
            })
          );
        restorations.push(() => {
          relay.event = event;
        });
        return relay;
      };
      return {
        faults: () => faults,
        restore: () => {
          failing = false;
          RelayPool.prototype.relay = original;
          for (const restore of restorations) restore();
        }
      };
    },
    installCommitFault(mode: 'aborted' | 'uncertain' | 'changed_successor') {
      const put = Object.getOwnPropertyDescriptor(
          IDBObjectStore.prototype,
          'put'
        )!.value as IDBObjectStore['put'],
        abort = Object.getOwnPropertyDescriptor(
          IDBTransaction.prototype,
          'abort'
        )!.value as IDBTransaction['abort'],
        transaction = Object.getOwnPropertyDescriptor(
          IDBDatabase.prototype,
          'transaction'
        )!.value as IDBDatabase['transaction'];
      let selected: IDBTransaction | undefined,
        putSuccess = 0,
        complete = 0,
        aborted = 0,
        abortRefused = 0,
        changed = 0;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        // Queue a genuine competing write BEFORE the first final readonly
        // transaction. A later complete listener can run after its admission.
        if (
          mode === 'changed_successor' &&
          selected?.db === this &&
          !changed &&
          args[1] === 'readonly'
        ) {
          changed++;
          const next = transaction.call(this, ['private_sends'], 'readwrite'),
            store = next.objectStore('private_sends'),
            row = store.get([host.owner, command!]);
          row.addEventListener('success', () => {
            const current = row.result as {
                owner: string;
                id: string;
                wire: string;
              },
              record = JSON.parse(current.wire) as { revision: number };
            record.revision++;
            put.call(store, { ...current, wire: JSON.stringify(record) });
          });
        }
        return transaction.apply(this, args);
      };
      IDBTransaction.prototype.abort = function (
        ...args: Parameters<IDBTransaction['abort']>
      ) {
        if (this === selected && mode !== 'aborted') {
          abortRefused++;
          throw Error('actual controlled abort refusal');
        }
        return abort.apply(this, args);
      };
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        const request = put.apply(this, args);
        if (!selected && this.name === 'private_sends') {
          selected = this.transaction;
          selected.addEventListener('complete', () => {
            complete++;
          });
          selected.addEventListener('abort', () => {
            aborted++;
          });
          request.addEventListener('success', () => {
            putSuccess++;
            if (mode === 'aborted') selected!.abort();
          });
          if (mode !== 'aborted')
            throw Error('actual controlled failure after native put admission');
        }
        return request;
      };
      return {
        counts: () => ({
          putSuccess,
          complete,
          aborted,
          abortRefused,
          changed
        }),
        restore: () => {
          IDBObjectStore.prototype.put = put;
          IDBTransaction.prototype.abort = abort;
          IDBDatabase.prototype.transaction = transaction;
        }
      };
    },
    async alternateController(foreignSession = false) {
      if (foreignSession) {
        const other = await openRecoveryContext(
          host.owner,
          host.policy,
          host.qualification,
          command!,
          shared
        );
        return {
          access: other.access,
          session: other.session,
          close: () => other.close()
        };
      }
      const view = createPublicView(runtime),
        run = beginPublicViewRun(view),
        resolver = resolveInboxPreference(view, run, host.owner);
      await settleInboxResolver(resolver);
      const other = createInboxAccessController(
        currentActor().session,
        host.policy,
        resolver
      );
      if (!other) throw Error('actual alternate controller');
      return {
        access: other,
        session: currentActor().session,
        close() {
          if (other !== access) closeInboxAccessController(other);
          disposePublicView(view);
        }
      };
    },
    async replacementWhilePending() {
      const a = currentActor(),
        view = createPublicView(runtime),
        run = beginPublicViewRun(view),
        resolver = resolveInboxPreference(view, run, host.owner);
      await settleInboxResolver(resolver);
      if (inboxResolutionSnapshot(resolver).status !== 'ready')
        throw Error('replacement must have actual ready discovery metadata');
      let other: InboxAccessController | undefined,
        recovery: InboxProbeRecoveryReader | undefined;
      try {
        other = createInboxAccessController(a.session, host.policy, resolver);
        if (!other)
          return {
            gate: 'actual_controller_factory' as const,
            privateCurrent: privateSessionSnapshot(a.session)?.current === true,
            originalController: inboxAccessControllerSnapshot(a.access)
          };
        recovery = captureInboxProbeRecoveryReader(
          repository,
          a.identity,
          a.session,
          host.policy,
          other,
          host.qualification,
          command!,
          'reviewed_inbox_probe_recovery'
        );
        if (!recovery)
          return {
            gate: 'actual_recovery_factory' as const,
            currentController: inboxAccessControllerSnapshot(other)
          };
        const recovered = await recoverInboxProbe(recovery);
        if (!('proof' in recovered))
          return {
            gate: 'actual_recovery_execution' as const,
            status: recovered.status,
            currentController: inboxAccessControllerSnapshot(other)
          };
        const adopted = await adoptRecoveredInboxProbe(
          repository,
          a.session,
          other,
          host.policy,
          host.qualification,
          recovered.proof,
          'reviewed_original_inbox_probe_retry'
        );
        if (!('acknowledgement' in adopted))
          return {
            gate: 'actual_adoption' as const,
            status: adopted.status,
            currentController: inboxAccessControllerSnapshot(other)
          };
        const binding = recoveredInboxProbePublicationBinding(
          repository,
          a.session,
          adopted.acknowledgement,
          host.policy
        );
        if (!binding?.current())
          throw Error(
            'fresh genuine recovered binding must be current before Retry'
          );
        const before = host.counts(),
          original = crypto.randomUUID.bind(crypto),
          closesBefore = Number(Reflect.get(window, 'ptCloseCalls') ?? 0);
        let uuids = 0;
        crypto.randomUUID = () => {
          uuids++;
          return original();
        };
        try {
          const selected = await captureRecoveredInboxProbeRetryAction(
            repository,
            a.session,
            host.policy,
            adopted.acknowledgement,
            'reviewed_original_inbox_probe_retry'
          );
          return {
            gate: 'actual_retry_admission' as const,
            genuineCurrentBinding: binding.current(),
            refused: !selected,
            uuids,
            before,
            after: host.counts(),
            closesBefore,
            closesAfter: Number(Reflect.get(window, 'ptCloseCalls') ?? 0),
            currentController: inboxAccessControllerSnapshot(other)
          };
        } finally {
          crypto.randomUUID = original;
        }
      } finally {
        if (recovery) stopInboxProbeRecoveryReader(recovery);
        if (other) closeInboxAccessController(other);
        disposePublicView(view);
      }
    },
    // Test-only physical deletion frees the existing native quota to test the
    // independent source meter bound. This is never product cleanup evidence.
    async removeFixtureRow() {
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready' || !command)
        throw Error('actual fixture deletion inputs');
      try {
        const tx = browserDatabaseTransaction(
          opened.owner,
          ['private_sends'],
          'readwrite'
        );
        tx.objectStore('private_sends').delete([host.owner, command]);
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('fixture delete abort'))
          );
        });
      } finally {
        closeBrowserDatabase(opened.owner);
      }
    },
    async replaceController() {
      const a = currentActor();
      stopCurrentController();
      replacementView = createPublicView(runtime);
      const run = beginPublicViewRun(replacementView),
        resolver = resolveInboxPreference(replacementView, run, host.owner);
      await settleInboxResolver(resolver);
      access = createInboxAccessController(a.session, host.policy, resolver);
      return access;
    },
    async freshSession() {
      stopCurrentController();
      actor?.close();
      actor = await openRecoveryContext(
        host.owner,
        host.policy,
        host.qualification,
        command ?? crypto.randomUUID(),
        shared
      );
      access = actor.access;
      return actor;
    },
    stop: stopCurrentController,
    close() {
      stopCurrentController();
      actor?.close();
      closePublicRuntime(publicContext);
      host.close();
    }
  };
}
