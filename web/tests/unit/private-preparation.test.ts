import { describe, it, expect } from 'vitest';
import {
  createPrivateComposer,
  updatePrivateComposerText,
  privateComposerText,
  privateComposerSnapshot,
  discardPrivateComposer,
  type PrivateComposer
} from '../../src/lib/messaging/composer-state.ts';
import {
  captureDirtyNavigation,
  requestPrivateNavigation,
  confirmDiscardNavigation,
  keepPrivateEditing,
  dirtyNavigationSnapshot,
  type DirtyNavigation
} from '../../src/lib/runtime/dirty-navigation.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
describe('private composer capabilities do not persist plaintext or infer acknowledgement', () => {
  it('SSR and forged composers are inert', () => {
    expect(
      createPrivateComposer({} as IdentitySession, 'reviewed_private_composer')
    ).toBeUndefined();
    const forged = {} as PrivateComposer;
    expect(
      updatePrivateComposerText(forged, 'private', 'reviewed_private_text')
    ).toBe(false);
    expect(privateComposerText(forged)).toBeUndefined();
    expect(privateComposerSnapshot(forged)).toBeUndefined();
    expect(
      discardPrivateComposer(forged, 0, 'reviewed_discard_private_text')
    ).toBe(false);
    expect(captureDirtyNavigation(forged)).toBeUndefined();
  });
  it('forged navigation cannot discard or grant leave permission', () => {
    const forged = {} as DirtyNavigation;
    expect(requestPrivateNavigation(forged)).toBe('invalid');
    expect(
      confirmDiscardNavigation(forged, 'reviewed_discard_private_text')
    ).toBe('invalid');
    expect(keepPrivateEditing(forged)).toBe(false);
    expect(dirtyNavigationSnapshot(forged)).toBeUndefined();
  });
  it('review objects are never coerced', () => {
    let calls = 0;
    const review = {
      toString() {
        calls++;
        return 'reviewed_private_text';
      }
    };
    expect(
      updatePrivateComposerText({} as PrivateComposer, 'private', review)
    ).toBe(false);
    expect(calls).toBe(0);
  });
});
