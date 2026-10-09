import {
  stopPrivateRetry,
  privateRetrySnapshot,
  type PrivateRetry
} from './retry-send.ts';
// Stop prevents future scheduling. It does not retract a relay attempt or
// acknowledge recipient delivery. Original attempt facts reconcile separately.
export function stopPrivateSend(operation: PrivateRetry): void {
  stopPrivateRetry(operation);
}
export function privateStopSendSnapshot(operation: PrivateRetry) {
  const view = privateRetrySnapshot(operation);
  return (
    view && {
      ...view,
      labels:
        view.state === 'stopped'
          ? [
              'Stopped',
              'Already attempted delivery may remain uncertain',
              'Resume requires an explicit matching-account action'
            ]
          : []
    }
  );
}
