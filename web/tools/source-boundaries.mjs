import { readdir, open, lstat, realpath } from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import ts from 'typescript';
import { parse } from 'svelte/compiler';

const byteLimit = 8 * 1024 * 1024;
const codeExtension = /\.(svelte|[cm]?[jt]sx?)$/;
const forbiddenPath =
  /(^|\/)(tests?|__tests__|fixtures?|harness|providers?|credentials?|core|app|foundation|enterprise|ops|generated|node_modules|build|\.svelte-kit)(\/|$)|\.(test|spec)\./i;
const applesauce = new Set([
  'applesauce-core',
  'applesauce-relay',
  'applesauce-signers',
  'applesauce-common',
  'applesauce-loaders'
]);
const runtimePackages = new Set([
  'svelte',
  '@sveltejs/kit',
  'rxjs',
  ...applesauce
]);
const frameworkModules = new Set([
  '$app/env',
  '$app/env/public',
  '$app/forms',
  '$app/navigation',
  '$app/paths',
  '$app/state'
]);
const declarativeMutationTags = new Set([
  'set',
  'animate',
  'animatemotion',
  'animatetransform',
  'discard'
]);
const inside = (directory, file) => file.startsWith(directory + path.sep);
const relative = (root, file) =>
  path.relative(root, file).split(path.sep).join('/');

// Admit bytes before any third-party parser. Retain the opened identity and
// reject replacement, hardlinks, mutation, symlinks and nonregular entries.
async function readSource(file) {
  const before = await lstat(file);
  if (!before.isFile() || before.nlink !== 1 || (await realpath(file)) !== file)
    throw new Error(`Unsafe source file: ${file}`);
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (
      !stat.isFile() ||
      stat.nlink !== 1 ||
      stat.size > byteLimit ||
      stat.dev !== before.dev ||
      stat.ino !== before.ino
    )
      throw new Error(`Unsafe source file: ${file}`);
    const chunks = [];
    let size = 0;
    for (;;) {
      const buffer = Buffer.alloc(64 * 1024);
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > byteLimit)
        throw new Error(`Source file exceeds audit limit: ${file}`);
      chunks.push(buffer.subarray(0, bytesRead));
    }
    const after = await handle.stat();
    const named = await lstat(file);
    if (
      size !== stat.size ||
      after.size !== stat.size ||
      after.mtimeMs !== stat.mtimeMs ||
      after.ctimeMs !== stat.ctimeMs ||
      named.dev !== stat.dev ||
      named.ino !== stat.ino ||
      !named.isFile() ||
      named.nlink !== 1 ||
      (await realpath(file)) !== file
    )
      throw new Error(`Source file changed during audit: ${file}`);
    return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(
      Buffer.concat(chunks)
    );
  } finally {
    await handle.close();
  }
}

function sourceAst(file, text) {
  const ast = ts.createSourceFile(
    file,
    text,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('x') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  if (ast.parseDiagnostics.length)
    throw new Error(`${file}: invalid source syntax`);
  return ast;
}
// A source guard is not a JavaScript sandbox. Admit a deliberately bounded DOM
// grammar: direct focus/blur/scroll and scalar control reads; no escaped nodes,
// actions, bindings, raw render factories or opaque object mutation. Svelte owns
// DOM updates. Mutable application data must be locally owned plain data, with
// pure values and lexical aliases. Future imperative UI work needs explicit
// admission and negative tests rather than a type cast or an ignored diagnostic.
function mutationAdmission(ast, report, identity = () => null) {
  // Bind this admitted AST only, without libs, module resolution, file reads,
  // config loading or evaluation. Symbols distinguish aliases and shadowing.
  const options = {
    noLib: true,
    noResolve: true,
    types: [],
    allowNonTsExtensions: true
  };
  const host = {
    ...ts.createCompilerHost(options),
    getSourceFile: (name) => (name === ast.fileName ? ast : undefined),
    fileExists: (name) => name === ast.fileName,
    directoryExists: () => false,
    getDirectories: () => [],
    readFile: () => undefined
  };
  const checker = ts
    .createProgram([ast.fileName], options, host)
    .getTypeChecker();
  const unwrap = (node) => {
    while (
      node &&
      (ts.isParenthesizedExpression(node) ||
        ts.isAsExpression(node) ||
        ts.isTypeAssertionExpression(node) ||
        ts.isNonNullExpression(node))
    )
      node = node.expression;
    return node;
  };
  const member = (node) =>
    ts.isPropertyAccessExpression(node)
      ? node.name.text
      : ts.isElementAccessExpression(node) &&
          ts.isStringLiteralLike(node.argumentExpression)
        ? node.argumentExpression.text
        : null;
  function uiLeaf(node) {
    let top = node;
    while (
      top.parent &&
      (ts.isPropertyAccessExpression(top.parent) ||
        ts.isElementAccessExpression(top.parent) ||
        ts.isCallExpression(top.parent) ||
        ts.isNonNullExpression(top.parent) ||
        ts.isAsExpression(top.parent) ||
        ts.isParenthesizedExpression(top.parent)) &&
      top.parent.expression === top
    )
      top = top.parent;
    const action =
      ts.isCallExpression(top) &&
      ['focus', 'blur', 'scrollIntoView'].includes(member(top.expression)) &&
      top.arguments.length === 0;
    const read =
      !action &&
      ['value', 'checked', 'selectedIndex', 'textContent'].includes(
        member(top)
      );
    if (!action && !read) return false;
    const receiver = unwrap(
      action ? top.expression.expression : top.expression
    );
    if (
      ts.isPropertyAccessExpression(receiver) &&
      ['currentTarget', 'target'].includes(receiver.name.text)
    )
      return true;
    return (
      ts.isCallExpression(receiver) &&
      ts.isPropertyAccessExpression(receiver.expression) &&
      ts.isIdentifier(receiver.expression.expression) &&
      receiver.expression.expression.text === 'document' &&
      !checker.getSymbolAtLocation(receiver.expression.expression) &&
      ['getElementById', 'querySelector'].includes(
        receiver.expression.name.text
      ) &&
      receiver.arguments.length === 1 &&
      ts.isStringLiteralLike(receiver.arguments[0])
    );
  }
  function pure(node, seen = new Set()) {
    node = unwrap(node);
    if (!node) return false;
    if (
      ts.isStringLiteralLike(node) ||
      ts.isNumericLiteral(node) ||
      ts.isBigIntLiteral(node) ||
      [
        ts.SyntaxKind.TrueKeyword,
        ts.SyntaxKind.FalseKeyword,
        ts.SyntaxKind.NullKeyword
      ].includes(node.kind)
    )
      return true;
    if (ts.isObjectLiteralExpression(node))
      return node.properties.every(
        (property) =>
          ts.isPropertyAssignment(property) &&
          !ts.isComputedPropertyName(property.name) &&
          !['__proto__', 'constructor', 'prototype'].includes(
            property.name.text
          ) &&
          pure(property.initializer, seen)
      );
    if (ts.isArrayLiteralExpression(node))
      return node.elements.every((element) => pure(element, seen));
    if (ts.isIdentifier(node)) {
      const symbol = checker.getSymbolAtLocation(node);
      if (!symbol) return node.text === 'undefined';
      if (seen.has(symbol)) return false;
      seen = new Set([...seen, symbol]);
      const declarations = symbol.declarations ?? [];
      return (
        declarations.length === 1 &&
        (ts.isVariableDeclaration(declarations[0])
          ? pure(declarations[0].initializer, seen)
          : ts.isParameter(declarations[0]) &&
            [
              ts.SyntaxKind.StringKeyword,
              ts.SyntaxKind.NumberKeyword,
              ts.SyntaxKind.BooleanKeyword,
              ts.SyntaxKind.BigIntKeyword
            ].includes(declarations[0].type?.kind))
      );
    }
    if (
      (ts.isPropertyAccessExpression(node) ||
        ts.isElementAccessExpression(node)) &&
      owned(node.expression, seen)
    )
      return (
        member(node) !== null &&
        !['constructor', 'prototype', '__proto__'].includes(member(node))
      );
    if (ts.isPrefixUnaryExpression(node)) return pure(node.operand, seen);
    if (
      ts.isBinaryExpression(node) &&
      !ts.isAssignmentOperator(node.operatorToken.kind)
    )
      return pure(node.left, seen) && pure(node.right, seen);
    if (ts.isConditionalExpression(node))
      return (
        pure(node.condition, seen) &&
        pure(node.whenTrue, seen) &&
        pure(node.whenFalse, seen)
      );
    return uiLeaf(node) && !ts.isCallExpression(node);
  }
  function owned(node, seen = new Set()) {
    node = unwrap(node);
    if (!node) return false;
    if (ts.isObjectLiteralExpression(node) || ts.isArrayLiteralExpression(node))
      return pure(node, seen);
    if (ts.isIdentifier(node)) {
      const symbol = checker.getSymbolAtLocation(node);
      if (!symbol || seen.has(symbol)) return false;
      const declaration =
        symbol.declarations?.length === 1 ? symbol.declarations[0] : null;
      return (
        declaration &&
        ts.isVariableDeclaration(declaration) &&
        owned(declaration.initializer, new Set([...seen, symbol]))
      );
    }
    if (
      ts.isPropertyAccessExpression(node) ||
      ts.isElementAccessExpression(node)
    )
      return (
        owned(node.expression, seen) &&
        member(node) !== null &&
        !['constructor', 'prototype', '__proto__'].includes(member(node))
      );
    return false;
  }
  const writes = new Set();
  const globalWrites = new Set();
  const collectWrites = (node) => {
    if (
      (ts.isBinaryExpression(node) &&
        ts.isAssignmentOperator(node.operatorToken.kind)) ||
      ((ts.isPrefixUnaryExpression(node) ||
        ts.isPostfixUnaryExpression(node)) &&
        [ts.SyntaxKind.PlusPlusToken, ts.SyntaxKind.MinusMinusToken].includes(
          node.operator
        ))
    ) {
      const target = unwrap(
        ts.isBinaryExpression(node) ? node.left : node.operand
      );
      if (ts.isIdentifier(target)) {
        const symbol = checker.getSymbolAtLocation(target);
        if (symbol) writes.add(symbol);
        else globalWrites.add(target.text);
      }
    }
    ts.forEachChild(node, collectWrites);
  };
  collectWrites(ast);
  function bindingSymbol(node) {
    const parent = node.parent;
    if (
      ts.isExportSpecifier(parent) &&
      !parent.parent.parent.moduleSpecifier &&
      node === (parent.propertyName ?? parent.name)
    )
      return checker.getExportSpecifierLocalTargetSymbol(parent);
    return checker.getSymbolAtLocation(node);
  }
  function declaration(node) {
    const symbol = bindingSymbol(node);
    return symbol && !writes.has(symbol) && symbol.declarations?.length === 1
      ? symbol.declarations[0]
      : null;
  }
  function immutable(node, seen = new Set()) {
    if (!ts.isIdentifier(node)) return null;
    const decl = declaration(node);
    if (
      !decl ||
      !ts.isVariableDeclaration(decl) ||
      !decl.initializer ||
      !(decl.parent.flags & ts.NodeFlags.Const) ||
      seen.has(decl)
    )
      return null;
    return { node: decl.initializer, seen: new Set([...seen, decl]) };
  }
  // Reject writes and namespace escapes independently from successful sink
  // resolution, so losing a binding's trust can never hide its capability.
  function capabilityBinding(node) {
    const symbol = bindingSymbol(node);
    const decl =
      symbol?.declarations?.length === 1 ? symbol.declarations[0] : null;
    if (decl && ts.isImportSpecifier(decl))
      return identity(
        decl.parent.parent.parent.moduleSpecifier.text,
        (decl.propertyName ?? decl.name).text
      );
    if (decl && ts.isNamespaceImport(decl))
      return identity(decl.parent.parent.moduleSpecifier.text, null) === 'goto'
        ? 'navigationNamespace'
        : null;
    return null;
  }
  function imported(node) {
    node = unwrap(node);
    if (ts.isIdentifier(node)) {
      const decl = declaration(node);
      if (
        decl &&
        ts.isImportSpecifier(decl) &&
        !decl.isTypeOnly &&
        !decl.parent.parent.isTypeOnly
      )
        return identity(
          decl.parent.parent.parent.moduleSpecifier.text,
          (decl.propertyName ?? decl.name).text
        );
    }
    if (
      ts.isPropertyAccessExpression(node) &&
      ts.isIdentifier(node.expression)
    ) {
      const decl = declaration(node.expression);
      if (decl && ts.isNamespaceImport(decl) && !decl.parent.isTypeOnly)
        return identity(
          decl.parent.parent.moduleSpecifier.text,
          node.name.text
        );
    }
    return null;
  }
  function strings(node, seen = new Set()) {
    node = unwrap(node);
    if (!node) return null;
    if (ts.isStringLiteralLike(node)) return [node.text];
    const alias = immutable(node, seen);
    if (alias) return strings(alias.node, alias.seen);
    if (ts.isConditionalExpression(node)) {
      const a = strings(node.whenTrue, seen),
        b = strings(node.whenFalse, seen);
      return a && b && a.length + b.length <= 64 ? [...a, ...b] : null;
    }
    if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.PlusToken
    ) {
      const a = strings(node.left, seen),
        b = strings(node.right, seen);
      return a && b && a.length * b.length <= 64
        ? a.flatMap((x) => b.map((y) => x + y))
        : null;
    }
    if (ts.isTemplateExpression(node)) {
      let result = [node.head.text];
      for (const span of node.templateSpans) {
        const part = strings(span.expression, seen);
        if (!part || result.length * part.length > 64) return null;
        result = result.flatMap((x) =>
          part.map((y) => x + y + span.literal.text)
        );
      }
      return result;
    }
    return null;
  }
  function forbiddenCharacters(value) {
    return [...value].some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || (code >= 127 && code <= 159) || character === '\\';
    });
  }
  function safeText(value, internal) {
    if (
      !value ||
      value.length > 8192 ||
      !value.isWellFormed() ||
      Buffer.byteLength(value) > 8192 ||
      /\s/u.test(value) ||
      forbiddenCharacters(value)
    )
      return false;
    try {
      if (forbiddenCharacters(decodeURIComponent(value))) return false;
      const local =
        (value[0] === '/' && value[1] !== '/') || ['?', '#'].includes(value[0]);
      if (!local && (internal || !/^(https:\/\/|mailto:|tel:)/i.test(value)))
        return false;
      if (/^https:/i.test(value) && !/^https:\/\/[^/?#]/i.test(value))
        return false;
      const url = new URL(value, 'https://navigation.invalid');
      return (
        !url.username &&
        !url.password &&
        !/^https:\/\/[^/?#]*@/i.test(value) &&
        (local || url.protocol === 'https:' || !url.host) &&
        (local ? url.origin === 'https://navigation.invalid' : !!url.pathname)
      );
    } catch {
      return false;
    }
  }
  function urlValue(node, internal, defined = false) {
    // Casts and non-null assertions are never evidence of runtime admission.
    while (ts.isParenthesizedExpression(node)) node = node.expression;
    const values = strings(node);
    if (values) return values.every((value) => safeText(value, internal));
    if (
      ts.isCallExpression(node) &&
      node.arguments.length === 1 &&
      ['internalHref', ...(internal ? [] : ['navigationHref'])].includes(
        imported(node.expression)
      )
    )
      return !defined;
    const alias = immutable(node);
    if (
      defined &&
      alias &&
      ts.isCallExpression(alias.node) &&
      urlValue(alias.node, internal)
    ) {
      let child = node;
      for (
        let parent = node.parent;
        parent;
        child = parent, parent = parent.parent
      ) {
        if (ts.isIfStatement(parent) && parent.thenStatement === child) {
          const condition = parent.expression;
          if (
            ts.isIdentifier(condition) &&
            checker.getSymbolAtLocation(condition) ===
              checker.getSymbolAtLocation(node)
          )
            return true;
          if (
            ts.isBinaryExpression(condition) &&
            condition.operatorToken.kind ===
              ts.SyntaxKind.ExclamationEqualsEqualsToken &&
            ts.isIdentifier(condition.left) &&
            checker.getSymbolAtLocation(condition.left) ===
              checker.getSymbolAtLocation(node) &&
            ts.isIdentifier(condition.right) &&
            condition.right.text === 'undefined' &&
            !checker.getSymbolAtLocation(condition.right)?.declarations?.length
          )
            return true;
        }
      }
    }
    return false;
  }
  function global(node, names) {
    return (
      ts.isIdentifier(node) &&
      names.includes(node.text) &&
      !checker.getSymbolAtLocation(node)
    );
  }
  function location(node) {
    return (
      global(node, ['location']) ||
      (ts.isPropertyAccessExpression(node) &&
        node.name.text === 'location' &&
        global(node.expression, ['window', 'globalThis', 'self']))
    );
  }
  function locationSink(node) {
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      ['assign', 'replace'].includes(node.expression.name.text) &&
      location(node.expression.expression)
    )
      return (
        node.arguments.length === 1 && urlValue(node.arguments[0], true, true)
      );
    if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
      (location(node.left) ||
        (ts.isPropertyAccessExpression(node.left) &&
          node.left.name.text === 'href' &&
          location(node.left.expression)))
    )
      return urlValue(node.right, true, true);
    return false;
  }
  function locationUse(node) {
    let top = node;
    while (
      top.parent &&
      ((ts.isPropertyAccessExpression(top.parent) &&
        top.parent.expression === top) ||
        (ts.isCallExpression(top.parent) && top.parent.expression === top))
    )
      top = top.parent;
    if (
      top.parent &&
      ts.isBinaryExpression(top.parent) &&
      top.parent.left === top
    )
      top = top.parent;
    return locationSink(top);
  }
  function callback(node) {
    if (ts.isArrowFunction(node) || ts.isFunctionExpression(node)) return true;
    const decl = ts.isIdentifier(node) && declaration(node);
    if (decl && ts.isFunctionDeclaration(decl)) return true;
    const alias = immutable(node);
    return (
      !!alias &&
      (ts.isArrowFunction(alias.node) || ts.isFunctionExpression(alias.node))
    );
  }
  function timerUse(node) {
    const parent = node.parent;
    return (
      ts.isCallExpression(parent) &&
      parent.expression === node &&
      parent.arguments.length >= 1 &&
      callback(parent.arguments[0])
    );
  }
  function collection(node) {
    const alias = immutable(node);
    return (
      !!alias &&
      ts.isNewExpression(alias.node) &&
      global(alias.node.expression, ['Set']) &&
      !globalWrites.has('Set') &&
      (!alias.node.arguments?.length ||
        (alias.node.arguments.length === 1 &&
          ts.isArrayLiteralExpression(alias.node.arguments[0]) &&
          pure(alias.node.arguments[0])))
    );
  }
  function stringReceiver(node, seen = new Set()) {
    if (strings(node)) return true;
    const alias = immutable(unwrap(node), seen);
    if (alias) return stringReceiver(alias.node, alias.seen);
    node = unwrap(node);
    return (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      node.expression.name.text === 'replace' &&
      node.arguments.length === 2 &&
      stringReceiver(node.expression.expression, seen) &&
      (ts.isRegularExpressionLiteral(node.arguments[0]) ||
        !!strings(node.arguments[0])) &&
      !!strings(node.arguments[1])
    );
  }
  function primitiveMethod(node) {
    const call = node.parent;
    if (
      !ts.isPropertyAccessExpression(node) ||
      !ts.isCallExpression(call) ||
      call.expression !== node
    )
      return false;
    if (node.name.text === 'add')
      return (
        collection(node.expression) &&
        call.arguments.length === 1 &&
        pure(call.arguments[0])
      );
    if (node.name.text === 'replace')
      return (
        stringReceiver(node.expression) &&
        call.arguments.length === 2 &&
        (ts.isRegularExpressionLiteral(call.arguments[0]) ||
          !!strings(call.arguments[0])) &&
        !!strings(call.arguments[1])
      );
    return false;
  }
  const animationMethods = new Set(['animate', 'setKeyframes']);
  const methods = new Set([
    ...animationMethods,
    'setAttribute',
    'setAttributeNS',
    'toggleAttribute',
    'removeAttribute',
    'removeAttributeNS',
    'setAttributeNode',
    'setAttributeNodeNS',
    'removeAttributeNode',
    'setNamedItem',
    'setNamedItemNS',
    'removeNamedItem',
    'removeNamedItemNS',
    'add',
    'remove',
    'replace',
    'toggle',
    'setProperty',
    'removeProperty',
    'appendRule',
    'deleteRule',
    'insertRule',
    'replaceSync',
    'append',
    'prepend',
    'appendChild',
    'insertBefore',
    'insertAdjacentElement',
    'insertAdjacentHTML',
    'replaceChild',
    'replaceChildren',
    'removeChild',
    'before',
    'after',
    'replaceWith',
    'write',
    'writeln',
    'insertNode',
    'surroundContents',
    'createContextualFragment',
    'execCommand',
    'load',
    'setHTML',
    'setHTMLUnsafe',
    '__defineGetter__',
    '__defineSetter__',
    'push',
    'unshift',
    'splice'
  ]);
  const navigatorCapabilities = new Set([
    'clipboard',
    'locks',
    'storage',
    'onLine',
    'language',
    'languages',
    'userAgent',
    'platform',
    'hardwareConcurrency',
    'cookieEnabled'
  ]);
  const navigatorLeaf = (node) =>
    ts.isPropertyAccessExpression(node.parent) &&
    node.parent.expression === node &&
    navigatorCapabilities.has(node.parent.name.text);
  function inspect(node) {
    const admittedLocation = locationSink(node);
    if (
      ts.isCallExpression(node) &&
      imported(node.expression) === 'goto' &&
      (node.arguments.length < 1 || !urlValue(node.arguments[0], true, true))
    )
      report('navigation goto requires a defined validated internal URL');
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      /^__hcr(?:Navigation|Resource)Url$/.test(node.expression.text)
    ) {
      const internal = node.expression.text === '__hcrResourceUrl';
      if (
        node.arguments.length !== 1 ||
        !urlValue(node.arguments[0], internal) ||
        (internal && !strings(node.arguments[0]))
      )
        report('unresolved or unsafe markup URL');
    }
    if (
      ts.isBinaryExpression(node) &&
      ts.isAssignmentOperator(node.operatorToken.kind) &&
      !admittedLocation
    ) {
      const left = unwrap(node.left);
      if (
        (ts.isPropertyAccessExpression(left) ||
          ts.isElementAccessExpression(left)) &&
        (!owned(left.expression) || !pure(node.right))
      )
        report(
          'opaque DOM/object mutations are forbidden; use owned plain model data'
        );
      if (ts.isIdentifier(left) && owned(left) && !pure(node.right))
        report('owned model reassignment cannot introduce opaque capabilities');
      if (
        ts.isArrayLiteralExpression(left) ||
        ts.isObjectLiteralExpression(left)
      )
        report(
          'bulk mutation targets must use the admitted plain-model grammar'
        );
    }
    if (
      ((ts.isPrefixUnaryExpression(node) ||
        ts.isPostfixUnaryExpression(node)) &&
        [ts.SyntaxKind.PlusPlusToken, ts.SyntaxKind.MinusMinusToken].includes(
          node.operator
        )) ||
      ts.isDeleteExpression(node)
    ) {
      const operand = unwrap(
        ts.isDeleteExpression(node) ? node.expression : node.operand
      );
      if (
        (ts.isPropertyAccessExpression(operand) ||
          ts.isElementAccessExpression(operand)) &&
        !owned(operand.expression)
      )
        report('opaque DOM/object update/delete mutations are forbidden');
    }
    if (
      ts.isPropertyAccessExpression(node) ||
      ts.isElementAccessExpression(node)
    ) {
      const computedNames = ts.isElementAccessExpression(node)
        ? strings(node.argumentExpression)
        : null;
      const name =
        member(node) ?? (computedNames?.length === 1 ? computedNames[0] : null);
      if (
        computedNames?.some((name) => animationMethods.has(name)) &&
        !owned(node.expression)
      )
        report(
          'opaque or indirect animation mutation/setter APIs are forbidden'
        );
      if (
        imported(node) === 'goto' &&
        !(ts.isCallExpression(node.parent) && node.parent.expression === node)
      )
        report('escaped goto capability is forbidden');
      if (location(node) && !locationUse(node))
        report('opaque Location capability is forbidden');
      if (
        ['setTimeout', 'setInterval'].includes(name) &&
        global(node.expression, ['window', 'globalThis', 'self'])
      )
        report('indirect timer capability is forbidden');
      if (
        name === 'navigator' &&
        ts.isIdentifier(node.expression) &&
        ['window', 'globalThis', 'self'].includes(node.expression.text) &&
        !checker.getSymbolAtLocation(node.expression) &&
        !navigatorLeaf(node)
      )
        report('opaque navigator capabilities cannot expose worker loaders');
      if (['constructor', 'prototype', '__proto__'].includes(name))
        report(
          'prototype capability access is outside the admitted DOM/model grammar'
        );
      if (
        [
          'currentTarget',
          'target',
          'ownerDocument',
          'contentDocument',
          'contentWindow',
          'defaultView',
          'composedPath',
          'getRootNode'
        ].includes(name) &&
        !owned(node.expression) &&
        !uiLeaf(node)
      )
        report(
          'opaque DOM capabilities cannot escape direct UI leaf operations'
        );
      const locationMethod =
        ts.isPropertyAccessExpression(node) &&
        ['assign', 'replace'].includes(name) &&
        location(node.expression) &&
        locationUse(node);
      if (
        methods.has(name) &&
        !primitiveMethod(node) &&
        !locationMethod &&
        !(
          animationMethods.has(name) &&
          owned(node.expression) &&
          !ts.isCallExpression(node.parent)
        )
      ) {
        if (
          !ts.isCallExpression(node.parent) ||
          node.parent.expression !== node ||
          !owned(node.expression) ||
          !node.parent.arguments.every((argument) => pure(argument))
        )
          report('opaque or indirect DOM mutation/setter APIs are forbidden');
      }
    }
    if (ts.isIdentifier(node)) {
      const parent = node.parent;
      if (
        collection(node) &&
        !(ts.isVariableDeclaration(parent) && parent.name === node) &&
        !(
          ts.isPropertyAccessExpression(parent) &&
          parent.expression === node &&
          ((['add', 'has'].includes(parent.name.text) &&
            ts.isCallExpression(parent.parent) &&
            parent.parent.expression === parent &&
            parent.parent.arguments.every((argument) => pure(argument))) ||
            parent.name.text === 'size')
        )
      )
        report(
          'owned collection capabilities cannot escape direct data operations'
        );
      const capability = capabilityBinding(node);
      if (capability && writes.has(checker.getSymbolAtLocation(node)))
        report('reassigned navigation capability is forbidden');
      if (
        capability === 'navigationNamespace' &&
        !ts.isNamespaceImport(parent) &&
        !(
          ts.isPropertyAccessExpression(parent) &&
          parent.expression === node &&
          parent.name.text === 'goto' &&
          ts.isCallExpression(parent.parent) &&
          parent.parent.expression === parent
        )
      )
        report('escaped navigation namespace is forbidden');
      const propertyName =
        (ts.isPropertyAccessExpression(parent) && parent.name === node) ||
        (ts.isPropertyAssignment(parent) &&
          parent.name === node &&
          !ts.isShorthandPropertyAssignment(parent));
      const typeName =
        ts.isTypeReferenceNode(parent) || ts.isTypeQueryNode(parent);
      const symbol = checker.getSymbolAtLocation(node);
      if (
        !propertyName &&
        !typeName &&
        !symbol &&
        ['KeyframeEffect', 'Animation'].includes(node.text)
      )
        report('imperative animation constructors are forbidden');
      if (
        !propertyName &&
        !typeName &&
        !symbol &&
        node.text === 'Set' &&
        globalWrites.has('Set')
      )
        report('global Set constructor rebinding is forbidden');
      if (
        !propertyName &&
        !typeName &&
        !symbol &&
        ['setTimeout', 'setInterval'].includes(node.text) &&
        !timerUse(node)
      )
        report('timer requires a direct function callback');
      if (
        !propertyName &&
        !typeName &&
        global(node, ['location']) &&
        !locationUse(node)
      )
        report('opaque Location capability is forbidden');
      if (
        imported(node) === 'goto' &&
        !(
          ts.isImportSpecifier(parent) ||
          (ts.isCallExpression(parent) && parent.expression === node)
        )
      )
        report('escaped goto capability is forbidden');
      if (!propertyName && !typeName && !symbol) {
        if (node.text === 'navigator' && !navigatorLeaf(node))
          report('opaque navigator capabilities cannot expose worker loaders');
        if (
          node.text === 'document' &&
          !uiLeaf(node) &&
          !ts.isTypeOfExpression(parent)
        )
          report('DOM handles cannot escape the bounded direct UI grammar');
        if (
          [
            'window',
            'globalThis',
            'self',
            'top',
            'parent',
            'frames',
            'opener'
          ].includes(node.text) &&
          !ts.isTypeOfExpression(parent)
        ) {
          if (
            !ts.isPropertyAccessExpression(parent) ||
            parent.expression !== node ||
            !['nostr', 'indexedDB', 'crypto', 'navigator', 'location'].includes(
              parent.name.text
            )
          )
            report('opaque browser/DOM global capability access is forbidden');
        }
        if (
          /^(?:Document|Node|Element|DOMParser|DOMImplementation|Range|XPathEvaluator|XSLTProcessor|HTML\w*Element|SVG\w*Element|CSS\w*|DOMTokenList|NamedNodeMap|Attr)$/.test(
            node.text
          )
        )
          report(
            'native DOM/resource constructors are outside the admitted UI grammar'
          );
        if (['Object', 'Reflect'].includes(node.text)) {
          if (
            !ts.isPropertyAccessExpression(parent) ||
            parent.expression !== node ||
            !ts.isCallExpression(parent.parent) ||
            parent.parent.expression !== parent ||
            !parent.parent.arguments.every((argument) => pure(argument))
          )
            report(
              'reflection/bulk operations require direct calls over owned plain data'
            );
        }
      }
    }
    for (const doc of node.jsDoc ?? []) inspect(doc);
    ts.forEachChild(node, inspect);
  }
  inspect(ast);
  return owned;
}

function literal(node) {
  if (ts.isStringLiteralLike(node)) return node.text;
  if (node.kind === ts.SyntaxKind.TrueKeyword) return true;
  if (node.kind === ts.SyntaxKind.FalseKeyword) return false;
  if (ts.isArrayLiteralExpression(node)) return node.elements.map(literal);
  if (ts.isObjectLiteralExpression(node)) {
    const value = Object.create(null);
    for (const property of node.properties) {
      if (
        !ts.isPropertyAssignment(property) ||
        !property.name ||
        (!ts.isIdentifier(property.name) && !ts.isStringLiteral(property.name))
      )
        throw new Error('Import configuration must use literal properties');
      const key = property.name.text;
      if (Object.hasOwn(value, key))
        throw new Error('Duplicate import configuration property');
      value[key] = literal(property.initializer);
    }
    return value;
  }
  throw new Error('Import configuration must be statically resolvable');
}

async function configuration(root) {
  for (const name of await readdir(root)) {
    if (/^(vite|svelte)\.config\.(js|mjs|cjs|mts|cts)$/.test(name))
      throw new Error(`Unsupported competing source configuration: ${name}`);
  }
  const pkg = JSON.parse(await readSource(path.join(root, 'package.json')));
  const configText = await readSource(path.join(root, 'tsconfig.json'));
  const parsed = ts.parseConfigFileTextToJson('tsconfig.json', configText);
  if (parsed.error) throw new Error('Invalid tsconfig.json');
  const config = parsed.config;
  if (config.extends !== '$app/tsconfig' || config.compilerOptions?.baseUrl)
    throw new Error('Unsupported TypeScript import configuration');
  const vite = sourceAst(
    'vite.config.ts',
    await readSource(path.join(root, 'vite.config.ts'))
  );
  const defaults = vite.statements.filter(ts.isExportAssignment);
  if (
    defaults.length !== 1 ||
    !ts.isCallExpression(defaults[0].expression) ||
    defaults[0].expression.expression.getText(vite) !== 'defineConfig'
  )
    throw new Error('Vite configuration must be statically resolvable');
  const object = defaults[0].expression.arguments[0];
  if (!object || !ts.isObjectLiteralExpression(object))
    throw new Error('Vite configuration must be a literal object');
  const aliases = [];
  const configKeys = new Set();
  for (const property of object.properties) {
    if (!ts.isPropertyAssignment(property) || !ts.isIdentifier(property.name))
      throw new Error('Unsupported Vite import configuration');
    const key = property.name.text;
    if (configKeys.has(key))
      throw new Error('Duplicate Vite configuration property');
    configKeys.add(key);
    if (key === 'plugins') {
      // Keep the existing Kit3/static adapter pipeline. Arbitrary plugins can
      // rewrite imports invisibly; they require explicit guard support.
      const normalized = property.initializer.getText(vite).replace(/\s/g, '');
      if (
        normalized !==
          "[sveltekit({adapter:adapter({strict:true,fallback:'200.html'})})]" &&
        normalized !==
          '[sveltekit({adapter:adapter({strict:true,fallback:"200.html"})})]'
      )
        throw new Error('Unsupported Vite source transformation');
    } else if (key === 'resolve') {
      const resolve = literal(property.initializer);
      if (Object.keys(resolve).some((name) => name !== 'alias'))
        throw new Error('Unsupported Vite resolution option');
      const values = Array.isArray(resolve.alias)
        ? resolve.alias
        : Object.entries(resolve.alias ?? {}).map(([find, replacement]) => ({
            find,
            replacement
          }));
      for (const value of values) {
        if (
          typeof value.find !== 'string' ||
          typeof value.replacement !== 'string'
        )
          throw new Error('Unsupported Vite alias');
        aliases.push(value);
      }
    } else throw new Error(`Unsupported Vite configuration property: ${key}`);
  }
  // Configuration must not execute hidden resolver mutation before defineConfig.
  for (const statement of vite.statements) {
    if (ts.isImportDeclaration(statement)) {
      const imported = statement.moduleSpecifier.text;
      const clause = statement.importClause;
      const expected = imported === 'vite' ? 'defineConfig' : 'sveltekit';
      if (imported === '@sveltejs/adapter-static') {
        if (!clause || clause.name?.text !== 'adapter' || clause.namedBindings)
          throw new Error('Unsupported Vite adapter binding');
      } else if (
        !clause ||
        clause.name ||
        !clause.namedBindings ||
        !ts.isNamedImports(clause.namedBindings) ||
        clause.namedBindings.elements.length !== 1 ||
        clause.namedBindings.elements[0].propertyName ||
        clause.namedBindings.elements[0].name.text !== expected
      )
        throw new Error('Unsupported Vite resolver binding');
      if (
        !['vite', '@sveltejs/kit/vite', '@sveltejs/adapter-static'].includes(
          imported
        )
      )
        throw new Error(`Unsupported Vite config import: ${imported}`);
    } else if (!ts.isExportAssignment(statement))
      throw new Error('Unsupported Vite configuration statement');
  }
  for (const [find, replacements] of Object.entries(
    config.compilerOptions?.paths ?? {}
  )) {
    if (
      !Array.isArray(replacements) ||
      replacements.length !== 1 ||
      typeof replacements[0] !== 'string'
    )
      throw new Error('Unsupported TypeScript path mapping');
    // Audit TS mappings as well as Vite mappings, so a second resolution
    // surface cannot hide an unsafe target from source checks.
    aliases.push({ find, replacement: replacements[0], typescript: true });
  }
  return { pkg, aliases };
}

export async function auditSource(root) {
  root = path.resolve(root);
  const sourceRoot = path.join(root, 'src');
  const files = new Map();
  const findings = [];
  const allowedCss = new Set(['src/theme.css', 'src/app.css']);
  const complain = (file, message) =>
    findings.push(`${relative(root, file)}: ${message}`);
  async function inventory(directory) {
    const before = await lstat(directory);
    if (
      !before.isDirectory() ||
      before.isSymbolicLink() ||
      (await realpath(directory)) !== directory
    )
      throw new Error(`Unsafe source directory: ${directory}`);
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      const stat = await lstat(file);
      if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) {
        complain(
          file,
          'source symlinks/nonregular source entries are forbidden'
        );
        continue;
      }
      if (stat.isDirectory()) await inventory(file);
      else files.set(file, await readSource(file));
    }
    const after = await lstat(directory);
    if (
      before.dev !== after.dev ||
      before.ino !== after.ino ||
      before.mtimeMs !== after.mtimeMs ||
      before.ctimeMs !== after.ctimeMs
    )
      throw new Error(`Source directory changed during audit: ${directory}`);
  }
  await inventory(sourceRoot);
  const { pkg, aliases } = await configuration(root);
  const dependencies = { ...pkg.dependencies, ...pkg.devDependencies };
  const cssImports = [];
  function local(file, target, specifier) {
    if (
      !inside(sourceRoot, target) ||
      forbiddenPath.test(relative(sourceRoot, target))
    ) {
      complain(file, `forbidden production import ${specifier}`);
      return;
    }
    const candidates = [
      target,
      ...[
        '.ts',
        '.tsx',
        '.js',
        '.jsx',
        '.mts',
        '.mjs',
        '.cts',
        '.cjs',
        '.svelte'
      ].map((extension) => target + extension),
      ...['.ts', '.tsx', '.js', '.jsx', '.mts', '.mjs', '.svelte'].map(
        (extension) => path.join(target, 'index' + extension)
      )
    ];
    // TypeScript source commonly retains a .js extension for its emitted path.
    if (/\.[cm]?js$/.test(target))
      candidates.push(
        target
          .replace(/\.js$/, '.ts')
          .replace(/\.mjs$/, '.mts')
          .replace(/\.cjs$/, '.cts')
      );
    const found = candidates.find((candidate) => files.has(candidate));
    if (!found)
      return complain(file, `unresolved production import ${specifier}`);
    const name = relative(root, found);
    if (/\.(css|scss|sass|less)$/.test(found)) {
      if (
        relative(root, file) !== 'src/routes/+layout.svelte' ||
        !allowedCss.has(name)
      )
        complain(file, 'CSS must be imported by the root layout');
      else cssImports.push(name);
    } else if (!codeExtension.test(found))
      complain(file, `unsupported production import ${specifier}`);
    return found;
  }
  function resolve(file, specifier, seen = new Set(), unrestricted = false) {
    if (
      typeof specifier !== 'string' ||
      /[\\?#]/.test(specifier.replace(/^#/, '')) ||
      [...specifier].some((character) => character.charCodeAt(0) <= 32) ||
      specifier.includes('%') ||
      specifier.startsWith('/')
    )
      return complain(file, `unsafe production import ${specifier}`);
    if (seen.has(specifier))
      return complain(file, `cyclic import alias ${specifier}`);
    seen = new Set([...seen, specifier]);
    const matches = aliases.filter(({ find }) =>
      find.includes('*')
        ? specifier.startsWith(find.split('*')[0]) &&
          specifier.endsWith(find.split('*')[1])
        : specifier === find ||
          (!find.startsWith('#') && specifier.startsWith(find + '/'))
    );
    if (matches.length) {
      const identities = [];
      for (const alias of matches) {
        const tail = alias.find.includes('*')
          ? specifier.slice(
              alias.find.indexOf('*'),
              specifier.length - alias.find.split('*')[1].length || undefined
            )
          : specifier.slice(alias.find.length);
        const target = alias.replacement.includes('*')
          ? alias.replacement.replace('*', tail)
          : alias.replacement + tail;
        if (
          alias.typescript ||
          target.startsWith('.') ||
          path.isAbsolute(target)
        )
          identities.push(local(file, path.resolve(root, target), specifier));
        else identities.push(resolve(file, target, seen, unrestricted));
      }
      return identities.every((value) => value === identities[0])
        ? identities[0]
        : null;
    }
    if (specifier.startsWith('#')) {
      const entries = Object.entries(pkg.imports ?? {}).filter(
        ([key]) =>
          key === specifier ||
          (key.endsWith('/*') && specifier.startsWith(key.slice(0, -1)))
      );
      if (!entries.length)
        return complain(file, `unresolved production import ${specifier}`);
      // Audit all conditions, not only the current host's default branch.
      const targets = (value) =>
        typeof value === 'string'
          ? [value]
          : value && typeof value === 'object' && !Array.isArray(value)
            ? Object.values(value).flatMap(targets)
            : (() => {
                throw new Error(`Unsafe package import ${specifier}`);
              })();
      const identities = [];
      for (const [key, value] of entries)
        for (const raw of targets(value)) {
          const target = key.endsWith('/*')
            ? raw.replace('*', specifier.slice(key.length - 1))
            : raw;
          if (target.startsWith('.'))
            identities.push(local(file, path.resolve(root, target), specifier));
          else identities.push(resolve(file, target, seen, unrestricted));
        }
      return identities.every((value) => value === identities[0])
        ? identities[0]
        : null;
    }
    if (specifier.startsWith('.'))
      return local(
        file,
        path.resolve(path.dirname(file), specifier),
        specifier
      );
    if (frameworkModules.has(specifier)) return specifier;
    const packageName = specifier.startsWith('@')
      ? specifier.split('/').slice(0, 2).join('/')
      : specifier.split('/')[0];
    const declared = dependencies[packageName];
    const actualName =
      typeof declared === 'string' && declared.startsWith('npm:')
        ? declared.slice(4).replace(/@[^@]*$/, '')
        : packageName;
    if (
      unrestricted &&
      actualName === 'svelte' &&
      !specifier.slice(packageName.length).startsWith('/store')
    )
      return complain(
        file,
        'unrestricted Svelte raw-render module access is forbidden'
      );
    const actualImport = actualName + specifier.slice(packageName.length);
    // These upstream primitives have no blanket runtime-package admission.
    // Only the exact reviewed pure adapters and exact package exports/pins may
    // use them. Source changes require a new independent review of this scope.
    const primitiveAdmissions = {
      '@noble/curves': {
        version: '1.2.0',
        import: '@noble/curves/secp256k1',
        file: 'src/lib/contracts/public-key.ts',
        sha256:
          'd36d4001c2f4835e24d50f00460a35c9cf9fa6b86cc3b4a6937d8b4e10afdeb9'
      },
      '@scure/base': {
        version: '1.1.1',
        import: '@scure/base',
        file: 'src/lib/nostr/references.ts',
        sha256:
          'b417864961a184cab732bebab4fb35089034dbb6a742068d2742f7b353525d80'
      }
    };
    const primitive = Object.hasOwn(primitiveAdmissions, actualName)
      ? primitiveAdmissions[actualName]
      : undefined;
    const admittedPrimitive =
      primitive !== undefined &&
      declared === primitive.version &&
      actualImport === primitive.import &&
      relative(root, file) === primitive.file &&
      createHash('sha256').update(files.get(file)).digest('hex') ===
        primitive.sha256;
    if (
      (actualName === '@sveltejs/kit' &&
        !['@sveltejs/kit', '@sveltejs/kit/hooks'].includes(actualImport)) ||
      (actualName === 'svelte' &&
        /^(svelte\/(compiler|internal|server|attachments|package\.json))/.test(
          actualImport
        ))
    )
      return complain(file, `forbidden production import ${specifier}`);
    if (
      applesauce.has(actualName) &&
      !relative(root, file).startsWith('src/lib/nostr/')
    )
      complain(file, 'Applesauce imports belong in lib/nostr');
    if (
      (!runtimePackages.has(actualName) && !admittedPrimitive) ||
      typeof declared !== 'string' ||
      /^(file:|link:|workspace:|git|https?:|\.\.?\/)/.test(declared)
    )
      return complain(file, `forbidden production import ${specifier}`);
    if (
      !/^(?:npm:(?:@[^/]+\/)?[^@]+@)?\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(
        declared
      )
    )
      return complain(file, `unpinned production import ${specifier}`);
    // Direct package imports must be exported by the installed, pinned package;
    // never search parent node_modules or fall back to a sibling workspace.
    try {
      const resolved = createRequire(path.join(root, 'package.json')).resolve(
        specifier
      );
      if (!inside(path.join(root, 'node_modules'), resolved))
        throw new Error('outside web dependencies');
    } catch {
      complain(file, `unresolved production import ${specifier}`);
    }
  }
  const helperFile = path.join(root, 'src/lib/navigation-url.ts');
  const helperPin =
    '4ce96334d7cf57ccba518132078278b71d3bf70f519202e491a5f6a8840f7b19';
  const helperTrusted =
    files.has(helperFile) &&
    createHash('sha256').update(files.get(helperFile)).digest('hex') ===
      helperPin;
  if (files.has(helperFile) && !helperTrusted)
    complain(
      helperFile,
      'navigation helper source identity does not match reviewed pin'
    );
  function exportIdentity(file, exported, seen = new Set()) {
    const key = file + ':' + exported;
    if (seen.has(key)) return null;
    seen = new Set([...seen, key]);
    if (file === '$app/navigation') return exported === 'goto' ? 'goto' : null;
    if (
      file === helperFile &&
      helperTrusted &&
      ['internalHref', 'navigationHref'].includes(exported)
    )
      return exported;
    if (
      !files.has(file) ||
      !codeExtension.test(file) ||
      file.endsWith('.svelte')
    )
      return null;
    const ast = sourceAst(file, files.get(file));
    const matches = [];
    for (const node of ast.statements) {
      if (
        ts.isExportDeclaration(node) &&
        !node.isTypeOnly &&
        node.moduleSpecifier &&
        node.exportClause &&
        ts.isNamedExports(node.exportClause)
      ) {
        for (const element of node.exportClause.elements)
          if (!element.isTypeOnly && element.name.text === exported)
            matches.push(
              exportIdentity(
                resolve(file, node.moduleSpecifier.text),
                (element.propertyName ?? element.name).text,
                seen
              )
            );
      }
    }
    return matches.length === 1 ? matches[0] : null;
  }
  function navigationExports(file, seen = new Set()) {
    if (file === '$app/navigation') return true;
    if (
      seen.has(file) ||
      !files.has(file) ||
      !codeExtension.test(file) ||
      file.endsWith('.svelte')
    )
      return false;
    seen = new Set([...seen, file]);
    const ast = sourceAst(file, files.get(file));
    for (const node of ast.statements) {
      if (
        !ts.isExportDeclaration(node) ||
        node.isTypeOnly ||
        !node.moduleSpecifier
      )
        continue;
      const target = resolve(file, node.moduleSpecifier.text);
      if (!node.exportClause || ts.isNamespaceExport(node.exportClause)) {
        if (navigationExports(target, seen)) return true;
      } else if (ts.isNamedExports(node.exportClause)) {
        if (
          node.exportClause.elements.some(
            (element) =>
              !element.isTypeOnly &&
              exportIdentity(
                target,
                (element.propertyName ?? element.name).text
              ) === 'goto'
          )
        )
          return true;
      }
    }
    return false;
  }
  for (const [file, content] of files) {
    const name = relative(root, file);
    if (/\.(css|scss|sass|less)$/.test(name) && !allowedCss.has(name))
      complain(file, 'handwritten CSS belongs in theme.css or app.css');
    if (/HC_TEST_ONLY_PROVIDER|HC_TEST_ONLY_SIGNER/.test(content))
      complain(file, 'test provider code is forbidden in production');
    if (forbiddenPath.test(relative(sourceRoot, file)))
      complain(
        file,
        'test/native/generated/credential source is forbidden in production'
      );
    if (allowedCss.has(name)) {
      // CSS loading is outside the two handwritten-file contract. Decode
      // escapes/comments before checking so escaped @import/url cannot hide.
      const css = content
        .replace(/\\([0-9a-fA-F]{1,6})\s?/g, (_, hex) =>
          String.fromCodePoint(parseInt(hex, 16))
        )
        .replace(/\\([^\r\n])/g, '$1')
        .replace(/\/\*[\s\S]*?\*\//g, '');
      if (/@import\b|url\s*\(/i.test(css))
        complain(file, 'CSS import/url loading is forbidden');
      if (
        /@(?:tailwind|apply|theme|utility|variant|custom-variant|config|plugin|source|reference)\b/i.test(
          css
        )
      )
        complain(file, 'Tailwind CSS directives are forbidden');
    }
    const html = name === 'src/app.html';
    if (!codeExtension.test(file) && !html) continue;
    const scripts = [];
    if (file.endsWith('.svelte') || html) {
      const ast = parse(content, { modern: true });
      if (ast.css)
        complain(file, 'component CSS and inline styles are forbidden');
      const staticAttribute = (attribute) => {
        if (!attribute || attribute.type !== 'Attribute') return null;
        if (
          Array.isArray(attribute.value) &&
          attribute.value.every((part) => part.type === 'Text')
        )
          return attribute.value.map((part) => part.data).join('');
        if (
          attribute.value?.type === 'ExpressionTag' &&
          attribute.value.expression?.type === 'Literal' &&
          typeof attribute.value.expression.value === 'string'
        )
          return attribute.value.expression.value;
        return null;
      };
      const elementTag = (node) => {
        const tag =
          node.type === 'SvelteElement'
            ? node.tag?.type === 'Literal' && typeof node.tag.value === 'string'
              ? node.tag.value
              : null
            : node.name;
        return tag?.toLowerCase().split(':').pop();
      };
      const inspectElement = (node) => {
        const tag = elementTag(node);
        if (node.type === 'SvelteElement' && !tag)
          return complain(
            file,
            'computed markup resource elements are forbidden'
          );
        if (declarativeMutationTags.has(tag))
          complain(
            file,
            'declarative SVG resource/style mutation is forbidden'
          );
        if (
          [
            'script',
            'style',
            'iframe',
            'object',
            'embed',
            'applet',
            'base'
          ].includes(tag)
        )
          complain(
            file,
            'raw markup scripts/styles/embedded resources are forbidden'
          );
        const attributes = node.attributes ?? [];
        const values = new Map();
        for (const attribute of attributes) {
          if (attribute.type === 'SpreadAttribute')
            complain(file, 'computed markup attributes are forbidden');
          if (attribute.type !== 'Attribute') continue;
          const key = attribute.name.toLowerCase();
          if (values.has(key))
            complain(
              file,
              'duplicate markup resource attributes are forbidden'
            );
          values.set(key, attribute);
          const value = staticAttribute(attribute);
          if (key === 'srcdoc')
            complain(file, 'embedded markup documents are forbidden');
          if (/^on[a-z]/.test(key) && (html || value !== null))
            complain(file, 'raw markup event scripts are forbidden');
          if (
            ['href', 'src', 'xlink:href', 'action', 'formaction'].includes(
              key
            ) &&
            value !== null &&
            /^(?:javascript|vbscript|data):/i.test(
              [...value]
                .filter((character) => character.charCodeAt(0) > 32)
                .join('')
            )
          )
            complain(file, 'executable markup resource URLs are forbidden');
        }
        if (tag === 'link') {
          const rel = staticAttribute(values.get('rel'));
          const href = staticAttribute(values.get('href'));
          const tokens = rel?.toLowerCase().trim().split(/\s+/) ?? [];
          const safeRelations = new Set([
            'icon',
            'shortcut',
            'apple-touch-icon',
            'mask-icon',
            'canonical',
            'alternate',
            'author',
            'license',
            'help'
          ]);
          if (
            !tokens.length ||
            tokens.some((token) => !safeRelations.has(token))
          )
            complain(
              file,
              'markup stylesheet/client resource links are forbidden'
            );
          // Resource selectors must stay literal. Spreads and computed href,
          // rel, as, type or resource URLs cannot smuggle another loader mode.
          if (
            rel === null ||
            href === null ||
            ['as', 'type'].some(
              (key) =>
                values.has(key) && staticAttribute(values.get(key)) === null
            )
          )
            complain(file, 'computed markup resource attributes are forbidden');
          if (href !== null) {
            const url = href.replace(/^%sveltekit.assets%/, '');
            if (
              /[\\\s]/.test(url) ||
              [...url].some((character) => character.charCodeAt(0) < 32) ||
              /^(?:javascript|vbscript|data):/i.test(url)
            )
              complain(file, 'unsafe markup resource URL');
            if (
              tokens.some((token) =>
                ['icon', 'shortcut', 'apple-touch-icon', 'mask-icon'].includes(
                  token
                )
              ) &&
              (!/^\/(?!\/)[a-zA-Z0-9_./-]+\.(?:ico|png|svg)$/.test(url) ||
                url.split('/').includes('..'))
            )
              complain(
                file,
                'favicon resources must use literal owned image paths'
              );
          }
        }
        if (tag === 'meta' && values.has('http-equiv')) {
          const directive = staticAttribute(values.get('http-equiv'));
          if (directive === null || directive.toLowerCase() === 'refresh')
            complain(
              file,
              'markup refresh/executable directives are forbidden'
            );
        }
      };
      const visit = (node) => {
        if (!node || typeof node !== 'object') return;
        if (
          node.type === 'RegularElement' ||
          node.type === 'SvelteElement' ||
          (node.type === 'Component' &&
            declarativeMutationTags.has(elementTag(node))) ||
          (html && node.name && Array.isArray(node.attributes))
        )
          inspectElement(node);
        if (
          (node.type === 'BindDirective' && node.name === 'this') ||
          [
            'UseDirective',
            'AttachTag',
            'TransitionDirective',
            'AnimateDirective'
          ].includes(node.type)
        )
          complain(
            file,
            'DOM bindings/actions/attachments cannot expose opaque mutation capabilities'
          );
        if (node.type === 'HtmlTag')
          complain(file, 'raw markup injection is forbidden');
        if (
          node.type === 'StyleDirective' ||
          (node.type === 'Attribute' && node.name.toLowerCase() === 'style')
        )
          complain(file, 'inline styles are forbidden');
        for (const value of Object.values(node)) {
          if (Array.isArray(value)) value.forEach(visit);
          else if (value && typeof value === 'object') visit(value);
        }
      };
      visit(ast.fragment);
      for (const script of [ast.instance, ast.module]) {
        if (!script) continue;
        if (
          html ||
          script.attributes?.some(
            (attribute) =>
              !['lang', 'context', 'module'].includes(
                attribute.name?.toLowerCase()
              )
          )
        )
          complain(
            file,
            'inline HTML scripts or external scripts are forbidden'
          );
      }
      const expression = (node) => content.slice(node.start, node.end);
      const attributeExpression = (attribute) => {
        const parts = Array.isArray(attribute.value)
          ? attribute.value
          : [attribute.value];
        if (
          !parts.length ||
          parts.some(
            (part) => !part || !['Text', 'ExpressionTag'].includes(part.type)
          )
        )
          return 'undefined';
        return parts
          .map((part) =>
            part.type === 'Text'
              ? JSON.stringify(part.data)
              : '(' + expression(part.expression) + ')'
          )
          .join(' + ');
      };
      // One lexical graph: module declarations enclose instance declarations,
      // and template bindings enclose their own branch bodies. Detached
      // expression parses cannot prove imported identity or shadowing.
      const emit = (node) => {
        if (!node || typeof node !== 'object') return '';
        if (node.type === 'Fragment')
          return '{' + node.nodes.map(emit).join('\n') + '}';
        if (node.type === 'ConstTag')
          return node.declaration.declarations
            .map(
              (decl) =>
                'const ' +
                expression(decl.id) +
                ' = ' +
                expression(decl.init) +
                ';'
            )
            .join('\n');
        if (node.type === 'EachBlock')
          return (
            '(' +
            expression(node.expression) +
            '); { let ' +
            expression(node.context) +
            ';' +
            (node.index ? 'let ' + node.index + ';' : '') +
            (node.key ? '(' + expression(node.key) + ');' : '') +
            emit(node.body) +
            '}' +
            emit(node.fallback)
          );
        if (node.type === 'SnippetBlock')
          return (
            'function ' +
            expression(node.expression) +
            '(' +
            node.parameters.map(expression).join(',') +
            ')' +
            emit(node.body)
          );
        if (node.type === 'AwaitBlock')
          return (
            '(' +
            expression(node.expression) +
            ');' +
            emit(node.pending) +
            '{' +
            (node.value ? 'let ' + expression(node.value) + ';' : '') +
            emit(node.then) +
            '}{' +
            (node.error ? 'let ' + expression(node.error) + ';' : '') +
            emit(node.catch) +
            '}'
          );
        let result = '';
        if (node.attributes) {
          const tag = elementTag(node);
          for (const attribute of node.attributes) {
            if (attribute.type === 'LetDirective')
              result +=
                'let ' +
                (attribute.expression
                  ? expression(attribute.expression)
                  : attribute.name) +
                ';';
            if (
              node.type !== 'Component' &&
              attribute.type === 'Attribute' &&
              [
                'href',
                'src',
                'xlink:href',
                'action',
                'formaction',
                'srcset',
                'poster'
              ].includes(attribute.name.toLowerCase())
            ) {
              const resource =
                !(
                  (tag === 'a' || tag === 'area') &&
                  attribute.name.toLowerCase() === 'href'
                ) &&
                !['action', 'formaction'].includes(
                  attribute.name.toLowerCase()
                );
              let value = attributeExpression(attribute);
              if (tag === 'link' && staticAttribute(attribute) !== null)
                value = JSON.stringify(
                  staticAttribute(attribute).replace(/^%sveltekit.assets%/, '')
                );
              result +=
                (resource ? '__hcrResourceUrl' : '__hcrNavigationUrl') +
                '(' +
                value +
                ');';
              if (['srcset', 'poster'].includes(attribute.name.toLowerCase()))
                complain(file, 'media resource attributes are forbidden');
            }
          }
        }
        if (node.expression?.start !== undefined)
          result += '(' + expression(node.expression) + ');';
        for (const [key, value] of Object.entries(node)) {
          if (
            [
              'expression',
              'context',
              'parameters',
              'declaration',
              'error'
            ].includes(key)
          )
            continue;
          if (Array.isArray(value)) result += value.map(emit).join('\n');
          else if (value && typeof value === 'object') result += emit(value);
        }
        return node.attributes ? '{' + result + '}' : result;
      };
      scripts.push(
        (ast.module
          ? content.slice(ast.module.content.start, ast.module.content.end)
          : '') +
          '\nfunction __hcrInstance() {\n' +
          (ast.instance
            ? content.slice(
                ast.instance.content.start,
                ast.instance.content.end
              )
            : '') +
          '\n' +
          emit(ast.fragment) +
          '\n}'
      );
    } else scripts.push(content);
    for (const script of scripts) {
      const ast = sourceAst(file, script);
      const ownedModel = mutationAdmission(
        ast,
        (message) => complain(file, message),
        (specifier, exported) => {
          const target = resolve(file, specifier);
          return exported === null
            ? navigationExports(target)
              ? 'goto'
              : null
            : exportIdentity(target, exported);
        }
      );
      if (ast.referencedFiles.length || ast.typeReferenceDirectives.length)
        complain(file, 'source reference directives are forbidden');
      const inspect = (node) => {
        if (
          ts.isExportDeclaration(node) &&
          node.moduleSpecifier &&
          (!node.exportClause || ts.isNamespaceExport(node.exportClause)) &&
          navigationExports(resolve(file, node.moduleSpecifier.text))
        )
          complain(
            file,
            'escaped navigation star/namespace reexport is forbidden'
          );
        if (
          (ts.isImportDeclaration(node) ||
            ts.isExportDeclaration(node) ||
            ts.isJSDocImportTag(node)) &&
          node.moduleSpecifier
        )
          resolve(
            file,
            node.moduleSpecifier.text,
            new Set(),
            (ts.isImportDeclaration(node) &&
              ((node.importClause?.namedBindings &&
                ts.isNamespaceImport(node.importClause.namedBindings)) ||
                node.importClause?.name)) ||
              (ts.isExportDeclaration(node) && !node.exportClause)
          );
        if (
          (ts.isImportSpecifier(node) || ts.isExportSpecifier(node)) &&
          (node.propertyName ?? node.name).text === 'createRawSnippet'
        )
          complain(file, 'Svelte raw-render factories are forbidden');
        if (ts.isImportTypeNode(node)) {
          if (
            ts.isLiteralTypeNode(node.argument) &&
            ts.isStringLiteral(node.argument.literal)
          )
            resolve(file, node.argument.literal.text);
          else complain(file, 'unresolved production import type');
        }
        if (
          ts.isNewExpression(node) &&
          node.expression.getText(ast) === 'URL' &&
          node.arguments?.[1]?.getText(ast) === 'import.meta.url'
        ) {
          if (ts.isStringLiteralLike(node.arguments[0]))
            resolve(file, node.arguments[0].text);
          else complain(file, 'computed production imports are forbidden');
        }
        if (ts.isImportEqualsDeclaration(node))
          complain(file, 'require/import-equals loaders are forbidden');
        if (
          ts.isCallExpression(node) &&
          node.expression.kind === ts.SyntaxKind.ImportKeyword
        ) {
          if (
            node.arguments.length !== 1 ||
            !ts.isStringLiteralLike(node.arguments[0])
          )
            complain(file, 'computed production imports are forbidden');
          else resolve(file, node.arguments[0].text, new Set(), true);
        }
        if (
          ts.isIdentifier(node) &&
          [
            'createRawSnippet',
            'require',
            'eval',
            'Function',
            'WebSocket',
            'Worker',
            'SharedWorker',
            'importScripts',
            'AudioContext',
            'OfflineAudioContext',
            'AudioWorkletNode',
            'generateSecretKey',
            'getSecretKey',
            'setSecretKey',
            'testSigner',
            'mockSigner',
            'testProvider',
            'mockProvider'
          ].includes(node.text)
        )
          complain(
            file,
            `production loader/provider/credential API is forbidden: ${node.text}`
          );
        if (
          (ts.isPropertyAccessExpression(node) ||
            ts.isElementAccessExpression(node)) &&
          !ownedModel(node.expression) &&
          [
            'serviceWorker',
            'audioWorklet',
            'paintWorklet',
            'addModule'
          ].includes(
            ts.isPropertyAccessExpression(node)
              ? node.name.text
              : ts.isStringLiteralLike(node.argumentExpression)
                ? node.argumentExpression.text
                : ''
          )
        )
          complain(file, 'browser worker/client loaders are forbidden');
        if (
          ts.isPropertyAccessExpression(node) &&
          node.expression.getText(ast) === 'import.meta' &&
          ['glob', 'globEager', 'resolve'].includes(node.name.text)
        )
          complain(file, 'computed production imports are forbidden');
        if (
          (ts.isPropertyAccessExpression(node) ||
            ts.isElementAccessExpression(node)) &&
          /^(window|globalThis)\b/.test(node.expression.getText(ast)) &&
          (ts.isElementAccessExpression(node) ||
            ['WebSocket', 'eval', 'Function'].includes(node.name.text))
        )
          complain(file, 'indirect production loaders are forbidden');
        if (
          ts.isPropertyAccessExpression(node) &&
          !ownedModel(node.expression) &&
          [
            'style',
            'cssText',
            'styleSheets',
            'insertRule',
            'adoptedStyleSheets'
          ].includes(node.name.text)
        )
          complain(file, 'JS-generated inline styles are forbidden');
        if (
          ts.isCallExpression(node) &&
          ts.isPropertyAccessExpression(node.expression) &&
          ['setAttribute', 'createElement'].includes(
            node.expression.name.text
          ) &&
          node.arguments.some(
            (argument) =>
              ts.isStringLiteralLike(argument) &&
              argument.text.toLowerCase() === 'style'
          )
        )
          complain(file, 'JS-generated inline styles are forbidden');
        if (
          ts.isPropertyAccessExpression(node) &&
          !ownedModel(node.expression) &&
          ['innerHTML', 'outerHTML', 'srcdoc', 'insertAdjacentHTML'].includes(
            node.name.text
          )
        )
          complain(file, 'raw markup injection APIs are forbidden');
        if (
          ts.isElementAccessExpression(node) &&
          !ownedModel(node.expression) &&
          ts.isStringLiteralLike(node.argumentExpression) &&
          ['innerHTML', 'outerHTML', 'srcdoc', 'style', 'cssText'].includes(
            node.argumentExpression.text
          )
        )
          complain(file, 'raw markup/style injection APIs are forbidden');
        if (
          ts.isCallExpression(node) &&
          (ts.isPropertyAccessExpression(node.expression) ||
            ts.isElementAccessExpression(node.expression))
        ) {
          const method = ts.isPropertyAccessExpression(node.expression)
            ? node.expression.name.text
            : ts.isStringLiteralLike(node.expression.argumentExpression)
              ? node.expression.argumentExpression.text
              : null;
          if (method === null)
            complain(file, 'computed markup/API calls are forbidden');
          if (['createElement', 'createElementNS'].includes(method)) {
            const tag = node.arguments[method === 'createElementNS' ? 1 : 0];
            if (!tag || !ts.isStringLiteralLike(tag))
              complain(
                file,
                'computed markup resource constructors are forbidden'
              );
            else if (
              declarativeMutationTags.has(
                tag.text.toLowerCase().split(':').pop()
              )
            )
              complain(
                file,
                'declarative SVG resource/style constructors are forbidden'
              );
            else if (
              [
                'script',
                'style',
                'link',
                'iframe',
                'object',
                'embed',
                'applet',
                'base'
              ].includes(tag.text.toLowerCase().split(':').pop())
            )
              complain(file, 'raw markup resource constructors are forbidden');
          }
          if (
            ['write', 'writeln'].includes(method) &&
            /(?:^|\.)document$/.test(node.expression.expression.getText(ast))
          )
            complain(file, 'raw markup document writers are forbidden');
        }
        const accessedMethod = ts.isPropertyAccessExpression(node)
          ? node.name.text
          : ts.isElementAccessExpression(node) &&
              ts.isStringLiteralLike(node.argumentExpression)
            ? node.argumentExpression.text
            : null;
        if (
          ['createElement', 'createElementNS'].includes(accessedMethod) &&
          (!ts.isCallExpression(node.parent) || node.parent.expression !== node)
        )
          complain(file, 'indirect markup resource constructors are forbidden');
        for (const doc of node.jsDoc ?? []) inspect(doc);
        ts.forEachChild(node, inspect);
      };
      inspect(ast);
    }
  }
  const presentCss = [...allowedCss].filter((file) =>
    files.has(path.join(root, file))
  );
  // Primitives may precede compositions; importing either file activates the complete pair contract.
  const unimportedTheme =
    presentCss.length === 1 &&
    presentCss[0] === 'src/theme.css' &&
    cssImports.length === 0;
  if (
    presentCss.length &&
    !unimportedTheme &&
    (presentCss.length !== 2 ||
      cssImports.join(',') !== 'src/theme.css,src/app.css')
  )
    findings.push(
      'CSS must be imported exactly once, theme.css then app.css, by the root layout'
    );
  return findings;
}
