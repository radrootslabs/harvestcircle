import { readFileSync } from 'node:fs';
import {
  messageFromWireParts,
  messageToWireParts,
  type MessageV1,
  type MessageWireParts
} from '../../src/lib/contracts/message-v1/index.ts';
export interface MessageVector {
  id: string;
  mode: 'write' | 'read';
  input: Record<string, unknown>;
  web_status: 'supported' | 'unsupported';
  policy_difference?: string;
  expected: {
    status: 'supported' | 'unsupported';
    wire_parts?: MessageWireParts;
    message?: MessageV1;
    error?: string;
  };
}
export interface MessageCorpus {
  revision: string;
  source_sha256: string;
  vectors: MessageVector[];
}
export function messageCorpus(): MessageCorpus {
  return JSON.parse(
    readFileSync(
      new URL(
        '../../../contracts/interop/message/corpus.v1.json',
        import.meta.url
      ),
      'utf8'
    )
  ) as MessageCorpus;
}
export function actualWebMessage(vector: MessageVector) {
  if (vector.mode === 'write') {
    const parts = messageToWireParts(JSON.stringify(vector.input));
    return parts
      ? {
          status: 'supported',
          wire_parts: parts,
          message: messageFromWireParts(JSON.stringify(parts))
        }
      : { status: 'unsupported' };
  }
  const value = messageFromWireParts(JSON.stringify(vector.input));
  return value
    ? { status: 'supported', message: value }
    : { status: 'unsupported' };
}
export function messageReplay() {
  return messageCorpus()
    .vectors.filter((v) => v.mode === 'write' && v.web_status === 'supported')
    .map((v) => ({
      id: v.id,
      wire_parts: messageToWireParts(JSON.stringify(v.input)),
      expected: v.expected.message
    }));
}
