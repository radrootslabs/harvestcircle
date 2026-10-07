import { expect, expectTypeOf, it } from 'vitest';
import * as boundary from '../../src/lib/nostr/private-envelope-capability.ts';
import type {
  PrivateEnvelopeCapability,
  PrivateOuterTemplate
} from '../../src/lib/nostr/private-envelope-capability.ts';

it('outer construction permission is SSR inert and exposes no signer or credential API', () => {
  expect(boundary.getPrivateEnvelopeCapability({} as never)).toBeUndefined();
  expect(Object.keys(boundary).sort()).toEqual([
    'closePrivateEnvelopeCapability',
    'getPrivateEnvelopeCapability',
    'privateEnvelopeConstructionOwnership'
  ]);
  expectTypeOf<PrivateOuterTemplate['kind']>().toEqualTypeOf<1059>();
  expectTypeOf<PrivateOuterTemplate['tags']>().toEqualTypeOf<
    readonly [readonly ['p', string]]
  >();
  expectTypeOf<
    Extract<
      keyof PrivateEnvelopeCapability,
      'signEvent' | 'key' | 'nip44' | 'exportKey' | 'importKey'
    >
  >().toEqualTypeOf<never>();
});
it('forged outer capability cannot authorize signing, acquire ownership or close a scope', () => {
  expect(() =>
    boundary.privateEnvelopeConstructionOwnership({} as never)
  ).toThrow('private_envelope_capability_invalid');
  expect(() => boundary.closePrivateEnvelopeCapability({} as never)).toThrow(
    'private_envelope_capability_invalid'
  );
});
