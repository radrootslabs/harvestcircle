import { describe, expect, it } from 'vitest';
import {
  approvePrivateResumeRouting,
  privateResumeRoutingSnapshot,
  type PrivateResumeRouting
} from '../../src/lib/messaging/resume-routing.ts';
describe('private route review is an opaque explicit local operation', () => {
  it('a forged route review cannot expose a peer or destination', () => {
    expect(
      privateResumeRoutingSnapshot({} as PrivateResumeRouting)
    ).toBeUndefined();
  });
  it('a forged reviewed token cannot mutate custody or publish', async () => {
    expect(
      await approvePrivateResumeRouting(
        {} as PrivateResumeRouting,
        'reviewed_private_destination_update'
      )
    ).toEqual({ status: 'invalid' });
  });
  it('caller objects never coerce into consent or a route review', async () => {
    let called = 0;
    const value = {
      toString() {
        called++;
        return 'reviewed_private_destination_update';
      }
    };
    expect(
      await approvePrivateResumeRouting({} as PrivateResumeRouting, value)
    ).toEqual({ status: 'invalid' });
    expect(called).toBe(0);
  });
});
