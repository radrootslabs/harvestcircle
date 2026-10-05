import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, mkdir, open, realpath, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const generatedPath = 'web/static/build-info.json';
const sourceLockPath = 'radroots.lib.source-lock.v1.toml';
const oracleLockPath = 'core/Cargo.lock';
/** @param {Uint8Array} bytes */
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** @param {string} file */
async function readInput(file) {
  if ((await realpath(file)) !== file)
    throw new Error('Symlinked build provenance input');
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size > 8 * 1024 * 1024)
      throw new Error('Unsafe build provenance input');
    const chunks = [];
    let size = 0;
    for (;;) {
      const buffer = Buffer.alloc(64 * 1024);
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > 8 * 1024 * 1024)
        throw new Error('Oversized build provenance input');
      chunks.push(buffer.subarray(0, bytesRead));
    }
    const bytes = Buffer.concat(chunks);
    return { bytes, executable: Boolean(stat.mode & 0o111) };
  } finally {
    await handle.close();
  }
}

// No ambient Git redirection/config injection may select a sibling or parent.
/** @param {string} root @param {string[]} args */
function git(root, args) {
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
  );
  return execFileSync('git', ['-C', root, ...args], {
    env,
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe']
  });
}

/** @param {Uint8Array} bytes */
function selectedOracle(bytes) {
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  const fields = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    const entry = /^([a-z_][a-z0-9_]*) = "([^"\\]*)"$/.exec(line);
    if (!entry || fields.has(entry[1]))
      throw new Error('Malformed public Radroots source lock');
    fields.set(entry[1], entry[2]);
  }
  if (
    fields.get('schema') !== 'radroots.lib.source-lock.v1' ||
    fields.get('repository') !== 'https://github.com/radrootslabs/lib' ||
    !/^[a-f0-9]{40}$/.test(fields.get('revision') ?? '') ||
    fields.get('lockfile') !== oracleLockPath ||
    !/^[a-f0-9]{64}$/.test(fields.get('lockfile_sha256') ?? '')
  )
    throw new Error('Unavailable or invalid public Radroots oracle selection');
  return fields;
}

/** @param {string} webDirectory */
export async function deriveBuildInfo(webDirectory) {
  const web = await realpath(webDirectory);
  if (path.basename(web) !== 'web')
    throw new Error('Build provenance requires the owned web directory');
  const root = await realpath(path.dirname(web));
  if (
    (await realpath(git(root, ['rev-parse', '--show-toplevel']).trim())) !==
    root
  )
    throw new Error(
      'Build provenance requires HarvestCircle-owned Git metadata'
    );
  const revision = git(root, ['rev-parse', '--verify', 'HEAD']).trim();
  if (!/^[a-f0-9]{40}$/.test(revision))
    throw new Error('Unavailable source revision');
  if (git(root, ['ls-files', '-z', '--', generatedPath]))
    throw new Error('Generated build metadata must not be tracked');

  const sourceLock = await readInput(path.join(root, sourceLockPath));
  const oracle = selectedOracle(sourceLock.bytes);
  const oracleLock = await readInput(path.join(root, oracleLockPath));
  const oracleLockDigest = digest(oracleLock.bytes);
  if (oracleLockDigest !== oracle.get('lockfile_sha256'))
    throw new Error('Selected public oracle dependency lock digest mismatch');
  const webLock = await readInput(path.join(web, 'pnpm-lock.yaml'));

  // Git admits tracked and ordinary untracked inputs, excluding ignored output.
  // Only the producer's exact generated file is removed from untracked inputs.
  const scope = ['web', sourceLockPath, oracleLockPath];
  const inputs = [
    ...new Set([
      ...git(root, [
        'ls-files',
        '-z',
        '--cached',
        '--others',
        '--exclude-standard',
        '--',
        ...scope
      ])
        .split('\0')
        .filter((name) => name && name !== generatedPath),
      sourceLockPath,
      oracleLockPath,
      'web/pnpm-lock.yaml'
    ])
  ].sort();
  const fingerprint = createHash('sha256');
  for (const name of inputs) {
    fingerprint.update(JSON.stringify(name) + '\0');
    try {
      const input = await readInput(path.join(root, name));
      fingerprint.update(input.executable ? 'executable\0' : 'regular\0');
      fingerprint.update(digest(input.bytes) + '\0');
    } catch (error) {
      if (!(
        error instanceof Error &&
        'code' in error &&
        error.code === 'ENOENT'
      ))
        throw error;
      fingerprint.update('deleted\0');
    }
  }
  const status = git(root, [
    'status',
    '--porcelain=v1',
    '-z',
    '--untracked-files=all',
    '--',
    ...scope
  ]);
  const entries = status.split('\0');
  let dirty = false;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (!entry) continue;
    if (entry.slice(3) !== generatedPath) dirty = true;
    if (/[RC]/.test(entry.slice(0, 2))) i++;
  }

  // Explicit public allowlist; no environment, host paths, time or qualification.
  const metadata = {
    web_source: { revision, dirty, input_sha256: fingerprint.digest('hex') },
    radroots_oracle: {
      repository: oracle.get('repository'),
      revision: oracle.get('revision'),
      source_lock_sha256: digest(sourceLock.bytes)
    },
    dependency_locks: {
      web_pnpm_sha256: digest(webLock.bytes),
      oracle_cargo_sha256: oracleLockDigest
    }
  };
  return metadata;
}

/** @param {string} webDirectory */
export async function generateBuildInfo(webDirectory) {
  const metadata = await deriveBuildInfo(webDirectory);
  const web = await realpath(webDirectory);
  const root = await realpath(path.dirname(web));
  const directory = path.join(web, 'static');
  await mkdir(directory, { recursive: true });
  if (!(await lstat(directory)).isDirectory())
    throw new Error('Unsafe generated metadata directory');
  const output = path.join(root, generatedPath);
  try {
    const previous = await lstat(output);
    if (!previous.isFile() || previous.nlink !== 1)
      throw new Error('Unsafe generated metadata target');
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT'))
      throw error;
  }
  const temporary = path.join(directory, `.build-info-${randomUUID()}.tmp`);
  const handle = await open(
    temporary,
    constants.O_WRONLY |
      constants.O_CREAT |
      constants.O_EXCL |
      constants.O_NOFOLLOW,
    0o644
  );
  try {
    await handle.writeFile(JSON.stringify(metadata, null, 2) + '\n');
    await handle.close();
    await rename(temporary, output);
  } finally {
    await handle.close();
    await rm(temporary, { force: true });
  }
  return metadata;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const web = fileURLToPath(new URL('../', import.meta.url));
  if ((await realpath(process.cwd())) !== (await realpath(web)))
    throw new Error('Run the build provenance producer from web/');
  await generateBuildInfo(web);
}
