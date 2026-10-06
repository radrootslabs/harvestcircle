import { describe, it, expect } from 'vitest';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../src/lib/config/relays.ts';
import { nip50SearchQuery } from '../../src/lib/nostr/search-query.ts';
import { qualifiedNip50Sources } from '../../src/lib/nostr/search-sources.ts';
const policy = validateRelayPolicy(
  JSON.stringify({
    schemaVersion: 1,
    public: [
      {
        origin: 'wss://one.example.org',
        read: true,
        write: false,
        nip50: true
      },
      {
        origin: 'wss://two.example.org',
        read: true,
        write: false,
        nip50: false
      },
      {
        origin: 'wss://three.example.org',
        read: false,
        write: true,
        nip50: false
      }
    ],
    inbox: [],
    postingEnabled: false,
    messagingEnabled: false,
    operatorDenylist: []
  })
)!;
describe('optional NIP50 request preparation', () => {
  it('selects only genuine manifest-qualified readable sources', () => {
    expect(qualifiedNip50Sources(policy)).toEqual(['wss://one.example.org']);
    const detached = readRelayPolicy(policy);
    expect(detached.public).toHaveLength(3);
    expect(() => qualifiedNip50Sources({} as typeof policy)).toThrow(
      'relay_policy_invalid'
    );
  });
  it('normalizes public query copies and requests one finite kind30402 sample without a cursor', () => {
    expect(nip50SearchQuery(' ＣＡＲＲＯＴＳ\n celery ')).toEqual([
      { kinds: [30402], search: 'carrots celery', limit: 100 }
    ]);
  });
  it('empty queries never request relevance sampling', () => {
    expect(nip50SearchQuery(' \n ')).toEqual([]);
  });
  it('refuses unsafe query inputs under unchanged byte and term bounds', () => {
    expect(() => nip50SearchQuery('x'.repeat(513))).toThrow('query_too_long');
    expect(() => nip50SearchQuery(Array(13).fill('a').join(' '))).toThrow(
      'too_many_terms'
    );
    expect(() => nip50SearchQuery({})).toThrow();
  });
});
