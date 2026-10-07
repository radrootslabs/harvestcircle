// Local record vocabulary, not Nostr schemas, installed identity or effect
// capabilities. Storage codecs admit structure; repositories own CAS/effects.
// JSON-shaped own enumerable data fields only; no getters are acquired.
export function exactLocalFields(
  value: unknown,
  fields: readonly string[]
): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    return false;
  let count = 0;
  function own(key: string): boolean {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    return (
      descriptor !== undefined &&
      descriptor.get === undefined &&
      descriptor.set === undefined
    );
  }
  for (const key in value) {
    if (!fields.includes(key) || !own(key)) return false;
    count++;
  }
  return count === fields.length;
}
export type PublicDraftForm = Readonly<{
  title: string;
  description: string;
  location: string;
  amount: string;
  currency: string;
  unit: string;
  quantity: string;
  contactType: '' | 'email' | 'phone' | 'https';
  contactValue: string;
}>;
export type PublicDraftRecord = Readonly<{
  schema: 1;
  family: 'public_draft';
  owner: string;
  id: string;
  revision: number;
  savedAtMilliseconds: number;
  form: PublicDraftForm;
}>;
export type PublicCaptureSource =
  | Readonly<{ type: 'draft'; id: string; revision: number }>
  | Readonly<{ type: 'public_head'; wire: string }>;
export type PreferenceCaptureSource = Readonly<{
  type: 'inbox_head';
  // Null retains no base event; it never proves global absence/lookup readiness.
  wire: string | null;
}>;
export type CapturedTemplate<Kind extends 30402 | 5 | 10050> = Readonly<{
  kind: Kind;
  wire: string;
  hash: string;
  targets: readonly string[];
  policyFingerprint: string;
}>;
export type SignedPublicArtifact = Readonly<{
  eventId: string;
  wire: string;
}>;
// Recorded local observation metadata, not independently proven relay/operator
// evidence. Exact readback and a named ACK remain distinct, even when both exist.
export type PublicTargetReceipt = Readonly<{
  actionId: string;
  origin: string;
  role: 'publication' | 'preference';
  attempt: number;
  eventId: string;
  status: 'accepted' | 'refused' | 'timed_out' | 'unknown' | 'stopped';
  observedAtMilliseconds: number;
  readbackWire: string | null;
  // Preference-only actual discovery source; origin remains the ACK write target.
  // Absence is legacy metadata, never an inferred cross-source observation.
  readbackOrigin?: string;
}>;
export type PublicOperationRecord = Readonly<{
  schema: 1;
  family: 'public_operation';
  owner: string;
  id: string;
  revision: number;
  source: PublicCaptureSource;
  capture: CapturedTemplate<30402 | 5>;
  artifact: SignedPublicArtifact | null;
  receipts: readonly PublicTargetReceipt[];
}>;
export type PreferenceOperationRecord = Readonly<{
  schema: 1;
  family: 'preference_operation';
  owner: string;
  id: string;
  revision: number;
  source: PreferenceCaptureSource;
  consent: 'explicit_review';
  capture: CapturedTemplate<10050>;
  artifact: SignedPublicArtifact | null;
  receipts: readonly PublicTargetReceipt[];
}>;
// Private body/subject/contact/product associations have no member in this
// public union. Encrypted private-send records retain their own later owner.
export type PublicRecord =
  PublicDraftRecord | PublicOperationRecord | PreferenceOperationRecord;
export type ConversationMappingRecord = Readonly<{
  schema: 1;
  family: 'conversation_handle';
  owner: string;
  id: string;
  peer: string;
}>;
