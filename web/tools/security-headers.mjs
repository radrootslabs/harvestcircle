import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { constants } from 'node:fs';
import { open, readFile, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { auditOutput } from './check-output.mjs';
import { deploymentRelayPolicy } from '../src/lib/config/deployment-relays.ts';
import { readRelayPolicy } from '../src/lib/config/relays.ts';

const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

// Only the retained opaque admission can supply origins. Hints and copied
// views are never accepted. This pure observation grants no launch authority.
export function relayConnectSources(policy) {
  const manifest = readRelayPolicy(policy);
  return [
    ...new Set(
      [...manifest.public, ...manifest.inbox].map(({ origin }) => origin)
    )
  ].sort();
}

async function readBuilt(web, name) {
  return readBoundFile(path.join(web, 'build', name));
}

async function readBoundFile(file) {
  if ((await realpath(file)) !== file)
    throw new Error('security_headers_unsafe_input');
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.nlink !== 1 || stat.size > 8 * 1024 * 1024)
      throw new Error('security_headers_unsafe_input');
    const bytes = await handle.readFile();
    if (bytes.length !== stat.size || bytes.length > 8 * 1024 * 1024)
      throw new Error('security_headers_unsafe_input');
    return bytes;
  } finally {
    await handle.close();
  }
}

async function frameworkStyles(web, artifacts) {
  const manifestFile = path.join(
    web,
    '.svelte-kit/output/client/.vite/manifest.json'
  );
  const manifestBytes = await readBoundFile(manifestFile);
  const entries = Object.values(
    JSON.parse(manifestBytes.toString('utf8'))
  ).filter((entry) => entry.name === 'navigation');
  if (entries.length !== 1 || !Object.hasOwn(artifacts, entries[0].file))
    throw new Error('security_headers_unreviewed_style_shape');
  const file = entries[0].file;
  const source = await readBuilt(web, file);
  const styles = extractFrameworkStyleAttributes(
    file,
    source,
    artifacts[file].sha256
  );
  return { manifestFile, manifest_sha256: digest(manifestBytes), styles };
}

// This finite value permission applies to any element with the exact hash;
// CSP cannot restrict a style hash to the announcer node.
export function extractFrameworkStyleAttributes(
  file,
  source,
  expectedSourceSha256
) {
  if (digest(source) !== expectedSourceSha256)
    throw new Error('security_headers_stale_or_invalid');
  const parsed = ts.createSourceFile(
    file,
    new TextDecoder('utf-8', { fatal: true }).decode(source),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.JS
  );
  if (parsed.parseDiagnostics.length)
    throw new Error('security_headers_unreviewed_style_shape');
  const styles = [];
  function visit(node) {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      if (/style\s*=/.test(node.text)) {
        const match =
          /^<div id="svelte-announcer" aria-live="assertive" aria-atomic="true" style="([^"]+)"><!><\/div>$/.exec(
            node.text
          );
        if (
          !match ||
          digest(match[1]) !==
            '4bca8ca6fa1fa2547c3298f2e2442f126ee6d6af1c97353875f0c7d009af663a'
        )
          throw new Error('security_headers_unreviewed_style_shape');
        styles.push({
          owner: 'svelte-announcer',
          file,
          source_sha256: digest(source),
          style_sha256: digest(match[1]),
          csp_hash:
            "'sha256-" +
            createHash('sha256').update(match[1]).digest('base64') +
            "'"
        });
      }
    } else if (
      ts.isTemplateExpression(node) &&
      /style\s*=|svelte-announcer/.test(node.getText(parsed))
    ) {
      throw new Error('security_headers_unreviewed_style_shape');
    }
    ts.forEachChild(node, visit);
  }
  visit(parsed);
  if (styles.length !== 1)
    throw new Error('security_headers_unreviewed_style_shape');
  return styles;
}

// Host-neutral SOURCE preparation, bound to the actual production source and
// audited compiler bytes. No alternate test policy can substitute for the
// policy compiled into this build. HTTPS/host/extension qualification is Q.
export async function prepareSecurityHeaders(webDirectory) {
  const web = path.resolve(webDirectory);
  const names = await auditOutput(web);
  const artifacts = {};
  const inlineScripts = {};
  const hashes = new Set();
  for (const name of names) {
    const bytes = await readBuilt(web, name);
    artifacts[name] = { sha256: digest(bytes), bytes: bytes.length };
    if (!name.endsWith('.html')) continue;
    const html = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)];
    if (
      scripts.length !== 1 ||
      [...html.matchAll(/<script\b/g)].length !== scripts.length
    )
      throw new Error('security_headers_unreviewed_script_shape');
    inlineScripts[name] = scripts.map(
      (match) =>
        "'sha256-" +
        createHash('sha256').update(match[1]).digest('base64') +
        "'"
    );
    for (const hash of inlineScripts[name]) hashes.add(hash);
  }
  const framework = await frameworkStyles(web, artifacts);
  const connectSources = relayConnectSources(deploymentRelayPolicy);
  const csp = [
    "default-src 'none'",
    "script-src 'self' " + [...hashes].sort().join(' '),
    "style-src 'self'",
    "style-src-attr 'unsafe-hashes' " + framework.styles[0].csp_hash,
    "img-src 'self'",
    "font-src 'self'",
    'connect-src ' +
      (connectSources.length ? connectSources.join(' ') : "'none'"),
    "object-src 'none'",
    "frame-src 'none'",
    "frame-ancestors 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "worker-src 'none'",
    "manifest-src 'none'"
  ].join('; ');
  // A second audit rejects a source/build transition during derivation.
  if (!isDeepStrictEqual(names, await auditOutput(web)))
    throw new Error('security_headers_stale_or_invalid');
  for (const name of names)
    if (digest(await readBuilt(web, name)) !== artifacts[name].sha256)
      throw new Error('security_headers_stale_or_invalid');
  if (
    digest(await readBoundFile(framework.manifestFile)) !==
    framework.manifest_sha256
  )
    throw new Error('security_headers_stale_or_invalid');
  return {
    schema: 'harvestcircle.static_security_headers.v1',
    readiness: 'SOURCE_PREPARATION_ONLY_HOST_QUALIFICATION_PENDING',
    relay_policy_sha256: digest(
      JSON.stringify(readRelayPolicy(deploymentRelayPolicy))
    ),
    build_info_sha256: artifacts['build-info.json'].sha256,
    compiler_manifest_sha256: framework.manifest_sha256,
    framework_style_attributes: framework.styles,
    connect_sources: connectSources,
    inline_scripts: inlineScripts,
    artifacts,
    headers: {
      'Content-Security-Policy': csp,
      'Referrer-Policy': 'no-referrer',
      'Permissions-Policy':
        'camera=(), microphone=(), geolocation=(), payment=(), usb=()',
      'X-Frame-Options': 'DENY',
      'X-Content-Type-Options': 'nosniff'
    }
  };
}

export async function verifySecurityHeaders(webDirectory, candidate) {
  if (!isDeepStrictEqual(candidate, await prepareSecurityHeaders(webDirectory)))
    throw new Error('security_headers_stale_or_invalid');
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const web = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
  const [operation, output, ...extra] = process.argv.slice(2);
  if (
    extra.length ||
    !['prepare', 'check'].includes(operation) ||
    !output ||
    !path.isAbsolute(output) ||
    (await realpath(process.cwd())) !== web
  )
    throw new Error(
      'Run from web: node tools/security-headers.mjs prepare|check /absolute/external/report.json'
    );
  const destination = path.resolve(output);
  const capsule = path.dirname(web);
  if (
    destination === capsule ||
    destination.startsWith(capsule + path.sep) ||
    (await realpath(path.dirname(destination))) !== path.dirname(destination)
  )
    throw new Error('Security header evidence must be outside the capsule');
  if (operation === 'prepare') {
    const prepared = await prepareSecurityHeaders(web);
    await writeFile(destination, JSON.stringify(prepared, null, 2) + '\n', {
      flag: 'wx',
      mode: 0o600
    });
    console.log(
      'Static security headers prepared; actual host qualification remains pending'
    );
  } else {
    await verifySecurityHeaders(
      web,
      JSON.parse(await readFile(destination, 'utf8'))
    );
    console.log('Static security headers match the current audited build');
  }
}
