// One approved public inventory for observations, SDK storage and retention.
export function isPublicEnvelopeKind(kind: unknown): kind is number {
  return typeof kind === 'number' && [0, 5, 10050, 30402].includes(kind);
}
