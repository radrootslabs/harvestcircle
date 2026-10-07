import type { UnsignedEvent } from 'applesauce-core/helpers';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  bindCapturedArtifact,
  type CapturedArtifact
} from '../persistence/artifact-records.ts';
import { inspectLocalTemplate } from './local-template.ts';
import { boundedEnvelopeNumbers } from './envelope-bounds.ts';

declare const approvalBrand: unique symbol;
export type ApprovedPublicSigning = Readonly<{ [approvalBrand]: true }>;
type Capture = Readonly<{
  owner: string;
  id: string;
  revision: number;
  wire: string;
  hash: string;
  recordWire: string;
}>;
const approvals = new WeakMap<ApprovedPublicSigning, Capture>();
// Internal captured-operation capability. This is neither a raw UI template
// signer nor current IDB/CAS, inbox-readiness, transport or publication proof.
// Later operation controllers own those obligations. Private seal13/AUTH22242
// approvals require their separate typed owners; plaintext14 is never admitted.
export function approveCapturedPublicSigning(
  record: PublicRecordHandle,
  owner: unknown,
  id: unknown,
  review: unknown
): ApprovedPublicSigning | undefined {
  if (review !== 'reviewed_captured_operation') return undefined;
  const row = publicRecordSnapshot(record, owner, id);
  const recordWire = publicRecordWire(record, owner, id);
  if (
    !row ||
    !recordWire ||
    row.family === 'public_draft' ||
    row.artifact !== null
  )
    return undefined;
  const template = inspectLocalTemplate(
    row.capture.wire,
    row.owner,
    row.capture.kind
  );
  if (!template || template.hash !== row.capture.hash) return undefined;
  // Retain primitive canonical wire/hash independently, never caller arrays.
  const token = Object.freeze({}) as ApprovedPublicSigning;
  approvals.set(token, {
    owner: row.owner,
    id: row.id,
    revision: row.revision,
    wire: row.capture.wire,
    hash: row.capture.hash,
    recordWire
  });
  return token;
}
export function approvedSigningIdentity(approval: ApprovedPublicSigning):
  | Readonly<{
      owner: string;
      id: string;
      revision: number;
      recordWire: string;
    }>
  | undefined {
  const saved = approvals.get(approval);
  return saved
    ? {
        owner: saved.owner,
        id: saved.id,
        revision: saved.revision,
        recordWire: saved.recordWire
      }
    : undefined;
}
// Each acquisition is a disposable fresh JSON copy. No retained template or
// mutable SDK verification symbol ever crosses the provider boundary.
export function disposableApprovedTemplate(
  approval: ApprovedPublicSigning
): UnsignedEvent | undefined {
  const saved = approvals.get(approval);
  return saved ? (JSON.parse(saved.wire) as UnsignedEvent) : undefined;
}
function own(value: object, name: string): unknown {
  const descriptor = Object.getOwnPropertyDescriptor(value, name);
  return descriptor && 'value' in descriptor ? descriptor.value : undefined;
}
// Bounded reconstruction, using only own data fields and indexed array entries.
// Do not spread/serialize the response, enumerate extras, invoke toJSON, acquire
// accessor values, or follow provider-controlled iterators and cache symbols.
function signedWire(raw: unknown): string | undefined {
  try {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw))
      return undefined;
    const id = own(raw, 'id'),
      pubkey = own(raw, 'pubkey'),
      sig = own(raw, 'sig'),
      numbers = boundedEnvelopeNumbers(
        own(raw, 'kind'),
        own(raw, 'created_at')
      ),
      content = own(raw, 'content'),
      sourceTags = own(raw, 'tags');
    if (
      typeof id !== 'string' ||
      !/^[0-9a-f]{64}$/.test(id) ||
      typeof pubkey !== 'string' ||
      !/^[0-9a-f]{64}$/.test(pubkey) ||
      typeof sig !== 'string' ||
      !/^[0-9a-f]{128}$/.test(sig) ||
      !numbers ||
      typeof content !== 'string' ||
      !boundedUtf8(content, 131072) ||
      !Array.isArray(sourceTags)
    )
      return undefined;
    const count = own(sourceTags, 'length');
    if (
      typeof count !== 'number' ||
      !Number.isSafeInteger(count) ||
      count < 0 ||
      count > 1024
    )
      return undefined;
    const tags: string[][] = [];
    let elements = 0,
      bytes = 0;
    const encoder = new TextEncoder();
    for (let i = 0; i < count; i++) {
      const source = own(sourceTags, String(i));
      if (!Array.isArray(source)) return undefined;
      const length = own(source, 'length');
      if (
        typeof length !== 'number' ||
        !Number.isSafeInteger(length) ||
        length <= 0 ||
        length > 4096 - elements
      )
        return undefined;
      elements += length;
      const tag: string[] = [];
      function append(value: string) {
        tag.push(value);
      }
      for (let j = 0; j < length; j++) {
        const value = own(source, String(j));
        if (
          typeof value !== 'string' ||
          !boundedUtf8(value, 4096) ||
          (j === 0 && (value.length === 0 || /\p{Cc}/u.test(value)))
        )
          return undefined;
        bytes += encoder.encode(value).length;
        if (bytes > 131072) return undefined;
        append(value);
      }
      tags.push(tag);
    }
    return JSON.stringify({
      id,
      pubkey,
      kind: numbers.kind,
      created_at: numbers.created_at,
      tags,
      content,
      sig
    });
  } catch {
    return undefined;
  }
}
export function bindApprovedResponse(
  approval: ApprovedPublicSigning,
  response: unknown
): CapturedArtifact | undefined {
  const saved = approvals.get(approval);
  if (!saved) return undefined;
  const wire = signedWire(response);
  if (!wire) return undefined;
  const returned = JSON.parse(wire) as UnsignedEvent;
  const original = JSON.parse(saved.wire) as UnsignedEvent;
  if (
    returned.pubkey !== original.pubkey ||
    returned.kind !== original.kind ||
    returned.created_at !== original.created_at ||
    returned.content !== original.content ||
    JSON.stringify(returned.tags) !== JSON.stringify(original.tags)
  )
    return undefined;
  // Reparse into a fresh SDK-verification object: no provider cache survives.
  // Hash and signature must both verify, independently of field comparison.
  const bound = bindCapturedArtifact(
    wire,
    saved.owner,
    original.kind,
    saved.hash
  );
  return bound.ok ? bound.value : undefined;
}
