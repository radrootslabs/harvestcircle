import { describe, it, expect } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  validateRelayPolicy,
  type RelayPolicy
} from '../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../src/lib/nostr/request-scope.ts';
import {
  createInboxResolver,
  type InboxResolver
} from '../../src/lib/messaging/resolve-inbox.ts';
import { inboxPreferenceSnapshot } from '../../src/lib/nostr/inbox-preferences.ts';
import { inboxResolutionSnapshot } from '../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxRoutePlan,
  inboxRoutePlanSnapshot,
  recheckInboxRoutePlan
} from '../../src/lib/messaging/inbox-routing.ts';
const discovery = 'wss://discovery.example.org',
  peerOrigin = 'wss://peer.example.org',
  archiveOrigin = 'wss://archive.example.org',
  common = 'wss://common.example.org',
  unapproved = 'wss://hint.example.org';
function policy(targets = [peerOrigin, archiveOrigin, common]): RelayPolicy {
  return validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin: discovery, read: true, write: false, nip50: false }],
      inbox: targets.map((origin) => ({ origin, read: true, write: true })),
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
}
function author(relays: string[]) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const tags = relays.map((origin) => ['relay', origin]);
    const original = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [...tags, ['unknown', 'preserve']],
        content: ''
      },
      key
    );
    const removed = finalizeEvent(
      { kind: 10050, created_at: 200, tags: tags.slice(1), content: '' },
      key
    );
    const changed = finalizeEvent(
      {
        kind: 10050,
        created_at: 201,
        tags: [...tags, ['unknown', 'changed']],
        content: ''
      },
      key
    );
    return { original, removed, changed };
  } finally {
    key.fill(0);
  }
}
function resolved(
  event: ReturnType<typeof author>['original'],
  complete = true,
  now: () => number = () => 0
) {
  const scheduler = createPublicScheduler({
      now,
      schedule: () => () => {}
    }),
    run = createPublicRun(scheduler, policy());
  let current = true;
  const resolver = createInboxResolver(
    run,
    event.pubkey,
    (next) =>
      openInboxRequest(
        run,
        event.pubkey,
        (sink) => {
          sink({ type: 'EVENT', from: discovery, id: 'fixture', event });
          if (complete) sink({ type: 'EOSE', from: discovery, id: 'fixture' });
          return () => {};
        },
        next
      ),
    () => current
  );
  return {
    resolver,
    close: () => closePublicScheduler(scheduler),
    stale() {
      current = false;
    }
  };
}
function fixture(
  peerRelays = [peerOrigin, common],
  selfRelays = [archiveOrigin]
) {
  const self = author(selfRelays),
    peer = author(peerRelays),
    own = resolved(self.original),
    other = resolved(peer.original);
  return {
    self,
    peer,
    own,
    other,
    close() {
      own.close();
      other.close();
    }
  };
}
describe('recipient and sender archive routing', () => {
  it('selects recipient advertised allowed writes and computes sender archive independently', () => {
    const f = fixture();
    try {
      const plan = createInboxRoutePlan(
        policy(),
        f.self.original.pubkey,
        f.peer.original.pubkey,
        f.own.resolver,
        f.other.resolver
      )!;
      const r = inboxRoutePlanSnapshot(plan)!;
      expect(r.peer).toMatchObject({
        role: 'peer',
        author: f.peer.original.pubkey,
        targets: [peerOrigin, common],
        knownBase: { id: f.peer.original.id }
      });
      expect(r.archive).toMatchObject({
        role: 'self_archive',
        author: f.self.original.pubkey,
        targets: [archiveOrigin],
        knownBase: { id: f.self.original.id }
      });
      expect(Object.keys(plan)).toEqual([]);
      expect(Object.isFrozen(plan)).toBe(true);
    } finally {
      f.close();
    }
  });
  it('empty peer intersection never falls back to sender archive or public discovery', () => {
    const f = fixture([unapproved]);
    try {
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.peer.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
    } finally {
      f.close();
    }
  });
  it('empty sender archive never falls back to peer destinations', () => {
    const f = fixture([peerOrigin], [unapproved]);
    try {
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.peer.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
    } finally {
      f.close();
    }
  });
  it('keeps all advertised preference data while limiting selected routes to the fixed allowlist', () => {
    const f = fixture([
      unapproved,
      peerOrigin,
      common,
      archiveOrigin,
      'wss://fifth.example.org'
    ]);
    try {
      const before = inboxPreferenceSnapshot(
        inboxResolutionSnapshot(f.other.resolver).head!
      )!;
      expect(before.relays).toHaveLength(5);
      const plan = createInboxRoutePlan(
        policy(),
        f.self.original.pubkey,
        f.peer.original.pubkey,
        f.own.resolver,
        f.other.resolver
      )!;
      expect(inboxRoutePlanSnapshot(plan)?.peer.targets).toEqual([
        peerOrigin,
        archiveOrigin,
        common
      ]);
      expect(
        inboxPreferenceSnapshot(inboxResolutionSnapshot(f.other.resolver).head!)
      ).toEqual(before);
    } finally {
      f.close();
    }
  });
  it('rejects forged resolution/policy, invalid identities, swapped authors and self messaging', () => {
    const f = fixture();
    try {
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.peer.original.pubkey,
          {} as InboxResolver,
          f.other.resolver
        )
      ).toBeUndefined();
      expect(
        createInboxRoutePlan(
          {} as RelayPolicy,
          f.self.original.pubkey,
          f.peer.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
      expect(
        createInboxRoutePlan(
          policy(),
          'f'.repeat(64),
          f.peer.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
      expect(
        createInboxRoutePlan(
          policy(),
          f.peer.original.pubkey,
          f.self.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.self.original.pubkey,
          f.own.resolver,
          f.own.resolver
        )
      ).toBeUndefined();
      expect(
        inboxRoutePlanSnapshot(
          {} as NonNullable<ReturnType<typeof createInboxRoutePlan>>
        )
      ).toBeUndefined();
    } finally {
      f.close();
    }
  });
  it('partial, unsupported or stale preference evidence cannot produce routes', () => {
    const f = fixture(),
      partial = resolved(f.peer.original, false);
    try {
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.peer.original.pubkey,
          f.own.resolver,
          partial.resolver
        )
      ).toBeUndefined();
      f.other.stale();
      expect(
        createInboxRoutePlan(
          policy(),
          f.self.original.pubkey,
          f.peer.original.pubkey,
          f.own.resolver,
          f.other.resolver
        )
      ).toBeUndefined();
    } finally {
      f.close();
      partial.close();
    }
    const g = fixture([peerOrigin]),
      bad = resolved(g.peer.removed);
    try {
      expect(
        createInboxRoutePlan(
          policy(),
          g.self.original.pubkey,
          g.peer.original.pubkey,
          g.own.resolver,
          bad.resolver
        )
      ).toBeUndefined();
    } finally {
      g.close();
      bad.close();
    }
  });
  it('detached snapshots cannot retarget the opaque plan', () => {
    const f = fixture();
    try {
      const p = createInboxRoutePlan(
        policy(),
        f.self.original.pubkey,
        f.peer.original.pubkey,
        f.own.resolver,
        f.other.resolver
      )!;
      const r = inboxRoutePlanSnapshot(p)!;
      r.peer.targets.push(unapproved);
      r.peer.sources.pop();
      expect(inboxRoutePlanSnapshot(p)?.peer.targets).toEqual([
        peerOrigin,
        common
      ]);
      expect(inboxRoutePlanSnapshot(p)?.peer.sources).toHaveLength(1);
      expect(
        recheckInboxRoutePlan(p, policy(), f.own.resolver, f.other.resolver)
      ).toBe('unchanged');
    } finally {
      f.close();
    }
  });
  it('removed approved destinations and changed known preference versions require explicit review', () => {
    const f = fixture(),
      removed = resolved(f.peer.removed),
      version = resolved(f.peer.changed);
    try {
      const p = createInboxRoutePlan(
        policy(),
        f.self.original.pubkey,
        f.peer.original.pubkey,
        f.own.resolver,
        f.other.resolver
      )!;
      expect(
        recheckInboxRoutePlan(
          p,
          policy([archiveOrigin, common]),
          f.own.resolver,
          f.other.resolver
        )
      ).toBe('review_required');
      expect(
        recheckInboxRoutePlan(p, policy(), f.own.resolver, removed.resolver)
      ).toBe('review_required');
      expect(
        recheckInboxRoutePlan(p, policy(), f.own.resolver, version.resolver)
      ).toBe('review_required');
      expect(inboxRoutePlanSnapshot(p)?.peer.targets).toEqual([
        peerOrigin,
        common
      ]);
      f.other.stale();
      expect(
        recheckInboxRoutePlan(p, policy(), f.own.resolver, f.other.resolver)
      ).toBe('unavailable');
    } finally {
      f.close();
      removed.close();
      version.close();
    }
  });
});

it('peer clock reentry cannot admit a stale sender archive captured first', () => {
  const f = fixture();
  let armed = false;
  const peer = resolved(f.peer.original, true, () => {
    if (armed) f.own.stale();
    return 0;
  });
  try {
    armed = true;
    expect(
      createInboxRoutePlan(
        policy(),
        f.self.original.pubkey,
        f.peer.original.pubkey,
        f.own.resolver,
        peer.resolver
      )
    ).toBeUndefined();
  } finally {
    f.close();
    peer.close();
  }
});

it('peer clock reentry cannot report unchanged after sender scheduler closes', () => {
  const f = fixture();
  let armed = false;
  const peer = resolved(f.peer.original, true, () => {
    if (armed) f.own.close();
    return 0;
  });
  try {
    const p = createInboxRoutePlan(
      policy(),
      f.self.original.pubkey,
      f.peer.original.pubkey,
      f.own.resolver,
      peer.resolver
    )!;
    expect(p).toBeDefined();
    armed = true;
    expect(
      recheckInboxRoutePlan(p, policy(), f.own.resolver, peer.resolver)
    ).toBe('unavailable');
    expect(inboxRoutePlanSnapshot(p)?.archive.targets).toEqual([archiveOrigin]);
  } finally {
    f.close();
    peer.close();
  }
});
