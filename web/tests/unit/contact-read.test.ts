import { describe, expect, it } from 'vitest';
import { readPublicContact } from '../../src/lib/contracts/food-availability-v1/contact-read.ts';
describe('dedicated optional public contact', () => {
  it('accepts only the writer final block with canonical safe contact', () => {
    for (const href of [
      'mailto:seller@example.org',
      'tel:+12025550123',
      'https://example.org/contact'
    ])
      expect(
        readPublicContact(`Carrots\n\nPublic contact: ${href}`)?.href
      ).toBe(href);
  });
  it('never infers prose, unsafe or malformed contact', () => {
    for (const content of [
      undefined,
      {},
      'https://example.org',
      'Public contact: mailto:seller@example.org',
      'Carrots\n\nPublic contact: javascript:alert(1)',
      'Carrots\n\nPublic contact: https://example.org\nOther text',
      'Carrots\n\nPublic contact: mailto:a@example.org?subject=secret',
      'Carrots\n\nPublic contact: https://user:pass@example.org',
      'Carrots\n\nPublic contact: https://example.org/%0a'
    ])
      expect(readPublicContact(content)).toBeUndefined();
  });
  it('rejects hinted relays, remote resource schemes and nested contact injection', () => {
    for (const href of [
      'wss://relay.example.org',
      'http://contact.example.org',
      'data:text/html,hello',
      'blob:https://contact.example.org/id',
      '//contact.example.org',
      'https://user@contact.example.org',
      'https://contact.example.org/%0d%0aHeader:value',
      'mailto:seller@example.org?body=private',
      'mailto:seller@example.org%0aBcc:other@example.org',
      'tel:+12025550123;ext=1',
      'tel:12025550123',
      'https://contact.example.org/' + 'a'.repeat(8192)
    ])
      expect(
        readPublicContact(`Carrots\n\nPublic contact: ${href}`)
      ).toBeUndefined();
  });
});
