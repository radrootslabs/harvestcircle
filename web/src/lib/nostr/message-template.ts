import {
  messageToWireParts,
  type MessageWireParts
} from '../contracts/message-v1/index.ts';
// Detached unsigned14 parts only. The subsequent rumor owner binds the actual
// author, timestamp and SDK hash. This is never a public signing/publish token.
export function buildUnsignedMessageTemplate(
  raw: unknown
): MessageWireParts | undefined {
  return messageToWireParts(raw);
}
