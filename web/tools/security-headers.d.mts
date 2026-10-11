import type { RelayPolicy } from '../src/lib/config/relays.ts';

export interface SecurityHeadersPreparation {
  schema: 'harvestcircle.static_security_headers.v1';
  readiness: 'SOURCE_PREPARATION_ONLY_HOST_QUALIFICATION_PENDING';
  relay_policy_sha256: string;
  build_info_sha256: string;
  compiler_manifest_sha256: string;
  framework_style_attributes: {
    owner: string;
    file: string;
    source_sha256: string;
    style_sha256: string;
    csp_hash: string;
  }[];
  connect_sources: string[];
  inline_scripts: Record<string, string[]>;
  artifacts: Record<string, { sha256: string; bytes: number }>;
  headers: Record<string, string>;
}
export function relayConnectSources(policy: RelayPolicy): string[];
export function extractFrameworkStyleAttributes(
  file: string,
  source: Uint8Array,
  expectedSourceSha256: string
): SecurityHeadersPreparation['framework_style_attributes'];
export function prepareSecurityHeaders(
  webDirectory: string
): Promise<SecurityHeadersPreparation>;
export function verifySecurityHeaders(
  webDirectory: string,
  candidate: unknown
): Promise<void>;
