import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  messageFromWireParts,
  type MessageV1
} from '../contracts/message-v1/index.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
import { boundedEnvelopeTags } from './verified-envelope.ts';

// Syntax-only duplicate detection precedes ordinary JSON semantics. Escaped
// field aliases count as the same key; harmless whitespace stays supported.
function uniqueJSONFields(raw: string): boolean {
  type Frame = {
    array: boolean;
    key: boolean;
    parent: Frame | undefined;
    accept: (name: string) => boolean;
  };
  function frame(parent: Frame | undefined, array: boolean): Frame {
    const names = new Set<string>();
    return {
      parent,
      array,
      key: !array,
      accept: (name: string) => {
        if (names.has(name)) return false;
        names.add(name);
        return true;
      }
    };
  }
  let current: Frame | undefined;
  try {
    for (let index = 0; index < raw.length; index++) {
      const character = raw[index];
      if (character === '"') {
        let end = index + 1;
        while (end < raw.length) {
          if (raw[end] === '\\') {
            end += 2;
            continue;
          }
          if (raw[end] === '"') break;
          end++;
        }
        if (end >= raw.length) return false;
        if (current && !current.array && current.key) {
          const name: unknown = JSON.parse(raw.slice(index, end + 1));
          if (typeof name !== 'string' || !current.accept(name)) return false;
          current = {
            parent: current.parent,
            array: current.array,
            accept: current.accept,
            key: false
          };
        }
        index = end;
      } else if (character === '{' || character === '[')
        current = frame(current, character === '[');
      else if (character === '}' || character === ']') {
        if (!current) return false;
        current = current.parent;
      } else if (character === ',' && current && !current.array)
        current = {
          parent: current.parent,
          array: current.array,
          accept: current.accept,
          key: true
        };
    }
    return current === undefined;
  } catch {
    return false;
  }
}
export function parseIncomingLayer(raw: unknown, bytes: number): unknown {
  if (
    typeof raw !== 'string' ||
    !boundedUtf8(raw, bytes) ||
    !uniqueJSONFields(raw)
  )
    return undefined;
  try {
    let invalid = false;
    const parsed: unknown = JSON.parse(
      raw,
      function (key: string, value: unknown, context?: { source?: string }) {
        if (
          !key.isWellFormed() ||
          (typeof value === 'string' && !value.isWellFormed())
        )
          invalid = true;
        if (
          typeof value === 'number' &&
          (!Number.isFinite(value) ||
            ((key === 'kind' || key === 'created_at') &&
              (context?.source === undefined ||
                !/^[0-9]+$/u.test(context.source))))
        )
          invalid = true;
        return value;
      }
    );
    return invalid ? undefined : parsed;
  } catch {
    return undefined;
  }
}
// Receive-only syntax/projection. These detached values grant no signature,
// nested custody, routing, persistence, publication or conversation authority.
export function incomingGiftWrapTags(
  tags: unknown,
  expectedOwner: unknown
): boolean {
  const owner = canonicalPublicKey(expectedOwner);
  if (
    !owner ||
    !boundedEnvelopeTags(tags) ||
    tags.some((row) => row[0] === 'expiration')
  )
    return false;
  const recipients = tags.filter((row) => row[0] === 'p');
  return (
    recipients.length === 1 &&
    (recipients[0].length === 2 || recipients[0].length === 3) &&
    recipients[0][1] === owner
  );
}

export type IncomingPairRumor = Readonly<{
  original: Readonly<UnsignedEvent & { id: string }>;
  message: MessageV1;
}>;

// Hash the original ordered fields. Semantic projection is deliberately
// independent of the strict outgoing writer; hints and unknown tags grant no effects.
export function readIncomingPairRumor(
  raw: unknown,
  expectedSender?: unknown
): IncomingPairRumor | undefined {
  const parsed = parseIncomingLayer(raw, PRIVATE_TRANSPORT_BUDGETS.rumorBytes);
  if (
    !exactLocalFields(parsed, [
      'id',
      'pubkey',
      'created_at',
      'kind',
      'tags',
      'content'
    ]) ||
    parsed.kind !== 14 ||
    typeof parsed.id !== 'string' ||
    !/^[0-9a-f]{64}$/u.test(parsed.id) ||
    safeUnsignedInteger(parsed.created_at) === undefined ||
    !boundedEnvelopeTags(parsed.tags)
  )
    return undefined;
  const sender = canonicalPublicKey(parsed.pubkey);
  if (
    !sender ||
    (expectedSender !== undefined &&
      canonicalPublicKey(expectedSender) !== sender)
  )
    return undefined;
  const tags = parsed.tags;
  // Unsupported retention/attachment semantics never become inert acceptance.
  if (tags.some((row) => ['expiration', 'file', 'imeta'].includes(row[0])))
    return undefined;
  const recipients = tags.filter((row) => row[0] === 'p'),
    parents = tags.filter((row) => row[0] === 'e'),
    subjects = tags.filter((row) => row[0] === 'subject');
  if (
    recipients.length !== 1 ||
    ![2, 3].includes(recipients[0].length) ||
    parents.length > 1 ||
    parents.some((row) => ![2, 3].includes(row.length)) ||
    subjects.length > 1 ||
    subjects.some((row) => row.length !== 2)
  )
    return undefined;
  const decoded = messageFromWireParts(
    JSON.stringify({ kind: 14, tags, content: parsed.content })
  );
  if (
    !decoded ||
    decoded.recipients.length !== 1 ||
    decoded.recipients[0].public_key === sender
  )
    return undefined;
  const template: UnsignedEvent = {
    pubkey: sender,
    kind: 14,
    created_at: parsed.created_at as number,
    tags: tags.map((row) => [...row]),
    content: decoded.content
  };
  if (getEventHash(template) !== parsed.id) return undefined;
  return { original: { id: parsed.id, ...template }, message: decoded };
}
