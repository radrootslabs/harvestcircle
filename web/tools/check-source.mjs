import { fileURLToPath } from 'node:url';
import { auditSource } from './source-boundaries.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
try {
  const findings = await auditSource(root);
  if (findings.length) throw new Error(findings.join('\n'));
  console.log('Source style/import boundaries: checked');
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
