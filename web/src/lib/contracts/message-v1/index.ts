import { canonicalPublicKey } from '../public-key.ts';
import { boundedUtf8 } from '../food-availability-v1/text.ts';
// Pure projections of Lib189 Message. No signature, participant, relay access,
// persistence or sending permission follows from these detached data values.
export type MessageRecipient = Readonly<{
  public_key: string;
  relay_url: string | null;
}>;
export type MessageReply = Readonly<{ id: string; relays: string | null }>;
export type MessageV1 = Readonly<{
  recipients: readonly MessageRecipient[];
  content: string;
  reply_to: MessageReply | null;
  subject: string | null;
}>;
export type MessageWireParts = Readonly<{
  kind: 14;
  tags: readonly (readonly string[])[];
  content: string;
}>;
const bodyBytes = 4096,
  partsBytes = 8192;
// Rust str::trim uses Unicode White_Space, unlike JavaScript trim's BOM rule.
const present = (value: string) => /\P{White_Space}/u.test(value);
const body = (value: unknown): value is string =>
  typeof value === 'string' && boundedUtf8(value, bodyBytes) && present(value);
const text = (value: unknown): value is string =>
  typeof value === 'string' && boundedUtf8(value, partsBytes) && present(value);
const hash = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
function fields(
  value: unknown,
  required: string[],
  optional: string[] = []
): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    return false;
  for (const key in value)
    if (!required.includes(key) && !optional.includes(key)) return false;
  return required.every((key) => key in value);
}
function parse(raw: unknown): unknown {
  if (typeof raw !== 'string' || !boundedUtf8(raw, partsBytes))
    return undefined;
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return undefined;
  }
}
function recipient(value: unknown): MessageRecipient | undefined {
  if (!fields(value, ['public_key'], ['relay_url'])) return undefined;
  const key = canonicalPublicKey(value.public_key),
    hint = value.relay_url ?? null;
  if (!key || (hint !== null && !text(hint))) return undefined;
  return { public_key: key, relay_url: hint };
}
function reply(value: unknown): MessageReply | null | undefined {
  if (value === null || value === undefined) return null;
  if (!fields(value, ['id'], ['relays']) || !hash(value.id)) return undefined;
  const hint = value.relays ?? null;
  if (hint !== null && !text(hint)) return undefined;
  return { id: value.id, relays: hint };
}
function subject(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;
  return text(value) ? value : undefined;
}
function message(value: unknown): MessageV1 | undefined {
  if (
    !fields(value, ['recipients', 'content'], ['reply_to', 'subject']) ||
    !body(value.content) ||
    !Array.isArray(value.recipients) ||
    !value.recipients.length
  )
    return undefined;
  const rows: unknown[] = value.recipients;
  const recipients = rows.map(recipient);
  if (
    !recipients.every(
      (person): person is MessageRecipient => person !== undefined
    )
  )
    return undefined;
  const parent = reply(value.reply_to),
    title = subject(value.subject);
  if (parent === undefined || title === undefined) return undefined;
  return {
    recipients,
    content: value.content,
    reply_to: parent,
    subject: title
  };
}
export function messageToWireParts(raw: unknown): MessageWireParts | undefined {
  const value = message(parse(raw));
  if (!value) return undefined;
  const tags: string[][] = [
    ...value.recipients.map((person) =>
      person.relay_url === null
        ? ['p', person.public_key]
        : ['p', person.public_key, person.relay_url]
    ),
    ...(value.reply_to
      ? [
          value.reply_to.relays === null
            ? ['e', value.reply_to.id]
            : ['e', value.reply_to.id, value.reply_to.relays]
        ]
      : []),
    ...(value.subject !== null ? [['subject', value.subject]] : [])
  ];
  const parts = {
    kind: 14 as const,
    tags,
    content: value.content
  };
  return boundedUtf8(JSON.stringify(parts), partsBytes) ? parts : undefined;
}
function tag(value: unknown): value is string[] {
  if (!Array.isArray(value)) return false;
  const values: unknown[] = value;
  return values.every(
    (item) => typeof item === 'string' && item.isWellFormed()
  );
}
export function messageFromWireParts(raw: unknown): MessageV1 | undefined {
  const value = parse(raw);
  if (
    !fields(value, ['kind', 'tags', 'content']) ||
    value.kind !== 14 ||
    !body(value.content) ||
    !Array.isArray(value.tags)
  )
    return undefined;
  const rows: unknown[] = value.tags;
  if (!rows.every(tag)) return undefined;
  const tags = rows;
  const recipients = tags
    .filter((row) => row[0] === 'p')
    .map((row) =>
      recipient({
        public_key: row[1],
        relay_url: row[2] ?? null
      })
    );
  if (
    !recipients.length ||
    !recipients.every(
      (person): person is MessageRecipient => person !== undefined
    )
  )
    return undefined;
  const parentTag = tags.find((row) => row[0] === 'e'),
    titleTag = tags.find((row) => row[0] === 'subject');
  const parent = parentTag
    ? reply({ id: parentTag[1], relays: parentTag[2] ?? null })
    : null;
  const title = titleTag ? subject(titleTag[1] ?? '') : null;
  if (parent === undefined || title === undefined) return undefined;
  return {
    recipients,
    content: value.content,
    reply_to: parent,
    subject: title
  };
}
