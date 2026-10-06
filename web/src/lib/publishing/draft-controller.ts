import type {
  PublicDraftForm,
  PublicDraftRecord
} from '../contracts/local-records.ts';
import {
  capturePublicDraftCreate,
  capturePublicDraftSave,
  commitPublicDraftWrite,
  observePublicDraftWrite,
  publicDraftFormSnapshot,
  publicDraftWriteSnapshot,
  publicDraftRepositoryOwner,
  readPublicDraft,
  type PublicDraftRepository,
  type PublicDraftWrite,
  type DraftFailure
} from '../persistence/drafts.ts';

declare const controllerBrand: unique symbol;
export type PublicDraftController = Readonly<{ [controllerBrand]: true }>;
export type DraftControllerSnapshot = Readonly<{
  form: PublicDraftForm;
  editVersion: number;
  savedEditVersion: number;
  draft: PublicDraftRecord | null;
  state: 'editing' | 'saving' | 'saved' | 'failed' | 'closed';
  failure: DraftFailure | null;
}>;
type Controller = {
  edit(form: unknown): boolean;
  flush(): Promise<boolean>;
  retry(): Promise<boolean>;
  saveClose(): Promise<boolean>;
  close(): void;
  snapshot(): DraftControllerSnapshot;
};
const controllers = new WeakMap<PublicDraftController, Controller>();
export async function createPublicDraftController(
  repository: PublicDraftRepository,
  initialForm: unknown,
  existingId?: string
): Promise<PublicDraftController | undefined> {
  if (!publicDraftRepositoryOwner(repository)) return undefined;
  const initial = publicDraftFormSnapshot(initialForm);
  if (!initial || !publicDraftRepositoryOwner(repository)) return undefined;
  let saved: PublicDraftRecord | null = null;
  if (existingId !== undefined) {
    const read = await readPublicDraft(repository, existingId);
    if (!read.ok || !publicDraftRepositoryOwner(repository)) return undefined;
    saved = read.value;
  }
  let form = saved ? saved.form : initial;
  let editVersion = 0,
    savedEditVersion = saved ? 0 : -1;
  let state: DraftControllerSnapshot['state'] = saved ? 'saved' : 'editing';
  let failure: DraftFailure | null = null;
  let closed = false;
  let active: Promise<void> | undefined;
  let attempted:
    Readonly<{ capturedWrite: PublicDraftWrite; version: number }> | undefined;
  // Exactly one captured attempt survives unknown receipts. Pending form is
  // separate; acknowledging a prior attempt never replaces that current form.
  async function resolveAttempt(): Promise<boolean> {
    const current = attempted;
    if (!current || closed) return false;
    state = 'saving';
    const expected = publicDraftWriteSnapshot(
      repository,
      current.capturedWrite
    );
    if (!expected) {
      failure = 'invalid_scope';
      state = 'failed';
      return false;
    }
    const result = await commitPublicDraftWrite(
      repository,
      current.capturedWrite
    );
    if (closed) return false;
    let receipt: PublicDraftRecord | undefined;
    if (result.ok && JSON.stringify(result.value) === JSON.stringify(expected))
      receipt = result.value;
    else if (!result.ok && result.reason === 'unknown_completion') {
      const observed = await observePublicDraftWrite(
        repository,
        current.capturedWrite
      );
      if (closed) return false;
      if (
        observed.state === 'committed' &&
        JSON.stringify(observed.value) === JSON.stringify(expected)
      )
        receipt = observed.value;
      else
        failure =
          observed.state === 'conflict'
            ? 'conflict'
            : observed.state === 'unavailable'
              ? observed.reason
              : 'unknown_completion';
    } else failure = result.ok ? 'unknown_completion' : result.reason;
    if (!receipt) {
      state = 'failed';
      return false;
    }
    saved = receipt;
    savedEditVersion = current.version;
    attempted = undefined;
    failure = null;
    state = savedEditVersion === editVersion ? 'saved' : 'editing';
    return true;
  }
  async function pump(): Promise<void> {
    while (!closed && failure === null && savedEditVersion !== editVersion) {
      if (!attempted) {
        const capturedVersion = editVersion;
        const capturedForm = form;
        const captured = saved
          ? capturePublicDraftSave(
              repository,
              saved.id,
              saved.revision,
              capturedForm
            )
          : capturePublicDraftCreate(repository, capturedForm);
        if (closed) return;
        if (!captured.ok) {
          failure = captured.reason;
          state = 'failed';
          return;
        }
        attempted = { capturedWrite: captured.value, version: capturedVersion };
      }
      if (!(await resolveAttempt())) return;
    }
  }
  function start(): Promise<void> {
    if (active) return active;
    // Queue one owned worker so synchronous edits coalesce before capture.
    const work = Promise.resolve()
      .then(pump)
      .catch(() => {
        if (!closed) {
          failure = 'unknown_completion';
          state = 'failed';
        }
      })
      .finally(() => {
        active = undefined;
        if (!closed && failure === null && savedEditVersion !== editVersion)
          void start();
      });
    active = work;
    return work;
  }
  const value: Controller = {
    edit(input) {
      if (closed || editVersion === Number.MAX_SAFE_INTEGER) return false;
      const next = publicDraftFormSnapshot(input);
      if (!next || closed || editVersion === Number.MAX_SAFE_INTEGER)
        return false;
      form = next;
      editVersion++;
      if (failure === null) {
        state = 'editing';
        void start();
      }
      return true;
    },
    async flush() {
      if (closed || failure !== null) return false;
      while (!closed && failure === null && savedEditVersion !== editVersion)
        await start();
      return !closed && failure === null && savedEditVersion === editVersion;
    },
    async retry() {
      if (closed) return false;
      if (active) await active;
      if (closed) return false;
      failure = null;
      // Retained attempt is committed/read back as originally captured, never
      // replaced with a new ID, time or payload during uncertain recovery.
      while (!closed && failure === null && savedEditVersion !== editVersion)
        await start();
      return !closed && failure === null && savedEditVersion === editVersion;
    },
    async saveClose() {
      while (await value.flush()) {
        // The await permits a pending edit or lifecycle close. Check the exact
        // current version and close together without another asynchronous gap.
        if (closed || failure !== null) return false;
        if (savedEditVersion !== editVersion) continue;
        value.close();
        return true;
      }
      return false;
    },
    close() {
      if (closed) return;
      closed = true;
      state = 'closed';
      // A native transaction already submitted may complete; its late result
      // cannot alter this closed controller or trigger another write.
      attempted = undefined;
    },
    snapshot() {
      return {
        form: { ...form },
        editVersion,
        savedEditVersion,
        draft: saved
          ? (JSON.parse(JSON.stringify(saved)) as PublicDraftRecord)
          : null,
        state,
        failure
      };
    }
  };
  const handle = Object.freeze({}) as PublicDraftController;
  controllers.set(handle, value);
  return handle;
}
export function editPublicDraft(
  controller: PublicDraftController,
  form: unknown
): boolean {
  return controllers.get(controller)?.edit(form) ?? false;
}
export function publicDraftControllerSnapshot(
  controller: PublicDraftController
): DraftControllerSnapshot | undefined {
  return controllers.get(controller)?.snapshot();
}
export function flushPublicDraft(
  controller: PublicDraftController
): Promise<boolean> {
  return controllers.get(controller)?.flush() ?? Promise.resolve(false);
}
export function retryPublicDraft(
  controller: PublicDraftController
): Promise<boolean> {
  return controllers.get(controller)?.retry() ?? Promise.resolve(false);
}
// Ordinary save-close is gated by a successful flush. Lifecycle/identity
// teardown uses explicit close instead and makes no successful-save claim.
export function saveAndClosePublicDraft(
  controller: PublicDraftController
): Promise<boolean> {
  return controllers.get(controller)?.saveClose() ?? Promise.resolve(false);
}
export function closePublicDraftController(
  controller: PublicDraftController
): void {
  controllers.get(controller)?.close();
}
