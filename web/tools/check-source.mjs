import { readdir, open } from 'node:fs/promises';
import { constants } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import ts from 'typescript';
import { parse } from 'svelte/compiler';

const root = fileURLToPath(new URL('../', import.meta.url));
const sourceRoot = path.join(root, 'src');
const allowedCss = new Set(['src/theme.css', 'src/app.css']);
const findings = [];

// Same source-audit limit as the standalone native inventory reader.
const sourceByteLimit = 8 * 1024 * 1024;
async function readSource(file) {
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size > sourceByteLimit)
      throw new Error(`Unsafe source file: ${file}`);
    const chunks = [];
    let size = 0;
    for (;;) {
      const buffer = Buffer.alloc(64 * 1024);
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > sourceByteLimit)
        throw new Error(`Source file exceeds audit limit: ${file}`);
      chunks.push(buffer.subarray(0, bytesRead));
    }
    return new TextDecoder('utf-8', { fatal: true }).decode(
      Buffer.concat(chunks)
    );
  } finally {
    await handle.close();
  }
}
async function scan(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    const relative = path.relative(root, file).split(path.sep).join('/');
    if (entry.isSymbolicLink()) {
      findings.push(`${relative}: source symlinks are forbidden`);
      continue;
    }
    if (entry.isDirectory()) {
      await scan(file);
      continue;
    }
    if (!entry.isFile()) {
      findings.push(`${relative}: nonregular source entries are forbidden`);
      continue;
    }
    if (/\.(css|scss|sass|less)$/.test(relative) && !allowedCss.has(relative))
      findings.push(
        `${relative}: handwritten CSS belongs in theme.css or app.css`
      );
    if (!/\.(svelte|[cm]?[jt]s)$/.test(relative)) continue;
    const content = await readSource(file);
    const scripts = [];
    if (relative.endsWith('.svelte')) {
      const ast = parse(content, { modern: true });
      if (ast.css) findings.push(`${relative}: component CSS is forbidden`);
      const visit = (node) => {
        if (!node || typeof node !== 'object') return;
        if (
          node.type === 'StyleDirective' ||
          (node.type === 'Attribute' && node.name === 'style')
        )
          findings.push(`${relative}: inline styles are forbidden`);
        for (const value of Object.values(node)) {
          if (Array.isArray(value)) value.forEach(visit);
          else if (value && typeof value === 'object') visit(value);
        }
      };
      visit(ast.fragment);
      for (const script of [ast.instance, ast.module])
        if (script)
          scripts.push(content.slice(script.content.start, script.content.end));
    } else scripts.push(content);
    const inspectImport = (specifier) => {
      const resolved = specifier.startsWith('.')
        ? path.resolve(path.dirname(file), specifier)
        : null;
      if (
        (resolved && !resolved.startsWith(sourceRoot + path.sep)) ||
        /(^|\/)(tests?|core|app|foundation|enterprise|ops|generated)(\/|$)/.test(
          specifier
        ) ||
        /^(node:|nostr-tools(?:\/|$)|@nostr-dev-kit\/|@radroots\/)/.test(
          specifier
        )
      )
        findings.push(`${relative}: forbidden production import ${specifier}`);
      if (
        /\.(css|scss|sass|less)(\?|$)/.test(specifier) &&
        !(
          relative === 'src/routes/+layout.svelte' &&
          resolved &&
          allowedCss.has(
            path.relative(root, resolved).split(path.sep).join('/')
          )
        )
      )
        findings.push(`${relative}: CSS must be imported by the root layout`);
      if (
        /^applesauce-/.test(specifier) &&
        !relative.startsWith('src/lib/nostr/')
      )
        findings.push(`${relative}: Applesauce imports belong in lib/nostr`);
    };
    for (const script of scripts) {
      const parsed = ts.createSourceFile(
        file + '.ts',
        script,
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.TS
      );
      const visit = (node) => {
        if (
          (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
          node.moduleSpecifier &&
          ts.isStringLiteral(node.moduleSpecifier)
        )
          inspectImport(node.moduleSpecifier.text);
        if (
          ts.isCallExpression(node) &&
          node.expression.kind === ts.SyntaxKind.ImportKeyword &&
          node.arguments[0] &&
          ts.isStringLiteral(node.arguments[0])
        )
          inspectImport(node.arguments[0].text);
        ts.forEachChild(node, visit);
      };
      visit(parsed);
    }
  }
}
await scan(sourceRoot);
if (findings.length) {
  console.error(findings.join('\n'));
  process.exitCode = 1;
} else console.log('Source style/import boundaries: checked');
