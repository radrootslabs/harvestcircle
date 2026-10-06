import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PublicDraftRecord } from '../../src/lib/contracts/local-records.ts';
import type {
  PublicDraftRepository,
  PublicDraftWrite,
  DraftResult
} from '../../src/lib/persistence/drafts.ts';

// Controlled receipt timing tests qualify the actual controller logic. Genuine
// IndexedDB persistence is qualified separately by the browser integration.
type Fixture = {
  rows: Map<object, PublicDraftRecord>;
  commits: object[];
  clock: number;
  outcome:
    undefined | ((write: object) => Promise<DraftResult<PublicDraftRecord>>);
  observed: 'committed' | 'base_observed';
  captureHook: undefined | (() => void);
};
const fixture = vi.hoisted<Fixture>(() => ({
  rows: new Map<object, PublicDraftRecord>(),
  commits: [],
  clock: 1000,
  outcome: undefined,
  observed: 'committed',
  captureHook: undefined
}));
vi.mock('../../src/lib/persistence/drafts.ts', async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import('../../src/lib/persistence/drafts.ts')
    >();
  const capture = (form: unknown, revision: number) => {
    fixture.captureHook?.();
    const admitted = actual.publicDraftFormSnapshot(form);
    if (!admitted)
      return { ok: false as const, reason: 'invalid_form' as const };
    const handle = Object.freeze({}) as PublicDraftWrite;
    fixture.rows.set(handle, {
      schema: 1,
      family: 'public_draft',
      owner: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
      id: '3d030bf5-901d-45e1-8251-41cbdf805e96',
      revision,
      savedAtMilliseconds: fixture.clock++,
      form: admitted
    });
    return { ok: true as const, value: handle };
  };
  return {
    ...actual,
    publicDraftRepositoryOwner: () => 'controlled-unit-owner',
    capturePublicDraftCreate: (
      _repository: PublicDraftRepository,
      form: unknown
    ) => capture(form, 0),
    capturePublicDraftSave: (
      _repository: PublicDraftRepository,
      _id: string,
      revision: number,
      form: unknown
    ) => capture(form, revision + 1),
    publicDraftWriteSnapshot: (
      _repository: PublicDraftRepository,
      write: PublicDraftWrite
    ) => fixture.rows.get(write),
    commitPublicDraftWrite: async (
      _repository: PublicDraftRepository,
      write: PublicDraftWrite
    ) => {
      fixture.commits.push(write);
      if (fixture.outcome) return fixture.outcome(write);
      const row = fixture.rows.get(write);
      if (!row) throw new Error('missing controlled capture');
      return { ok: true as const, value: structuredClone(row) };
    },
    observePublicDraftWrite: (
      _repository: PublicDraftRepository,
      write: PublicDraftWrite
    ) => {
      if (fixture.observed === 'base_observed')
        return Promise.resolve({ state: 'base_observed' as const });
      const row = fixture.rows.get(write);
      if (!row) throw new Error('missing controlled capture');
      return Promise.resolve({
        state: 'committed' as const,
        value: structuredClone(row)
      });
    }
  };
});
import {
  createPublicDraftController,
  editPublicDraft,
  flushPublicDraft,
  publicDraftControllerSnapshot,
  retryPublicDraft,
  saveAndClosePublicDraft,
  closePublicDraftController
} from '../../src/lib/publishing/draft-controller.ts';
const repository = Object.freeze({}) as PublicDraftRepository;
const form = {
  title: '',
  description: '',
  location: '',
  amount: '.',
  currency: '',
  unit: '',
  quantity: '',
  contactType: '' as const,
  contactValue: ''
};
beforeEach(() => {
  fixture.rows.clear();
  fixture.commits = [];
  fixture.clock = 1000;
  fixture.outcome = undefined;
  fixture.observed = 'committed';
  fixture.captureHook = undefined;
});
async function controller() {
  const value = await createPublicDraftController(repository, form);
  if (!value) throw new Error('missing controlled controller');
  return value;
}
describe('single owned public autosave worker', () => {
  it('synchronous pending edits coalesce into one actual controller capture', async () => {
    const value = await controller();
    expect(editPublicDraft(value, { ...form, title: 'one' })).toBe(true);
    expect(editPublicDraft(value, { ...form, title: 'latest' })).toBe(true);
    expect(await flushPublicDraft(value)).toBe(true);
    expect(fixture.commits).toHaveLength(1);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'saved',
      editVersion: 2,
      savedEditVersion: 2,
      form: { title: 'latest' },
      draft: { form: { title: 'latest' } }
    });
  });
  it('delayed acknowledgement never overwrites newer input and only one write runs at a time', async () => {
    let acknowledge: (() => void) | undefined;
    fixture.outcome = async (write) => {
      const row = fixture.rows.get(write);
      if (!row) throw new Error('missing capture');
      if (fixture.commits.length === 1)
        await new Promise<void>((resolve) => {
          acknowledge = resolve;
        });
      return { ok: true, value: structuredClone(row) };
    };
    const value = await controller();
    editPublicDraft(value, { ...form, title: 'first' });
    const flushed = flushPublicDraft(value);
    await Promise.resolve();
    expect(fixture.commits).toHaveLength(1);
    editPublicDraft(value, { ...form, title: 'newer' });
    expect(publicDraftControllerSnapshot(value)?.form.title).toBe('newer');
    if (!acknowledge) throw new Error('missing delayed acknowledgement');
    acknowledge();
    expect(await flushed).toBe(true);
    expect(fixture.commits).toHaveLength(2);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'saved',
      form: { title: 'newer' },
      draft: { revision: 1 }
    });
  });
  it('unknown receipt with exact readback acknowledges the original captured payload', async () => {
    fixture.outcome = () =>
      Promise.resolve({ ok: false, reason: 'unknown_completion' });
    const value = await controller();
    editPublicDraft(value, { ...form, title: 'retained' });
    expect(await flushPublicDraft(value)).toBe(true);
    expect(fixture.rows.size).toBe(1);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'saved',
      draft: { form: { title: 'retained' }, savedAtMilliseconds: 1000 }
    });
  });
  it('unobserved unknown result blocks navigation and explicit retry retains the same ID/time/capture before pending edits', async () => {
    fixture.outcome = () =>
      Promise.resolve({ ok: false, reason: 'unknown_completion' });
    fixture.observed = 'base_observed';
    const value = await controller();
    editPublicDraft(value, { ...form, title: 'attempted' });
    expect(await flushPublicDraft(value)).toBe(false);
    const original = fixture.commits[0];
    editPublicDraft(value, { ...form, title: 'pending' });
    expect(await saveAndClosePublicDraft(value)).toBe(false);
    expect(publicDraftControllerSnapshot(value)?.state).toBe('failed');
    fixture.outcome = undefined;
    expect(await retryPublicDraft(value)).toBe(true);
    expect(fixture.commits[1]).toBe(original);
    expect(fixture.rows.size).toBe(2);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'saved',
      form: { title: 'pending' },
      draft: { revision: 1 }
    });
  });
  it('quota failure retains dirty input and prevents save-close', async () => {
    fixture.outcome = () =>
      Promise.resolve({ ok: false, reason: 'cap_reached' });
    const value = await controller();
    editPublicDraft(value, { ...form, description: 'retain me' });
    expect(await saveAndClosePublicDraft(value)).toBe(false);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'failed',
      failure: 'cap_reached',
      form: { description: 'retain me' },
      savedEditVersion: -1
    });
  });
  it('closing while a request is pending fences late acknowledgements and subsequent captures', async () => {
    let finish: (() => void) | undefined;
    fixture.outcome = async (write) => {
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
      const row = fixture.rows.get(write);
      if (!row) throw new Error('missing capture');
      return { ok: true, value: row };
    };
    const value = await controller();
    editPublicDraft(value, form);
    const flushed = flushPublicDraft(value);
    await Promise.resolve();
    closePublicDraftController(value);
    if (!finish) throw new Error('missing pending receipt');
    finish();
    expect(await flushed).toBe(false);
    expect(fixture.commits).toHaveLength(1);
    expect(editPublicDraft(value, form)).toBe(false);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'closed',
      savedEditVersion: -1,
      draft: null
    });
  });
  it('private composer shapes and caller snapshot mutations cannot enter public autosave', async () => {
    const value = await controller();
    expect(
      editPublicDraft(value, { body: 'private', subject: 'private' })
    ).toBe(false);
    const copy = publicDraftControllerSnapshot(value);
    if (!copy) throw new Error('missing snapshot');
    const mutable = copy.form as { title: string };
    mutable.title = 'foreign mutation';
    expect(publicDraftControllerSnapshot(value)?.form.title).toBe('');
    expect(fixture.commits).toHaveLength(0);
  });
  it('save-close rechecks an edit arriving after an already-saved flush resolves', async () => {
    const value = await controller();
    editPublicDraft(value, { ...form, title: 'first' });
    expect(await flushPublicDraft(value)).toBe(true);
    const closing = saveAndClosePublicDraft(value);
    expect(
      editPublicDraft(value, { ...form, title: 'latest before close' })
    ).toBe(true);
    expect(await closing).toBe(true);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'closed',
      savedEditVersion: 2,
      editVersion: 2,
      draft: { form: { title: 'latest before close' } }
    });
  });

  it('input admission cannot revive a controller closed by a reentrant proxy', async () => {
    const value = await controller();
    const input = new Proxy(form, {
      get(target, property, receiver) {
        if (property === 'title') closePublicDraftController(value);
        return Reflect.get(target, property, receiver) as unknown;
      }
    });
    expect(editPublicDraft(value, input)).toBe(false);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'closed',
      editVersion: 0,
      draft: null
    });
    expect(fixture.commits).toHaveLength(0);
  });
  it('capture reentry never labels older form bytes with the newer edit version', async () => {
    const value = await controller();
    fixture.captureHook = () => {
      fixture.captureHook = undefined;
      editPublicDraft(value, { ...form, title: 'newer during capture' });
    };
    editPublicDraft(value, { ...form, title: 'first captured' });
    expect(await flushPublicDraft(value)).toBe(true);
    expect(fixture.commits).toHaveLength(2);
    expect(publicDraftControllerSnapshot(value)).toMatchObject({
      state: 'saved',
      editVersion: 2,
      savedEditVersion: 2,
      form: { title: 'newer during capture' },
      draft: { revision: 1, form: { title: 'newer during capture' } }
    });
  });
});
