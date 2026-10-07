export { makeFixture } from './approved-signing.ts';
export {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
export {
  createPublicQuotaRepository,
  claimPublicOperation,
  commitPublicOperationTransition,
  inspectPublicStorage,
  publicInventorySnapshot
} from '../../../src/lib/persistence/quota.ts';
export {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire
} from '../../../src/lib/persistence/records.ts';
export {
  preparePublicArtifactTransition,
  capturedArtifactSnapshot
} from '../../../src/lib/persistence/artifact-records.ts';
export { approveCapturedPublicSigning } from '../../../src/lib/nostr/approved-signing.ts';
export {
  createExtensionAdapter,
  connectExtensionAdapter,
  disconnectExtensionAdapter,
  extensionOwnershipCapture,
  signApprovedExtensionAdapter
} from '../../../src/lib/nostr/extension.ts';
export {
  runCapturedPublicEffect,
  publicEffectSnapshot,
  stopPublicEffect
} from '../../../src/lib/runtime/effect-ownership.ts';
