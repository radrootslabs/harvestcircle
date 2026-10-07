import { expect, it } from 'vitest';
import {
  getPrivateVisibilityScope,
  privateVisibilitySnapshot,
  closePrivateVisibilityScope
} from '../../src/lib/runtime/dispose.ts';
it('SSR visibility scope acquisition is inert without a browser or provider', () => {
  expect(getPrivateVisibilityScope({} as never)).toBeUndefined();
});
it('forged visibility handles have no lifecycle authority', () => {
  expect(() => privateVisibilitySnapshot({} as never)).toThrow(
    'private_visibility_invalid'
  );
  expect(() => closePrivateVisibilityScope({} as never)).toThrow(
    'private_visibility_invalid'
  );
});
