import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  closePublicRequest,
  cancelPublicRun,
  disposePublicRun,
  closePublicScheduler,
  publicRunSnapshot,
  publicRequestScopeSnapshot,
  publicSchedulerSnapshot,
  publicRunObservations,
  type RequestClock
} from '../../src/lib/nostr/request-scope.ts';
const origin = 'wss://one.example.org';
const policy = validateRelayPolicy(
  JSON.stringify({
    schemaVersion: 1,
    public: [{ origin, read: true, write: false, nip50: false }],
    inbox: [],
    postingEnabled: false,
    messagingEnabled: false,
    operatorDenylist: []
  })
)!;
const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const wire = corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires
  .current;
const event = () =>
  JSON.parse(wire) as Extract<PublicPoolMessage, { type: 'EVENT' }>['event'];
function fixture() {
  let now = 0;
  const timers = new Map<() => void, { at: number; callback: () => void }>();
  const clock: RequestClock = {
    now: () => now,
    schedule(callback, delay) {
      const cancel = () => {
        timers.delete(cancel);
      };
      timers.set(cancel, { at: now + delay, callback });
      return cancel;
    }
  };
  const scheduler = createPublicScheduler(clock);
  return {
    scheduler,
    advance(value: number, flush = true) {
      now = value;
      if (flush)
        for (const [cancel, row] of [...timers])
          if (row.at <= now) {
            cancel();
            row.callback();
          }
    },
    timers
  };
}
function channel() {
  let sink: (message: PublicPoolMessage) => void = () => {};
  let stopped = 0;
  let opened = 0;
  return {
    open(this: void, next: (message: PublicPoolMessage) => void) {
      opened++;
      sink = next;
      return () => {
        stopped++;
      };
    },
    emit(message: PublicPoolMessage) {
      sink(message);
    },
    counts: () => ({ opened, stopped })
  };
}
describe('owned public request scopes', () => {
  it('admits only six aggregate primary and auxiliary scopes before effects', () => {
    const f = fixture(),
      a = createPublicRun(f.scheduler, policy),
      b = createPublicRun(f.scheduler, policy),
      c = channel();
    const requests = Array.from({ length: 6 }, (_, i) =>
      openPublicRequest(
        i < 3 ? a : b,
        ['search', 'head', 'deletion', 'profile'][i % 4] as
          'search' | 'head' | 'deletion' | 'profile',
        c.open,
        () => {}
      )
    );
    expect(() => openPublicRequest(b, 'profile', c.open, () => {})).toThrow(
      'public_request_concurrency_limit'
    );
    expect(c.counts().opened).toBe(6);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(6);
    closePublicRequest(requests[0]);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(5);
    expect(publicRequestScopeSnapshot(requests[1]).state).toBe('active');
    openPublicRequest(b, 'head', c.open, () => {});
    closePublicScheduler(f.scheduler);
    expect(f.timers.size).toBe(0);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
  });
  it('caps each request at ten seconds and never extends the absolute run deadline', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel();
    const first = openPublicRequest(run, 'search', c.open, () => {});
    expect(publicRunSnapshot(run).deadline).toBe(15000);
    expect(publicRequestScopeSnapshot(first).deadline).toBe(10000);
    f.advance(10000);
    expect(publicRequestScopeSnapshot(first).state).toBe('deadline');
    f.advance(14000);
    const late = openPublicRequest(run, 'profile', c.open, () => {});
    expect(publicRequestScopeSnapshot(late).deadline).toBe(15000);
    f.advance(15000);
    expect(publicRequestScopeSnapshot(late).state).toBe('deadline');
    expect(() => openPublicRequest(run, 'head', c.open, () => {})).toThrow(
      'public_run_inactive'
    );
    expect(c.counts().stopped).toBe(2);
  });
  it('checks absolute time on callbacks even when timer dispatch is delayed', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel();
    let writes = 0;
    const request = openPublicRequest(run, 'search', c.open, () => {
      writes++;
    });
    f.advance(10001, false);
    c.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(writes).toBe(0);
    expect(publicRequestScopeSnapshot(request).state).toBe('deadline');
    expect(publicRunSnapshot(run).ingress.deliveries).toBe(0);
    expect(f.timers.size).toBe(0);
  });
  it('cancellation invalidates late writes and leaves another owner running', () => {
    const f = fixture(),
      a = createPublicRun(f.scheduler, policy),
      b = createPublicRun(f.scheduler, policy),
      ca = channel(),
      cb = channel();
    let writes = 0;
    const ra = openPublicRequest(a, 'search', ca.open, () => {
        writes++;
      }),
      rb = openPublicRequest(b, 'profile', cb.open, () => {
        writes++;
      });
    cancelPublicRun(a);
    ca.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    cb.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(writes).toBe(1);
    expect(publicRequestScopeSnapshot(ra).state).toBe('cancelled');
    expect(publicRequestScopeSnapshot(rb).state).toBe('active');
    expect(cb.counts().stopped).toBe(0);
    closePublicScheduler(f.scheduler);
  });
  it('shares duplicate and rejected candidate charges across auxiliary scopes', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      a = channel(),
      b = channel();
    let writes = 0;
    const ra = openPublicRequest(run, 'search', a.open, () => {
        writes++;
      }),
      rb = openPublicRequest(run, 'deletion', b.open, () => {
        writes++;
      });
    a.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    b.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    b.emit({
      type: 'EVENT',
      id: 'fixture',
      from: origin,
      event: { ...event(), sig: '0'.repeat(128) }
    });
    expect(publicRunSnapshot(run).ingress.deliveries).toBe(3);
    expect(writes).toBe(2);
    expect(publicRunObservations(run, ra)).toHaveLength(1);
    expect(publicRunObservations(run, rb)).toHaveLength(1);
    closePublicScheduler(f.scheduler);
  });
  it('finishes a bounded source at EOSE while preserving exact evidence', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel();
    const request = openPublicRequest(run, 'head', c.open, () => {});
    c.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    c.emit({ type: 'EOSE', id: 'fixture', from: origin });
    expect(publicRequestScopeSnapshot(request).state).toBe('eose');
    expect(publicRequestScopeSnapshot(request).result.definitiveAbsence).toBe(
      false
    );
    expect(publicRunObservations(run, request)).toHaveLength(1);
    expect(c.counts().stopped).toBe(1);
    cancelPublicRun(run);
    expect(publicRunObservations(run, request)).toHaveLength(1);
    disposePublicRun(run);
    expect(publicRunObservations(run, request)).toEqual([]);
  });
  it('cleans a synchronously completed open without orphaning its stop control', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy);
    let stops = 0;
    const request = openPublicRequest(
      run,
      'search',
      (next) => {
        next({ type: 'EOSE', id: 'fixture', from: origin });
        return () => {
          stops++;
        };
      },
      () => {}
    );
    expect(publicRequestScopeSnapshot(request).state).toBe('eose');
    expect(stops).toBe(1);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    expect(f.timers.size).toBe(0);
  });
  it('cleans rejected open and callback failures with safe outcome text', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy);
    expect(() =>
      openPublicRequest(
        run,
        'search',
        () => {
          throw new Error('PRIVATE_RAW_ERROR');
        },
        () => {}
      )
    ).toThrow('public_request_open_failed');
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    expect(f.timers.size).toBe(0);
    const c = channel();
    const request = openPublicRequest(run, 'profile', c.open, () => {
      throw new Error('PRIVATE_UI_ERROR');
    });
    c.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(publicRequestScopeSnapshot(request).state).toBe('error');
    expect(JSON.stringify(publicRequestScopeSnapshot(request))).not.toMatch(
      /PRIVATE/
    );
  });
  it('rejects forged handles and another run observation context', () => {
    const f = fixture(),
      a = createPublicRun(f.scheduler, policy),
      b = createPublicRun(f.scheduler, policy),
      c = channel();
    const request = openPublicRequest(a, 'search', c.open, () => {});
    expect(Object.keys(request)).toEqual([]);
    expect(Object.isFrozen(request)).toBe(true);
    expect(() => publicRunObservations(b, request)).toThrow(
      'public_request_run_changed'
    );
    expect(() => closePublicRequest({} as typeof request)).toThrow(
      'public_request_invalid'
    );
    closePublicScheduler(f.scheduler);
    expect(() => createPublicRun(f.scheduler, policy)).toThrow(
      'public_scheduler_closed'
    );
  });
  it('retains failed cleanup admission until explicit retry and stops successful controls once', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy);
    let stops = 0;
    const request = openPublicRequest(
      run,
      'search',
      () => () => {
        stops++;
        if (stops === 1) throw new Error('RAW_CLEANUP_ERROR');
      },
      () => {}
    );
    expect(() => closePublicRequest(request)).toThrow(
      'public_request_close_failed'
    );
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(1);
    expect(publicRequestScopeSnapshot(request).state).toBe('cancelled');
    closePublicRequest(request);
    closePublicRequest(request);
    expect(stops).toBe(2);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    expect(f.timers.size).toBe(0);
    closePublicScheduler(f.scheduler);
  });
  it('lets the inclusive two-thousandth candidate finish then cancels every auxiliary scope', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      a = channel(),
      b = channel();
    let writes = 0;
    const ra = openPublicRequest(run, 'search', a.open, () => {
        writes++;
      }),
      rb = openPublicRequest(run, 'profile', b.open, () => {
        writes++;
      });
    for (let i = 0; i < 2000; i++)
      (i % 2 === 0 ? a : b).emit({
        type: 'EVENT',
        id: 'fixture',
        from: origin,
        event: event()
      });
    expect(writes).toBe(2000);
    expect(publicRunSnapshot(run).ingress.deliveries).toBe(2000);
    expect(publicRunSnapshot(run).ingress.stopped).toBe(true);
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    expect(publicRequestScopeSnapshot(rb).state).toBe('limit');
    expect(publicRequestScopeSnapshot(ra).state).toBe('cancelled');
    expect(f.timers.size).toBe(0);
    expect(publicRunObservations(run, ra)).toHaveLength(1);
    expect(publicRunObservations(run, rb)).toHaveLength(1);
    a.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(writes).toBe(2000);
    disposePublicRun(run);
  });
  it('does not extend captured deadlines when an injected clock moves backwards', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel();
    f.advance(5000);
    const request = openPublicRequest(run, 'head', c.open, () => {});
    expect(publicRequestScopeSnapshot(request).deadline).toBe(15000);
    f.advance(1000, false);
    expect(publicRunSnapshot(run).deadline).toBe(15000);
    c.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(publicRequestScopeSnapshot(request).state).toBe('active');
    f.advance(15000);
    expect(publicRequestScopeSnapshot(request).state).toBe('deadline');
    closePublicScheduler(f.scheduler);
  });
  it('fails closed on invalid clock values and returns detached outcome snapshots', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel();
    let writes = 0;
    const request = openPublicRequest(run, 'search', c.open, () => {
      writes++;
    });
    const snapshot = publicRequestScopeSnapshot(request);
    (snapshot as unknown as { state: string }).state = 'eose';
    expect(publicRequestScopeSnapshot(request).state).toBe('active');
    f.advance(Number.NaN, false);
    c.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(writes).toBe(0);
    expect(publicRequestScopeSnapshot(request).state).toBe('error');
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    closePublicScheduler(f.scheduler);
  });
  it('retries only a failed timer cancellation without repeating successful subscription cleanup', () => {
    let timerCalls = 0,
      stopCalls = 0;
    const clock: RequestClock = {
      now: () => 0,
      schedule: () => () => {
        timerCalls++;
        if (timerCalls === 1) throw new Error('timer_failed');
      }
    };
    const scheduler = createPublicScheduler(clock),
      run = createPublicRun(scheduler, policy),
      request = openPublicRequest(
        run,
        'search',
        () => () => {
          stopCalls++;
        },
        () => {}
      );
    expect(() => closePublicRequest(request)).toThrow(
      'public_request_close_failed'
    );
    expect(stopCalls).toBe(1);
    expect(publicSchedulerSnapshot(scheduler).activeRequests).toBe(1);
    closePublicRequest(request);
    expect(timerCalls).toBe(2);
    expect(stopCalls).toBe(1);
    expect(publicSchedulerSnapshot(scheduler).activeRequests).toBe(0);
    closePublicScheduler(scheduler);
  });
  it('cancels auxiliary work even when the inclusive final candidate view callback throws', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      a = channel(),
      b = channel();
    let calls = 0;
    const ra = openPublicRequest(run, 'search', a.open, () => {}),
      rb = openPublicRequest(run, 'profile', b.open, () => {
        calls++;
        throw new Error('final_callback_failed');
      });
    for (let i = 0; i < 1999; i++)
      a.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    b.emit({ type: 'EVENT', id: 'fixture', from: origin, event: event() });
    expect(calls).toBe(1);
    expect(publicRunSnapshot(run).ingress.deliveries).toBe(2000);
    expect(publicRequestScopeSnapshot(rb).state).toBe('error');
    expect(publicRequestScopeSnapshot(ra).state).toBe('cancelled');
    expect(publicSchedulerSnapshot(f.scheduler).activeRequests).toBe(0);
    expect(f.timers.size).toBe(0);
    expect(publicRunObservations(run, ra)).toHaveLength(1);
    expect(publicRunObservations(run, rb)).toHaveLength(1);
    closePublicScheduler(f.scheduler);
  });
  it('checks captured absolute time again when a deadline timer fires early', () => {
    const f = fixture(),
      run = createPublicRun(f.scheduler, policy),
      c = channel(),
      request = openPublicRequest(run, 'head', c.open, () => {});
    f.advance(9999, false);
    const [cancel, row] = [...f.timers][0];
    cancel();
    row.callback();
    expect(publicRequestScopeSnapshot(request).state).toBe('active');
    expect(f.timers.size).toBe(1);
    f.advance(10000);
    expect(publicRequestScopeSnapshot(request).state).toBe('deadline');
    expect(f.timers.size).toBe(0);
    closePublicScheduler(f.scheduler);
  });
});
