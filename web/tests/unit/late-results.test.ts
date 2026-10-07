import { describe, it, expect } from 'vitest';
import {
  makeFixture,
  approveCapturedPublicSigning,
  capturedArtifactSnapshot,
  publicRecordSnapshot
} from '../e2e/harness/approved-signing.ts';
import { decodePublicRecord } from '../../src/lib/persistence/records.ts';
import {
  bindLatePublicResponse,
  latePublicResultSnapshot,
  reviewLatePublicArtifact,
  type LatePublicArtifact
} from '../../src/lib/runtime/late-results.ts';
describe('original public late-result review', () => {
  for (const kind of [30402, 5, 10050] as const)
    it(`retains exact verified ${kind} only against its original captured operation`, () => {
      const f = makeFixture(kind);
      try {
        const approval = approveCapturedPublicSigning(
          f.record,
          f.owner,
          f.id,
          'reviewed_captured_operation'
        )!;
        const signed = f.sign(f.template),
          late = bindLatePublicResponse(approval, signed, Symbol())!;
        expect(latePublicResultSnapshot(late)).toMatchObject({
          owner: f.owner,
          id: f.id,
          revision: 0
        });
        expect(
          reviewLatePublicArtifact(late, f.record, f.owner, f.id, 'unreviewed')
        ).toBeUndefined();
        const reviewed = reviewLatePublicArtifact(
          late,
          f.record,
          f.owner,
          f.id,
          'review_original_late_artifact'
        );
        expect(
          reviewed && JSON.parse(capturedArtifactSnapshot(reviewed)!.wire)
        ).toEqual(JSON.parse(JSON.stringify(signed)));
      } finally {
        f.close();
      }
    });
  it('changed full record or owner/ID cannot reinterpret the original result', () => {
    const f = makeFixture();
    try {
      const approval = approveCapturedPublicSigning(
        f.record,
        f.owner,
        f.id,
        'reviewed_captured_operation'
      )!;
      const late = bindLatePublicResponse(
        approval,
        f.sign(f.template),
        Symbol()
      )!;
      const row = publicRecordSnapshot(f.record, f.owner, f.id)!;
      if (row.family === 'public_draft') throw new Error('invalid fixture');
      const changed = decodePublicRecord(
        JSON.stringify({
          ...row,
          capture: { ...row.capture, targets: ['wss://other.example.org'] }
        }),
        f.owner,
        f.id
      );
      expect(changed.ok).toBe(true);
      expect(
        changed.ok &&
          reviewLatePublicArtifact(
            late,
            changed.value,
            f.owner,
            f.id,
            'review_original_late_artifact'
          )
      ).toBeUndefined();
      expect(
        reviewLatePublicArtifact(
          late,
          f.record,
          f.owner,
          crypto.randomUUID(),
          'review_original_late_artifact'
        )
      ).toBeUndefined();
      expect(
        reviewLatePublicArtifact(
          late,
          f.record,
          '',
          f.id,
          'review_original_late_artifact'
        )
      ).toBeUndefined();
    } finally {
      f.close();
    }
  });
  for (const mode of ['content', 'cached', 'author'] as const)
    it(`rejects late ${mode} response despite SDK verification cache`, () => {
      const f = makeFixture();
      try {
        const approval = approveCapturedPublicSigning(
          f.record,
          f.owner,
          f.id,
          'reviewed_captured_operation'
        )!;
        expect(
          bindLatePublicResponse(approval, f.sign(f.template, mode), Symbol())
        ).toBeUndefined();
      } finally {
        f.close();
      }
    });
  it('fake tokens and mutated disclosure cannot convey artifact authority', () => {
    const f = makeFixture();
    try {
      expect(
        latePublicResultSnapshot({} as LatePublicArtifact)
      ).toBeUndefined();
      expect(
        reviewLatePublicArtifact(
          {} as LatePublicArtifact,
          f.record,
          f.owner,
          f.id,
          'review_original_late_artifact'
        )
      ).toBeUndefined();
      const approval = approveCapturedPublicSigning(
        f.record,
        f.owner,
        f.id,
        'reviewed_captured_operation'
      )!;
      const late = bindLatePublicResponse(
        approval,
        f.sign(f.template),
        Symbol()
      )!;
      const view = latePublicResultSnapshot(late)!;
      (view as { id: string }).id = crypto.randomUUID();
      expect(latePublicResultSnapshot(late)?.id).toBe(f.id);
      expect(
        reviewLatePublicArtifact(
          late,
          f.record,
          f.owner,
          f.id,
          'review_original_late_artifact'
        )
      ).toBeDefined();
    } finally {
      f.close();
    }
  });
});
