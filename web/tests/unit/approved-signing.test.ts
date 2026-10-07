import { describe, expect, it } from 'vitest';
import { getEventHash } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import { decodePublicRecord } from '../../src/lib/persistence/records.ts';
import {
  makeFixture,
  approveCapturedPublicSigning,
  disposableApprovedTemplate,
  bindApprovedResponse,
  capturedArtifactSnapshot,
  publicRecordSnapshot
} from '../e2e/harness/approved-signing.ts';
import type { ApprovedPublicSigning } from '../../src/lib/nostr/approved-signing.ts';
import type { PublicRecordHandle } from '../../src/lib/persistence/records.ts';
function approved(f: ReturnType<typeof makeFixture>) {
  const value = approveCapturedPublicSigning(
    f.record,
    f.owner,
    f.id,
    'reviewed_captured_operation'
  );
  if (!value) throw new Error('missing approved fixture');
  return value;
}
describe('immutable approved signer responses', () => {
  it('rejects a revoked response Proxy without throwing beyond the finite boundary', () => {
    const f = makeFixture();
    try {
      const a = approved(f),
        revoked = Proxy.revocable({}, {});
      revoked.revoke();
      expect(() => bindApprovedResponse(a, revoked.proxy)).not.toThrow();
      expect(bindApprovedResponse(a, revoked.proxy)).toBeUndefined();
    } finally {
      f.close();
    }
  });
  for (const kind of [30402, 5, 10050] as const)
    it(`admits genuine exact signed ${kind} from a captured reviewed operation`, () => {
      const f = makeFixture(kind);
      try {
        const a = approved(f),
          copy = disposableApprovedTemplate(a);
        if (!copy) throw new Error('missing copy');
        const artifact = bindApprovedResponse(a, f.sign(copy));
        expect(artifact).toBeDefined();
        expect(artifact && capturedArtifactSnapshot(artifact)?.hash).toBe(
          getEventHash(f.template)
        );
        expect(artifact && capturedArtifactSnapshot(artifact)?.author).toBe(
          f.owner
        );
      } finally {
        f.close();
      }
    });
  for (const mode of [
    'author',
    'kind',
    'time',
    'tags',
    'content',
    'cached',
    'mutate'
  ] as const)
    it(`rejects hostile ${mode} even with a mutable verified cache flag`, () => {
      const f = makeFixture();
      try {
        const a = approved(f),
          copy = disposableApprovedTemplate(a);
        if (!copy) throw new Error('missing copy');
        const response = f.sign(copy, mode);
        expect(verifyEnvelope(JSON.stringify(response)).ok).toBe(
          mode !== 'cached'
        );
        expect(bindApprovedResponse(a, response)).toBeUndefined();
        expect(disposableApprovedTemplate(a)).toEqual(f.template);
      } finally {
        f.close();
      }
    });
  it('retains independent original despite mutable caller snapshots and disposable references', () => {
    const f = makeFixture();
    try {
      const a = approved(f),
        copy = disposableApprovedTemplate(a);
      if (!copy) throw new Error('missing copy');
      copy.tags.push(['changed']);
      copy.content = 'changed';
      const snapshot = publicRecordSnapshot(f.record, f.owner, f.id);
      if (snapshot && snapshot.family !== 'public_draft')
        Object.assign(snapshot.capture, { wire: '{}' });
      expect(disposableApprovedTemplate(a)).toEqual(f.template);
      expect(bindApprovedResponse(a, f.sign(f.template))).toBeDefined();
    } finally {
      f.close();
    }
  });
  it('requires genuine owner/id scoped record and explicit review', () => {
    const f = makeFixture();
    try {
      expect(
        approveCapturedPublicSigning(f.record, f.owner, f.id, undefined)
      ).toBeUndefined();
      expect(
        approveCapturedPublicSigning(
          f.record,
          f.owner,
          crypto.randomUUID(),
          'reviewed_captured_operation'
        )
      ).toBeUndefined();
      expect(
        approveCapturedPublicSigning(
          {} as PublicRecordHandle,
          f.owner,
          f.id,
          'reviewed_captured_operation'
        )
      ).toBeUndefined();
      expect(
        disposableApprovedTemplate({} as ApprovedPublicSigning)
      ).toBeUndefined();
      expect(
        bindApprovedResponse({} as ApprovedPublicSigning, f.sign(f.template))
      ).toBeUndefined();
    } finally {
      f.close();
    }
  });
  it('reconstructs own primitive fields without calling getters, toJSON or tag iterators', () => {
    const f = makeFixture();
    try {
      const a = approved(f),
        event = f.sign(f.template);
      let touched = 0;
      Object.defineProperty(event, 'toJSON', {
        value: () => {
          touched++;
          throw new Error('test-only');
        }
      });
      Object.defineProperty(event.tags, Symbol.iterator, {
        value: () => {
          touched++;
          throw new Error('test-only');
        }
      });
      expect(bindApprovedResponse(a, event)).toBeDefined();
      Object.defineProperty(event, 'content', {
        get: () => {
          touched++;
          return f.template.content;
        }
      });
      expect(bindApprovedResponse(a, event)).toBeUndefined();
      expect(touched).toBe(0);
    } finally {
      f.close();
    }
  });
  it('rejects draft or already signed records and another owner without creating sign authority', () => {
    const f = makeFixture(),
      other = makeFixture();
    try {
      expect(
        approveCapturedPublicSigning(
          f.record,
          other.owner,
          f.id,
          'reviewed_captured_operation'
        )
      ).toBeUndefined();
      const row = publicRecordSnapshot(f.record, f.owner, f.id);
      if (!row || row.family === 'public_draft')
        throw new Error('missing operation');
      const event = f.sign(f.template);
      const signed = decodePublicRecord(
        JSON.stringify({
          ...row,
          artifact: { eventId: event.id, wire: JSON.stringify(event) }
        }),
        f.owner,
        f.id
      );
      expect(signed.ok).toBe(true);
      if (signed.ok)
        expect(
          approveCapturedPublicSigning(
            signed.value,
            f.owner,
            f.id,
            'reviewed_captured_operation'
          )
        ).toBeUndefined();
      const draft = decodePublicRecord(
        JSON.stringify({
          schema: 1,
          family: 'public_draft',
          owner: f.owner,
          id: f.id,
          revision: 0,
          savedAtMilliseconds: 0,
          form: {
            title: '',
            description: '',
            location: '',
            amount: '',
            currency: '',
            unit: '',
            quantity: '',
            contactType: '',
            contactValue: ''
          }
        }),
        f.owner,
        f.id
      );
      expect(draft.ok).toBe(true);
      if (draft.ok)
        expect(
          approveCapturedPublicSigning(
            draft.value,
            f.owner,
            f.id,
            'reviewed_captured_operation'
          )
        ).toBeUndefined();
    } finally {
      f.close();
      other.close();
    }
  });
  it('bounds huge/sparse/malformed tags and rejects unsupported numbers before encoding', () => {
    const f = makeFixture();
    try {
      const a = approved(f),
        event = f.sign(f.template);
      for (const value of [
        null,
        [],
        { ...event, tags: Array(1025) },
        { ...event, tags: [Array(4097)] },
        { ...event, created_at: -0 },
        { ...event, kind: NaN },
        { ...event, content: 'x'.repeat(131073) },
        { ...event, tags: [['x', '\ud800']] }
      ])
        expect(bindApprovedResponse(a, value)).toBeUndefined();
    } finally {
      f.close();
    }
  });
});
