import { render } from 'svelte/server';
import { describe, it, expect } from 'vitest';
import Harness from './CapabilityGateHarness.svelte';
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
describe('shared safe connection capability presentation', () => {
  it('guest exposes connection help and browse, with no private form or SSR effect', () => {
    const forbidden = () => {
      throw new Error('SSR effect');
    };
    const html = render(Harness, { props: { onconnect: forbidden } }).body;
    expect(html).toContain('Connect extension');
    expect(html).toContain('Keep browsing');
    expect(html).toContain('private key stays in the extension');
    expect(html).not.toMatch(/<form|<textarea|nsec|password|HC_PRIVATE/);
  });
  it('pending approval is visibly distinct and cannot schedule another connection', () => {
    const html = render(Harness, {
      props: {
        identity: { state: 'pending', action: 'connect' },
        onconnect: () => {}
      }
    }).body;
    expect(html).toContain('Waiting for extension');
    expect(html).toMatch(/<button[^>]*disabled/);
  });
  it('missing or denied extension gets finite safe help, never provider diagnostics', () => {
    for (const reason of ['missing', 'refused'] as const) {
      const html = render(Harness, {
        props: { identity: { state: 'guest', reason } }
      }).body;
      expect(html).toContain('Connection help');
      expect(html).not.toMatch(/stack|nsec|<input/);
    }
  });
  it('signing capability is not messaging readiness or inbox authority', () => {
    const html = render(Harness, {
      props: {
        required: 'messaging',
        identity: {
          state: 'signing_only',
          publicKey: owner,
          messaging: 'unsupported'
        },
        onprobe: () => {}
      }
    }).body;
    expect(html).toContain('message encryption');
    expect(html).not.toMatch(
      /Send message|Publish listing|inbox ready|<textarea/
    );
  });
  it('connected presentation never executes an earlier operation or exposes a private body', () => {
    const html = render(Harness, {
      props: {
        identity: {
          state: 'messaging_capable',
          publicKey: owner,
          messaging: 'capable'
        },
        required: 'messaging',
        onconnect: () => {
          throw new Error('Send');
        }
      }
    }).body;
    expect(html).toContain('Messaging support checked');
    expect(html).not.toMatch(/HC_PRIVATE|Send message|<form/);
  });
});
