import { afterEach, describe, expect, it, vi } from 'vitest';
import { copyProductReference } from '../../src/lib/navigation-copy.ts';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
afterEach(() => vi.unstubAllGlobals());
describe('explicit product reference copy', () => {
  it('copies only canonical public route and reports actual success', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const naddr = encodeProductReference({
      kind: 30402,
      pubkey: 'ab'.repeat(32),
      identifier: 'carrots'
    })!;
    expect(
      await copyProductReference(naddr.toUpperCase(), 'https://harvest.example')
    ).toBe(true);
    expect(writeText).toHaveBeenCalledExactlyOnceWith(
      `https://harvest.example/products/${naddr}`
    );
  });
  it('reports denial/unavailable and rejects invalid input before effects', async () => {
    const writeText = vi.fn().mockRejectedValue(new Error('denied'));
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    expect(
      await copyProductReference(
        'https://bad.example',
        'https://harvest.example'
      )
    ).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
    const naddr = encodeProductReference({
      kind: 30402,
      pubkey: 'ab'.repeat(32),
      identifier: 'carrots'
    })!;
    expect(await copyProductReference(naddr, 'https://harvest.example')).toBe(
      false
    );
    vi.stubGlobal('navigator', undefined);
    expect(await copyProductReference(naddr, 'https://harvest.example')).toBe(
      false
    );
  });
  it('rejects noncanonical, credential, remote HTTP and javascript origins before copying', async () => {
    const writeText = vi.fn();
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const naddr = encodeProductReference({
      kind: 30402,
      pubkey: 'ab'.repeat(32),
      identifier: 'carrots'
    })!;
    for (const origin of [
      'https://host.example/',
      'https://user:pass@host.example',
      'http://host.example',
      'javascript:alert(1)',
      'https://host.example?secret=1',
      undefined
    ])
      expect(await copyProductReference(naddr, origin)).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });
});
