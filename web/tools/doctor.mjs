import { execFileSync } from 'node:child_process';
import { constants, openSync, fstatSync, readSync, closeSync } from 'node:fs';

/** @param {URL} file */
function readToolInput(file) {
  const fd = openSync(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = fstatSync(fd);
    if (!stat.isFile() || stat.size > 64 * 1024)
      throw new Error('Unsafe web toolchain input');
    const buffer = Buffer.alloc(64 * 1024 + 1);
    let size = 0;
    while (size < buffer.length) {
      const count = readSync(fd, buffer, size, buffer.length - size, null);
      if (!count) break;
      size += count;
    }
    if (size > 64 * 1024) throw new Error('Oversized web toolchain input');
    return new TextDecoder('utf-8', { fatal: true }).decode(
      buffer.subarray(0, size)
    );
  } finally {
    closeSync(fd);
  }
}

const inputs = JSON.parse(
  readToolInput(new URL('../package.json', import.meta.url))
);
const nodeVersion = inputs.engines?.node;
const pnpmVersion = inputs.engines?.pnpm;
if (
  typeof nodeVersion !== 'string' ||
  typeof pnpmVersion !== 'string' ||
  !/^\d+\.\d+\.\d+$/.test(nodeVersion) ||
  !/^\d+\.\d+\.\d+$/.test(pnpmVersion) ||
  inputs.packageManager !== `pnpm@${pnpmVersion}` ||
  readToolInput(new URL('../.node-version', import.meta.url)).trim() !==
    nodeVersion
)
  throw new Error('Web toolchain pins disagree or are not exact');

if (process.versions.node !== nodeVersion)
  throw new Error(
    `Expected Node ${nodeVersion}; selected ${process.versions.node}`
  );

// Diagnosis must not install a missing package manager or fetch its tooling.
const selectedPnpm = execFileSync('corepack', ['pnpm', '--version'], {
  cwd: process.cwd(),
  env: { ...process.env, COREPACK_ENABLE_NETWORK: '0' },
  encoding: 'utf8',
  timeout: 600000,
  maxBuffer: 64 * 1024,
  stdio: ['ignore', 'pipe', 'pipe']
}).trim();
if (selectedPnpm !== pnpmVersion)
  throw new Error(`Expected pnpm ${pnpmVersion}; selected ${selectedPnpm}`);
console.log(
  `HarvestCircle web tools: Node ${nodeVersion}, pnpm ${pnpmVersion}`
);
