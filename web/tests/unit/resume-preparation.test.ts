import { describe, it, expect } from 'vitest';
import {
  captureResumePreparation,
  resumeEncryptedPreparation,
  resumePreparationSnapshot,
  resumeRecoveredRumor,
  type ResumePreparation
} from '../../src/lib/messaging/resume-preparation.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
describe('explicit encrypted recovery cannot manufacture a new message', () => {
  it('SSR and forged preparation are inert', async () => {
    expect(
      captureResumePreparation(
        {} as PrivateStorageRepository,
        {} as IdentitySession,
        '12345678-1234-4234-8234-123456789abc',
        'reviewed_private_resume'
      )
    ).toBeUndefined();
    expect(
      await resumeEncryptedPreparation(
        {} as ResumePreparation,
        undefined,
        'reviewed_private_resume'
      )
    ).toEqual({ status: 'invalid' });
  });
  it('detached tokens expose neither recovered text nor acknowledgement', () => {
    expect(resumePreparationSnapshot({} as ResumePreparation)).toBeUndefined();
    expect(resumeRecoveredRumor({} as ResumePreparation)).toBeUndefined();
  });
  it('unreviewed input cannot coerce itself into Resume', async () => {
    let calls = 0;
    const review = {
      toString() {
        calls++;
        return 'reviewed_private_resume';
      }
    };
    expect(
      await resumeEncryptedPreparation(
        {} as ResumePreparation,
        undefined,
        review
      )
    ).toEqual({ status: 'invalid' });
    expect(calls).toBe(0);
  });
});
