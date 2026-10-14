import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { constants } from 'node:fs';
import { lstat, open, opendir, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { deriveBuildInfo } from './build-info.mjs';
import ts from 'typescript';
import { parse as parseSvelte } from 'svelte/compiler';

/** @param {unknown} actual @param {unknown} expected @param {string} [message] */
function equal(actual, expected, message = 'Invalid public output shape') {
  if (!isDeepStrictEqual(actual, expected)) throw new Error(message);
}
/** @param {string} text */
function parse(text) {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error('Invalid static JSON');
  }
}
const maxBytes = 8 * 1024 * 1024;
/** @param {string} name */
function safeName(name) {
  if (
    !name ||
    name.includes('\\') ||
    name.split('/').some((part) => !part || part === '.' || part === '..') ||
    [...name].some(
      (character) =>
        character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127
    )
  )
    throw new Error('Unsafe output path');
}
/** @param {string} root @param {string} name */
async function readOwned(root, name) {
  safeName(name);
  const parts = name.split('/');
  let file = root;
  for (let i = 0; i < parts.length; i++) {
    file = path.join(file, parts[i]);
    const stat = await lstat(file);
    if (
      i < parts.length - 1
        ? !stat.isDirectory()
        : !stat.isFile() ||
          stat.nlink !== 1 ||
          Boolean(stat.mode & 0o111) ||
          stat.size > maxBytes
    )
      throw new Error('Unsafe output entry');
  }
  if ((await realpath(file)) !== file)
    throw new Error('Unsafe output ancestry');
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (
      !stat.isFile() ||
      stat.nlink !== 1 ||
      Boolean(stat.mode & 0o111) ||
      stat.size > maxBytes
    )
      throw new Error('Unsafe output entry');
    const chunks = [];
    let size = 0;
    const buffer = Buffer.alloc(64 * 1024);
    for (;;) {
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > maxBytes) throw new Error('Oversized output entry');
      chunks.push(Buffer.from(buffer.subarray(0, bytesRead)));
    }
    return Buffer.concat(chunks);
  } finally {
    await handle.close();
  }
}
/** @param {string} root */
async function inventory(root) {
  if (!(await lstat(root)).isDirectory() || (await realpath(root)) !== root)
    throw new Error('Unsafe output root');
  const files = new Map();
  let entries = 0;
  let total = 0;
  /** @param {string} prefix */
  async function walk(prefix) {
    let childCount = 0;
    for await (const entry of await opendir(path.join(root, prefix))) {
      childCount++;
      if (++entries > 512) throw new Error('Unbounded output inventory');
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
      safeName(name);
      if (name.split('/').length > 8) throw new Error('Unbounded output depth');
      const stat = await lstat(path.join(root, name));
      if (stat.isDirectory()) {
        await walk(name);
      } else {
        const bytes = await readOwned(root, name);
        total += bytes.length;
        if (total > 32 * 1024 * 1024) throw new Error('Unbounded output bytes');
        files.set(name, bytes);
      }
    }
    if (prefix && !childCount)
      throw new Error('Undeclared empty output directory');
  }
  await walk('');
  return files;
}
/** @param {Uint8Array} bytes */
function publicText(bytes) {
  const buffer = Buffer.from(bytes);
  // Controlled binary/container/database signatures, irrespective of filename.
  const magic = [
    '7f454c46',
    'cffaedfe',
    'cefaedfe',
    'feedfacf',
    'feedface',
    'cafebabe',
    'bebafeca',
    '504b0304',
    '504b0506',
    '1f8b',
    '0061736d'
  ];
  if (
    magic.some(
      (hex) => buffer.subarray(0, hex.length / 2).toString('hex') === hex
    ) ||
    buffer.subarray(0, 2).toString() === 'MZ' ||
    buffer.subarray(0, 16).toString() === 'SQLite format 3\0'
  )
    throw new Error('Static binary contamination');
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  if (
    [...text].some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 && ![9, 10, 13].includes(code);
    })
  )
    throw new Error('Invalid static text');
  // These are bounded contamination controls, not an arbitrary-secret scanner.
  if (
    /UniFFI|HC_TEST_ONLY_SECRET|nsec1[023456789acdefghjklmnpqrstuvwxyz]{20,}|-----BEGIN (?:[A-Z ]*PRIVATE KEY|OPENSSH)|private[_ -]transcript|TestSigner|test[_ -]provider|createRelayHarness|createStaticHarness|installControlledProvider|sourceMappingURL|radroots\.lib\.source-lock|core\/Cargo\.lock|docs\/oss\/harvestcircle|HARVESTCIRCLE_SECRET/i.test(
      text
    )
  )
    throw new Error('Static controlled contamination');
  return text;
}
/** @param {unknown} value @param {string[]} keys */
function objectKeys(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid compiler object');
  if (Object.keys(value).some((key) => !keys.includes(key)))
    throw new Error('Unknown compiler field');
}
const sdkNegentropy =
  'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js';
// Keep every root byte except ordinary trailing instance-script trivia. This
// preserves the owned reader-boundary positive without erasing annotations,
// changing executable source, or normalizing the template or script attributes.
/** @param {Buffer} bytes */
export function publicHideRootIdentity(bytes) {
  const layout = publicText(bytes);
  const component = parseSvelte(layout);
  if (!component.instance || component.module)
    throw new Error('Invalid owned root runtime activation');
  const { start, end } = component.instance.content;
  const script = layout.slice(start, end);
  const source = ts.createSourceFile(
    'root.ts',
    script,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  /** @type {unknown} */
  const diagnostics = Reflect.get(source, 'parseDiagnostics');
  if (
    !Array.isArray(diagnostics) ||
    diagnostics.length ||
    !source.statements.length
  )
    throw new Error('Invalid owned root runtime activation');
  const last = source.statements[source.statements.length - 1].end;
  const scanner = ts.createScanner(
    ts.ScriptTarget.Latest,
    false,
    ts.LanguageVariant.Standard,
    script.slice(last)
  );
  for (
    let token = scanner.scan();
    token !== ts.SyntaxKind.EndOfFileToken;
    token = scanner.scan()
  ) {
    if (
      token === ts.SyntaxKind.WhitespaceTrivia ||
      token === ts.SyntaxKind.NewLineTrivia
    )
      continue;
    if (
      token !== ts.SyntaxKind.MultiLineCommentTrivia ||
      !scanner.getTokenText().endsWith('*/') ||
      /[@#<]|PURE|NO_SIDE_EFFECTS|vite|webpack|sourceMappingURL|sourceURL/.test(
        scanner.getTokenText()
      )
    )
      throw new Error('Invalid owned root runtime activation');
  }
  return createHash('sha256')
    .update(
      JSON.stringify([
        layout.slice(0, start),
        script.slice(0, last),
        layout.slice(end)
      ])
    )
    .digest('hex');
}
// Compiler admission follows actual root bindings, never copied files or text
// matches. This is the deliberately fixed lifecycle shape owned by HCP032.
/** @param {string} web */
async function operationalFlagsAdmission(web) {
  const shell = publicText(
    await readOwned(web, 'src/lib/components/AppShell.svelte')
  );
  if (!shell.includes('../runtime/capabilities.ts')) return false;
  /** @type {Record<string,string>} */
  const producers = {
    // Exact previous AccountGate remains compatibility history; the only
    // current alternative changes its obsolete condition copy below.
    'src/lib/components/AccountGate.svelte':
      '642925e1819628263810dd42624e97cadf29818f84a42ef49a86ad1f196888ca',
    'src/lib/catalog/observations.ts':
      '7386470e698156f2480de1d6c9fcffacab507f97d2f4006df926db467327120c',
    'src/lib/catalog/product-view.ts':
      '3558230c9f7cd17666e27c032de4ba6f30d2fb6b4d54149a69e5b8f5dc5d5363',
    'src/lib/catalog/search-run.ts':
      '213a58cf897456a0dc0e99e46594cba013952ff08212802fb471f0d323c38573',
    'src/lib/catalog/search-view.ts':
      'bc4d0160574084a547d54d5ca582b15382f44d4649c255743ebdb4c8b398f795',
    'src/lib/components/AppShell.svelte':
      '90430e0503f2c1f2e22a4cf55ca6ec56985d1f2a15e17f13bcc6ccd5036f31e0',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/components/ConversationOptions.svelte':
      '032a5f49d19a43a8ee88495b92d5337f1bb2de5dc58bb600b579e1423c9dfab1',
    'src/lib/components/ConversationRow.svelte':
      'c5d0926ac9cdda18a6befe365227dfe62405f71598f59964ff36af9a0e85d743',
    'src/lib/components/ConversationThread.svelte':
      '07d7421ca00f5f0c6e68b38b790f863ca5c0d01de6567ac3875ef719b83462f4',
    'src/lib/components/InboxStatus.svelte':
      'a1015aac1d6c9ebb7a54bd5746e4cbcb4b93e376a30a2cbd7f7df212713fc930',
    'src/lib/components/ListingEditor.svelte':
      '593c28fbce8b010d57ee6e79f11fdbeeba07787f20d868ad83024349a4d8a9f5',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/components/LocalDataReview.svelte':
      '63b259ed2c7daa552a6c54665da74f45275ec45c60b415207a008a4ac1aa5b37',
    'src/lib/components/MessageComposer.svelte':
      'c3ab09ae9a2dc5ac8268cc45218140c5480dcb9bcf8b8bce54d2ae35ffdb2d14',
    'src/lib/components/MessageItem.svelte':
      '5c9af10ece18deeddd85e11453239c2a0b8e741cb84529e15c4ad87864704517',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/components/PublisherHide.svelte':
      '0fa30a68b44e9de68c3bb1edc9ed2c7fedefc2dc8a916c0a1df1f15440248927',
    'src/lib/components/ReportPreview.svelte':
      'fe133c95d640d3fa306667c613940a81783ce821471f70394671bb474c96d767',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/components/SupportLink.svelte':
      'c8608221416a47055c6b0aabe7a9d8370d3bc26e3fcd3c65214275d99ea1927e',
    'src/lib/config/budgets.ts':
      'e61806f1c2b580340e862b8dc7dd8063e90f74e03c64c721a7d112b0872f935d',
    'src/lib/config/deployment-relays.ts':
      '14443151ff819a11a4f9c71e4ff756d1fbdd7cf6c36242ac712c378e68583aa2',
    'src/lib/config/deployment-support.ts':
      'c698313143943749bb457674825b648f6af29477f07cae960f3435c035cd6bd8',
    'src/lib/config/operation-flags.ts':
      'd5e0ce74a17de7a06100dcc75886616655fbd398c657027e7d6990d97aa8a0c1',
    'src/lib/config/inbox-operators.ts':
      '52a4a739bf7434e3dea529858456ab49bbbbd517de789dac7f6407b687e5c07a',
    'src/lib/config/relays.ts':
      'ca8c63df89509728a7b87e65232f1da751621ed4777993a079730472a0923957',
    'src/lib/config/support.ts':
      'a5b4c179db6f13ebf85081960e2f7c330c3e894666fbf8f0472edb1c5c818872',
    'src/lib/contracts/local-records.ts':
      '04ddd51339a439665a94d4a4de7b23c2257d4a673873a672d4427e9764d21f3f',
    'src/lib/contracts/public-key.ts':
      'd36d4001c2f4835e24d50f00460a35c9cf9fa6b86cc3b4a6937d8b4e10afdeb9',
    'src/lib/messaging/admit-conversation.ts':
      '1ad085aa530beeabbc9488676e77cf7cd0901e61e7705471f79a47bb1df70cf1',
    'src/lib/nostr/incoming-wire.ts':
      'd471dde674b31a62961bb6fcb503cd9e54791d83c7afdaabcb74686cc78a3723',
    'src/lib/messaging/cited-product.ts':
      'bed076c0902f6453971f88f107766c331b5737e488cb8c3c5f6c941e3428778d',
    'src/lib/messaging/conversation-thread.ts':
      '7367f0e6d96a9f249514f66c2459274e856eb83462523e37264d4cd97d2e36d1',
    'src/lib/messaging/decryption-queue.ts':
      'be704e6cbe777722c3eb80ec9843f00afd7bac8c52af0c2af7294f2437414553',
    'src/lib/messaging/inbox-list.ts':
      '9cc82877bfa8dbb2a7fa41bceb842793349416271f19569f49fdd5cfc1de07e5',
    'src/lib/messaging/inbox-setup-operation.ts':
      '245c6ce88b146dc731e7ace4f16dc9b5002a87a997f5568e4811b0c4a2daf035',
    'src/lib/nostr/self-recovery-reader.ts':
      '806230d8082a48b9c2117a16060aff50312be270348d7b8c5b93d6d6368d31a1',
    'src/lib/nostr/private-publisher.ts':
      '2f939dda3504b760dbf3758632cda6ce38d3e16c6d142b814e40a0e295a77d90',
    'src/lib/nostr/private-pool.ts':
      'c5d7884b33ea8dcde6aea495c92e7a53db12cf650f4476f2b66adc4165e771c2',
    'src/lib/nostr/inbox-ingress.ts':
      'f860a094a8ba767585124c795d44b7620064f73a5b3a0170041c581c0fc64ecb',
    'src/lib/persistence/inbox-probes.ts':
      'b4d3089bcf08a5c9ac3a99dfcf8e2c2e2160a2c8996c19fccb2670614f7b4d7a',
    'src/lib/nostr/inbox-access.ts':
      'd39f218b66fb51833e962b20152a2df81adf3097ba434a4cf2172ee4a7c8e3bc',
    'src/lib/nostr/seal-builder.ts':
      '3ccc967cc348e63857243ff6f0302c1a68da2afe85ca2c37e00d5a894785fc53',
    'src/lib/nostr/giftwrap-builder.ts':
      'faf54dd80a0995ac94b4b994d3cfd015516e5e201a1fbb6340e13c836ce49e98',
    'src/lib/messaging/inbox-access-controller.ts':
      '103c22b4cd721f7e3371ff887f4e7ca5df6a535688b9e9d0c67241db21430f9a',
    'src/lib/messaging/inbox-setup-view.ts':
      'e85238d20f1bbcb23599b36e67910f3df784efa43f0fa0b0b7f3da7e7fd33c79',
    'src/lib/messaging/inbox-setup.ts':
      '7a935d8542ff8f64287d2b441a8a458d299dea4ed88beecda39cb1637dd4a9ac',
    'src/lib/messaging/inbox-sync.ts':
      '0d91f5895e7eb054b036a5cb80897a3d60a212d470029a82d11124c8536b3d2c',
    'src/lib/messaging/inbox-view.ts':
      '6871707ae7395d3679856a01e87c7d8f273848e0b9ef0dc7b94f450d8417528c',
    'src/lib/messaging/product-request-controller.ts':
      'e1fab3a2bfb910c1d8b798628cf4799cd1f7e72f1d80ebc2a67e73730b6f3940',
    'src/lib/messaging/reply-controller.ts':
      '8d2612b4788211b35ee8d56caa389bb88a53268bbba71407ee787c9dc23ee8a6',
    'src/lib/messaging/rumor-plan.ts':
      'b622889e96f911501524cd86f9fca9f2207ce853767494fb30799b00ae8275ed',
    'src/lib/persistence/private-send-reservations.ts':
      '5a57522fdb5acabecf836fb8d642a2adfb4bc7e041843b38fd1b96b673b3942d',
    'src/lib/messaging/send-identity.ts':
      'cc840afbf155e18030bea9b475884ca3e0068b79ef3da1459b661fe2822499df',
    'src/lib/messaging/unlocked-session.ts':
      'b7ecc1acea5ed210b39cfe9ec82b3d02c75f6092c5c31dd3e0f79946b319ef4e',
    'src/lib/navigation-url.ts':
      '4ce96334d7cf57ccba518132078278b71d3bf70f519202e491a5f6a8840f7b19',
    'src/lib/nostr/enquiry-references.ts':
      'a17175337ee53279f898a7c6fa1af7e77ad26c0d2c0da46f26a9766fa701d365',
    'src/lib/nostr/envelope-bounds.ts':
      'b1e1d4a0d0673f358321c978726747ddaba3ebf94dd65e67adac408bb6574dae',
    'src/lib/nostr/exports.ts':
      '683fc933fadc515bd9999ddd337d85b408e0ad4fe891832f2825c35ffbe26f89',
    'src/lib/nostr/inbox-preference-publication.ts':
      '032e40d12ecd3a55a973edb8a8b8c7c2a9c7a791425dc5e7d56aab4b0ea4c4bb',
    'src/lib/nostr/ingress.ts':
      'c433d459ad650c1fa9aa81aca24ddfaad8c251de9e43f92502e83fd5b673e772',
    'src/lib/nostr/listing-publisher.ts':
      '43a61caa6e308e15608ace5337086ef5eb5cee2c1191fe73de99405be159cfb0',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/nostr/public-store.ts':
      '330f7599f601956ed44795df1e016945e3564bbb7da6491c48f628d407de3d14',
    'src/lib/nostr/request-result.ts':
      '9bed0e13f94d38c3db471c0e750ef18d685776cb8832fedf7ddb7f92f663c2ac',
    'src/lib/nostr/request-scope.ts':
      'c6aa1005264e8ded37870c95f937d891afe67b8e07ef62dd137097c4ebb2388b',
    'src/lib/nostr/verified-envelope.ts':
      '01eb6224ece812fcafed5203c2cd117058a3d45cb8a2175b370e8cd74cdc0f77',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/persistence/database.ts':
      'd37d53378f531de376fb6d1b307b9b99e54736299a04eff56a66b4036261e170',
    'src/lib/persistence/inbox-retention.ts':
      '8bcc73be03321e60a6fbbe7ba58de14eb8fb34db5d5cdce0adee31b5467d7bb9',
    'src/lib/persistence/local-data-review.ts':
      '90379b183d0a1d8885076fbfc593d7c9ac1e351b16ce7800890107014e5cb0e4',
    'src/lib/persistence/inbox-envelope-repository.ts':
      'd3613a098c93c8881d53de12f91ffb689827926217cabcba66894885d49cb095',
    'src/lib/persistence/history-metadata-repository.ts':
      'afe4656e59bd6b365444fbda64fe6572ffa4c5323c474ee862e3cf19252b6f6d',
    'src/lib/messaging/history-pagination.ts':
      '79b22b312a126b55c80a3c6ca22724bd2c5f3ad766f8600f3647028a2624c255',
    'src/lib/persistence/private-outbox-cleanup.ts':
      '13f4e7d771b3a8d4ec7703f2026a1c8ff71582a2d2a810b9d402d58684b2e2b6',
    'src/lib/persistence/local-inventory.ts':
      'cd44ed47548453f0443400894c57c364b544781c028c4dba6e9fa096034c5965',
    'src/lib/persistence/local-private-blocks.ts':
      'd7e76c1122ff5479f261293db458a3240438ce0471c788c2f07ffaf854449cbc',
    'src/lib/persistence/local-public-hides.ts':
      '4f0e64ed0fabbe9f86014a0512e7dbd3c81711f28ca2c626e2dafa5cda349f83',
    'src/lib/persistence/private-records.ts':
      '821ef96740cf644bff4ffe5c7816c6aa097aa419bd16b8b301809b963294dafe',
    'src/lib/persistence/private-sends.ts':
      '6696f4179370cad373c06debf57b919598d53b3d25b6d2a6054a557608e4eb9c',
    'src/lib/persistence/private-storage.ts':
      'ba4a61dd1fa08fc89d00adac3c258c1f6ae7e5c1b15165facffe76ddd71d5e91',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/lib/persistence/quota.ts':
      'd82858b3de51ddb55466eedb980f37ebe3f70db6c36d0abe5f39feed30d441fc',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/publishing/prepare-listing.ts':
      '70cbd7ba19222bed0a1584961eace7d21cefded1659e2597d813bfdf0ee25c76',
    'src/lib/publishing/resume-listing.ts':
      'cf33ef15c3c7bee3ac2095bf50db4d93f0d4a3c505e944be9dfa10c79bca89c9',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/selling-view.ts':
      '7d1f07c5b128512d3ed19a73409f1c64ada9988d77ac9bf60799352261797f5b',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/runtime/capabilities.ts':
      '55a3dffc6b4b58fd96a91820c9a26a6ac951864d12ed269d8708cf61d446799f',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/runtime/identity-session.ts':
      '7d665f6872bdc780f1f2b56245730adcc90f32cf1fed6997f7e2e44ea2447b49',
    'src/lib/runtime/private-session.ts':
      'ecffe8a4a735478669f46e666e96add6d4b32c615e3741fcc2277a07ca2ef6c3',
    'src/lib/runtime/public-runtime.ts':
      '679aa4cd51a1dfdc81459bdfccb29441ac60edfa8b1ddd579425787c85b5cff4',
    'src/lib/runtime/view-context.ts':
      'e2329113e29db35e983270217c47d38a44d4e0491fd710395e008786b5ae7033',
    'src/lib/support/report.ts':
      'd02ebbfa82cf415c1af192f691b5464fbbc51f898207263bf1422f049a5d9dec',
    'src/routes/+layout.svelte':
      'b013d2d2bba40ee894c122fbb352dc78110e35ad17ec7ebb830a4556d3204ff7',
    'src/routes/about/+page.svelte':
      'b89a886622bfaee20dc0bf3b8cb3f84bbdc94df249d746d690da4cf81619fcba',
    'src/routes/messages/+page.svelte':
      '8279a92770f38858e197e790839bc98b4d7889214ebecdf47e542d632afbc84f',
    'src/routes/messages/[conversationId=local_id]/+page.svelte':
      'b06c6189adee80b68bc6657fa0e8504551fe9a8b1655a6d81807ebee9930a5e7',
    'src/routes/privacy/+page.svelte':
      'c6fdaeea02196067c111e839ec2d8ea3ba4bf21b81dbe23a2cad5ae9503aad44',
    'src/routes/products/[naddr=naddr]/+page.svelte':
      '5f325300384e2156724f3f7ac3fa8512ac6b6090d11aef9e8e53f6aea9c51665',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const identity =
      name === 'src/routes/+layout.svelte'
        ? publicHideRootIdentity(bytes)
        : createHash('sha256').update(bytes).digest('hex');
    if (
      identity !== pin &&
      !(
        name === 'src/lib/components/AccountGate.svelte' &&
        identity ===
          '95c04e52fdc7c706c84af9d8d842a1b19db2a0ba2e0f5f04e5b68c7d95be1ab0'
      ) &&
      !(
        name === 'src/routes/products/[naddr=naddr]/+page.svelte' &&
        identity ===
          '10d98f1a306cfe216b57d6b990254335a7b21a8c42e558a54b2d471c49858069'
      ) &&
      !(
        name === 'src/routes/about/+page.svelte' &&
        identity ===
          '0f091601ef33ea7fbb1b233a26608427e71cc670cfe654fd04ff37d218b2e47f'
      ) &&
      !(
        name === 'src/routes/privacy/+page.svelte' &&
        identity ===
          'aed875eb72755c1a46c30c63f0a4c7266b30029279227bf6c2604305d2cfa318'
      )
    )
      throw new Error(
        name === 'src/routes/+layout.svelte'
          ? 'Invalid owned root runtime activation'
          : 'Invalid owned operational flags source'
      );
  }
  return true;
}
/** @param {string} web */
async function privacyDataAdmission(web) {
  if (await operationalFlagsAdmission(web)) return true;
  let privacy;
  try {
    privacy = publicText(
      await readOwned(web, 'src/routes/privacy/+page.svelte')
    );
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT')
      return false;
    throw error;
  }
  if (!privacy.includes('../../lib/components/LocalDataReview.svelte'))
    return false;
  /** @type {Record<string,string>} */
  const producers = {
    'src/lib/catalog/observations.ts':
      '7386470e698156f2480de1d6c9fcffacab507f97d2f4006df926db467327120c',
    'src/lib/catalog/product-view.ts':
      '3558230c9f7cd17666e27c032de4ba6f30d2fb6b4d54149a69e5b8f5dc5d5363',
    'src/lib/catalog/search-run.ts':
      '213a58cf897456a0dc0e99e46594cba013952ff08212802fb471f0d323c38573',
    'src/lib/catalog/search-view.ts':
      'bc4d0160574084a547d54d5ca582b15382f44d4649c255743ebdb4c8b398f795',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/components/ConversationOptions.svelte':
      '032a5f49d19a43a8ee88495b92d5337f1bb2de5dc58bb600b579e1423c9dfab1',
    'src/lib/components/ConversationRow.svelte':
      'c5d0926ac9cdda18a6befe365227dfe62405f71598f59964ff36af9a0e85d743',
    'src/lib/components/ConversationThread.svelte':
      '07d7421ca00f5f0c6e68b38b790f863ca5c0d01de6567ac3875ef719b83462f4',
    'src/lib/components/InboxStatus.svelte':
      'a1015aac1d6c9ebb7a54bd5746e4cbcb4b93e376a30a2cbd7f7df212713fc930',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/components/LocalDataReview.svelte':
      '63b259ed2c7daa552a6c54665da74f45275ec45c60b415207a008a4ac1aa5b37',
    'src/lib/components/MessageComposer.svelte':
      'bc8ac0e2123b60957f1c289acbb2b8740e171097029f640f657cd79f5aa8423b',
    'src/lib/components/MessageItem.svelte':
      '5c9af10ece18deeddd85e11453239c2a0b8e741cb84529e15c4ad87864704517',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/components/PublisherHide.svelte':
      '0fa30a68b44e9de68c3bb1edc9ed2c7fedefc2dc8a916c0a1df1f15440248927',
    'src/lib/components/ReportPreview.svelte':
      'fe133c95d640d3fa306667c613940a81783ce821471f70394671bb474c96d767',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/components/SupportLink.svelte':
      'c8608221416a47055c6b0aabe7a9d8370d3bc26e3fcd3c65214275d99ea1927e',
    'src/lib/config/budgets.ts':
      'e61806f1c2b580340e862b8dc7dd8063e90f74e03c64c721a7d112b0872f935d',
    'src/lib/config/deployment-relays.ts':
      '14443151ff819a11a4f9c71e4ff756d1fbdd7cf6c36242ac712c378e68583aa2',
    'src/lib/config/deployment-support.ts':
      'c698313143943749bb457674825b648f6af29477f07cae960f3435c035cd6bd8',
    'src/lib/config/inbox-operators.ts':
      '52a4a739bf7434e3dea529858456ab49bbbbd517de789dac7f6407b687e5c07a',
    'src/lib/config/relays.ts':
      'ca8c63df89509728a7b87e65232f1da751621ed4777993a079730472a0923957',
    'src/lib/config/support.ts':
      'a5b4c179db6f13ebf85081960e2f7c330c3e894666fbf8f0472edb1c5c818872',
    'src/lib/contracts/local-records.ts':
      '04ddd51339a439665a94d4a4de7b23c2257d4a673873a672d4427e9764d21f3f',
    'src/lib/contracts/public-key.ts':
      'd36d4001c2f4835e24d50f00460a35c9cf9fa6b86cc3b4a6937d8b4e10afdeb9',
    'src/lib/messaging/admit-conversation.ts':
      '1ad085aa530beeabbc9488676e77cf7cd0901e61e7705471f79a47bb1df70cf1',
    'src/lib/nostr/incoming-wire.ts':
      'd471dde674b31a62961bb6fcb503cd9e54791d83c7afdaabcb74686cc78a3723',
    'src/lib/messaging/cited-product.ts':
      'bed076c0902f6453971f88f107766c331b5737e488cb8c3c5f6c941e3428778d',
    'src/lib/messaging/conversation-thread.ts':
      '7367f0e6d96a9f249514f66c2459274e856eb83462523e37264d4cd97d2e36d1',
    'src/lib/messaging/decryption-queue.ts':
      'be704e6cbe777722c3eb80ec9843f00afd7bac8c52af0c2af7294f2437414553',
    'src/lib/messaging/inbox-list.ts':
      '9cc82877bfa8dbb2a7fa41bceb842793349416271f19569f49fdd5cfc1de07e5',
    'src/lib/nostr/self-recovery-reader.ts':
      '806230d8082a48b9c2117a16060aff50312be270348d7b8c5b93d6d6368d31a1',
    'src/lib/nostr/private-publisher.ts':
      '2f939dda3504b760dbf3758632cda6ce38d3e16c6d142b814e40a0e295a77d90',
    'src/lib/nostr/private-pool.ts':
      'c5d7884b33ea8dcde6aea495c92e7a53db12cf650f4476f2b66adc4165e771c2',
    'src/lib/nostr/inbox-ingress.ts':
      'f860a094a8ba767585124c795d44b7620064f73a5b3a0170041c581c0fc64ecb',
    'src/lib/persistence/inbox-probes.ts':
      'b4d3089bcf08a5c9ac3a99dfcf8e2c2e2160a2c8996c19fccb2670614f7b4d7a',
    'src/lib/nostr/inbox-access.ts':
      'd39f218b66fb51833e962b20152a2df81adf3097ba434a4cf2172ee4a7c8e3bc',
    'src/lib/nostr/seal-builder.ts':
      '3ccc967cc348e63857243ff6f0302c1a68da2afe85ca2c37e00d5a894785fc53',
    'src/lib/nostr/giftwrap-builder.ts':
      'faf54dd80a0995ac94b4b994d3cfd015516e5e201a1fbb6340e13c836ce49e98',
    'src/lib/messaging/inbox-access-controller.ts':
      '103c22b4cd721f7e3371ff887f4e7ca5df6a535688b9e9d0c67241db21430f9a',
    'src/lib/messaging/inbox-setup-view.ts':
      '4ec9025232bb1355fef1ba039b46ce0a2be10e02e348e1b4eaadc3d1f3c2fd84',
    'src/lib/messaging/inbox-sync.ts':
      '0d91f5895e7eb054b036a5cb80897a3d60a212d470029a82d11124c8536b3d2c',
    'src/lib/messaging/inbox-view.ts':
      '6871707ae7395d3679856a01e87c7d8f273848e0b9ef0dc7b94f450d8417528c',
    'src/lib/messaging/reply-controller.ts':
      '8d2612b4788211b35ee8d56caa389bb88a53268bbba71407ee787c9dc23ee8a6',
    'src/lib/messaging/rumor-plan.ts':
      'b622889e96f911501524cd86f9fca9f2207ce853767494fb30799b00ae8275ed',
    'src/lib/persistence/private-send-reservations.ts':
      '5a57522fdb5acabecf836fb8d642a2adfb4bc7e041843b38fd1b96b673b3942d',
    'src/lib/messaging/send-identity.ts':
      'cc840afbf155e18030bea9b475884ca3e0068b79ef3da1459b661fe2822499df',
    'src/lib/messaging/unlocked-session.ts':
      'b7ecc1acea5ed210b39cfe9ec82b3d02c75f6092c5c31dd3e0f79946b319ef4e',
    'src/lib/navigation-url.ts':
      '4ce96334d7cf57ccba518132078278b71d3bf70f519202e491a5f6a8840f7b19',
    'src/lib/nostr/enquiry-references.ts':
      'a17175337ee53279f898a7c6fa1af7e77ad26c0d2c0da46f26a9766fa701d365',
    'src/lib/nostr/envelope-bounds.ts':
      'b1e1d4a0d0673f358321c978726747ddaba3ebf94dd65e67adac408bb6574dae',
    'src/lib/nostr/exports.ts':
      '683fc933fadc515bd9999ddd337d85b408e0ad4fe891832f2825c35ffbe26f89',
    'src/lib/nostr/ingress.ts':
      'c433d459ad650c1fa9aa81aca24ddfaad8c251de9e43f92502e83fd5b673e772',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/nostr/public-store.ts':
      '330f7599f601956ed44795df1e016945e3564bbb7da6491c48f628d407de3d14',
    'src/lib/nostr/request-result.ts':
      '9bed0e13f94d38c3db471c0e750ef18d685776cb8832fedf7ddb7f92f663c2ac',
    'src/lib/nostr/request-scope.ts':
      'c6aa1005264e8ded37870c95f937d891afe67b8e07ef62dd137097c4ebb2388b',
    'src/lib/nostr/verified-envelope.ts':
      '01eb6224ece812fcafed5203c2cd117058a3d45cb8a2175b370e8cd74cdc0f77',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/persistence/database.ts':
      'd37d53378f531de376fb6d1b307b9b99e54736299a04eff56a66b4036261e170',
    'src/lib/persistence/inbox-retention.ts':
      '8bcc73be03321e60a6fbbe7ba58de14eb8fb34db5d5cdce0adee31b5467d7bb9',
    'src/lib/persistence/local-data-review.ts':
      '90379b183d0a1d8885076fbfc593d7c9ac1e351b16ce7800890107014e5cb0e4',
    'src/lib/persistence/inbox-envelope-repository.ts':
      'd3613a098c93c8881d53de12f91ffb689827926217cabcba66894885d49cb095',
    'src/lib/persistence/history-metadata-repository.ts':
      'afe4656e59bd6b365444fbda64fe6572ffa4c5323c474ee862e3cf19252b6f6d',
    'src/lib/messaging/history-pagination.ts':
      '79b22b312a126b55c80a3c6ca22724bd2c5f3ad766f8600f3647028a2624c255',
    'src/lib/persistence/private-outbox-cleanup.ts':
      '13f4e7d771b3a8d4ec7703f2026a1c8ff71582a2d2a810b9d402d58684b2e2b6',
    'src/lib/persistence/local-inventory.ts':
      'cd44ed47548453f0443400894c57c364b544781c028c4dba6e9fa096034c5965',
    'src/lib/persistence/local-private-blocks.ts':
      'd7e76c1122ff5479f261293db458a3240438ce0471c788c2f07ffaf854449cbc',
    'src/lib/persistence/local-public-hides.ts':
      '4f0e64ed0fabbe9f86014a0512e7dbd3c81711f28ca2c626e2dafa5cda349f83',
    'src/lib/persistence/private-records.ts':
      '821ef96740cf644bff4ffe5c7816c6aa097aa419bd16b8b301809b963294dafe',
    'src/lib/persistence/private-sends.ts':
      '6696f4179370cad373c06debf57b919598d53b3d25b6d2a6054a557608e4eb9c',
    'src/lib/persistence/private-storage.ts':
      'ba4a61dd1fa08fc89d00adac3c258c1f6ae7e5c1b15165facffe76ddd71d5e91',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/lib/persistence/quota.ts':
      'd82858b3de51ddb55466eedb980f37ebe3f70db6c36d0abe5f39feed30d441fc',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/selling-view.ts':
      '7d1f07c5b128512d3ed19a73409f1c64ada9988d77ac9bf60799352261797f5b',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/runtime/identity-session.ts':
      '7d665f6872bdc780f1f2b56245730adcc90f32cf1fed6997f7e2e44ea2447b49',
    'src/lib/runtime/private-session.ts':
      'ecffe8a4a735478669f46e666e96add6d4b32c615e3741fcc2277a07ca2ef6c3',
    'src/lib/runtime/public-runtime.ts':
      '679aa4cd51a1dfdc81459bdfccb29441ac60edfa8b1ddd579425787c85b5cff4',
    'src/lib/runtime/view-context.ts':
      'e2329113e29db35e983270217c47d38a44d4e0491fd710395e008786b5ae7033',
    'src/lib/support/report.ts':
      'd02ebbfa82cf415c1af192f691b5464fbbc51f898207263bf1422f049a5d9dec',
    'src/routes/+layout.svelte':
      'b013d2d2bba40ee894c122fbb352dc78110e35ad17ec7ebb830a4556d3204ff7',
    'src/routes/about/+page.svelte':
      'b89a886622bfaee20dc0bf3b8cb3f84bbdc94df249d746d690da4cf81619fcba',
    'src/routes/messages/+page.svelte':
      '8279a92770f38858e197e790839bc98b4d7889214ebecdf47e542d632afbc84f',
    'src/routes/messages/[conversationId=local_id]/+page.svelte':
      'b06c6189adee80b68bc6657fa0e8504551fe9a8b1655a6d81807ebee9930a5e7',
    'src/routes/privacy/+page.svelte':
      'c6fdaeea02196067c111e839ec2d8ea3ba4bf21b81dbe23a2cad5ae9503aad44',
    'src/routes/products/[naddr=naddr]/+page.svelte':
      '5f325300384e2156724f3f7ac3fa8512ac6b6090d11aef9e8e53f6aea9c51665',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const identity =
      name === 'src/routes/+layout.svelte'
        ? publicHideRootIdentity(bytes)
        : createHash('sha256').update(bytes).digest('hex');
    if (identity !== pin)
      throw new Error(
        name === 'src/routes/+layout.svelte'
          ? 'Invalid owned root runtime activation'
          : 'Invalid owned privacy data source'
      );
  }
  return true;
}
/** @param {string} web */
async function supportReportingAdmission(web) {
  if (await privacyDataAdmission(web)) return true;
  let about;
  try {
    about = publicText(await readOwned(web, 'src/routes/about/+page.svelte'));
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT')
      return false;
    throw error;
  }
  if (!about.includes('../../lib/components/SupportLink.svelte')) return false;
  /** @type {Record<string,string>} */
  const producers = {
    'src/lib/catalog/observations.ts':
      '7386470e698156f2480de1d6c9fcffacab507f97d2f4006df926db467327120c',
    'src/lib/catalog/product-view.ts':
      '3558230c9f7cd17666e27c032de4ba6f30d2fb6b4d54149a69e5b8f5dc5d5363',
    'src/lib/catalog/search-run.ts':
      '213a58cf897456a0dc0e99e46594cba013952ff08212802fb471f0d323c38573',
    'src/lib/catalog/search-view.ts':
      'bc4d0160574084a547d54d5ca582b15382f44d4649c255743ebdb4c8b398f795',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/components/ConversationOptions.svelte':
      '032a5f49d19a43a8ee88495b92d5337f1bb2de5dc58bb600b579e1423c9dfab1',
    'src/lib/components/ConversationRow.svelte':
      'c5d0926ac9cdda18a6befe365227dfe62405f71598f59964ff36af9a0e85d743',
    'src/lib/components/ConversationThread.svelte':
      '07d7421ca00f5f0c6e68b38b790f863ca5c0d01de6567ac3875ef719b83462f4',
    'src/lib/components/InboxStatus.svelte':
      'a1015aac1d6c9ebb7a54bd5746e4cbcb4b93e376a30a2cbd7f7df212713fc930',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/components/MessageComposer.svelte':
      'bc8ac0e2123b60957f1c289acbb2b8740e171097029f640f657cd79f5aa8423b',
    'src/lib/components/MessageItem.svelte':
      '5c9af10ece18deeddd85e11453239c2a0b8e741cb84529e15c4ad87864704517',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/components/PublisherHide.svelte':
      '0fa30a68b44e9de68c3bb1edc9ed2c7fedefc2dc8a916c0a1df1f15440248927',
    'src/lib/components/ReportPreview.svelte':
      'fe133c95d640d3fa306667c613940a81783ce821471f70394671bb474c96d767',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/components/SupportLink.svelte':
      'c8608221416a47055c6b0aabe7a9d8370d3bc26e3fcd3c65214275d99ea1927e',
    'src/lib/config/budgets.ts':
      'e61806f1c2b580340e862b8dc7dd8063e90f74e03c64c721a7d112b0872f935d',
    'src/lib/config/deployment-relays.ts':
      '14443151ff819a11a4f9c71e4ff756d1fbdd7cf6c36242ac712c378e68583aa2',
    'src/lib/config/deployment-support.ts':
      'c698313143943749bb457674825b648f6af29477f07cae960f3435c035cd6bd8',
    'src/lib/config/relays.ts':
      'ca8c63df89509728a7b87e65232f1da751621ed4777993a079730472a0923957',
    'src/lib/config/support.ts':
      'a5b4c179db6f13ebf85081960e2f7c330c3e894666fbf8f0472edb1c5c818872',
    'src/lib/contracts/local-records.ts':
      '04ddd51339a439665a94d4a4de7b23c2257d4a673873a672d4427e9764d21f3f',
    'src/lib/contracts/public-key.ts':
      'd36d4001c2f4835e24d50f00460a35c9cf9fa6b86cc3b4a6937d8b4e10afdeb9',
    'src/lib/messaging/admit-conversation.ts':
      '1ad085aa530beeabbc9488676e77cf7cd0901e61e7705471f79a47bb1df70cf1',
    'src/lib/nostr/incoming-wire.ts':
      'd471dde674b31a62961bb6fcb503cd9e54791d83c7afdaabcb74686cc78a3723',
    'src/lib/messaging/cited-product.ts':
      'bed076c0902f6453971f88f107766c331b5737e488cb8c3c5f6c941e3428778d',
    'src/lib/messaging/conversation-thread.ts':
      '7367f0e6d96a9f249514f66c2459274e856eb83462523e37264d4cd97d2e36d1',
    'src/lib/messaging/decryption-queue.ts':
      'be704e6cbe777722c3eb80ec9843f00afd7bac8c52af0c2af7294f2437414553',
    'src/lib/messaging/inbox-list.ts':
      '9cc82877bfa8dbb2a7fa41bceb842793349416271f19569f49fdd5cfc1de07e5',
    'src/lib/messaging/inbox-setup-view.ts':
      '4ec9025232bb1355fef1ba039b46ce0a2be10e02e348e1b4eaadc3d1f3c2fd84',
    'src/lib/messaging/inbox-sync.ts':
      '0d91f5895e7eb054b036a5cb80897a3d60a212d470029a82d11124c8536b3d2c',
    'src/lib/messaging/inbox-view.ts':
      '6871707ae7395d3679856a01e87c7d8f273848e0b9ef0dc7b94f450d8417528c',
    'src/lib/messaging/reply-controller.ts':
      '8d2612b4788211b35ee8d56caa389bb88a53268bbba71407ee787c9dc23ee8a6',
    'src/lib/messaging/rumor-plan.ts':
      'b622889e96f911501524cd86f9fca9f2207ce853767494fb30799b00ae8275ed',
    'src/lib/persistence/private-send-reservations.ts':
      '5a57522fdb5acabecf836fb8d642a2adfb4bc7e041843b38fd1b96b673b3942d',
    'src/lib/messaging/send-identity.ts':
      'cc840afbf155e18030bea9b475884ca3e0068b79ef3da1459b661fe2822499df',
    'src/lib/messaging/unlocked-session.ts':
      'b7ecc1acea5ed210b39cfe9ec82b3d02c75f6092c5c31dd3e0f79946b319ef4e',
    'src/lib/navigation-url.ts':
      '4ce96334d7cf57ccba518132078278b71d3bf70f519202e491a5f6a8840f7b19',
    'src/lib/nostr/enquiry-references.ts':
      'a17175337ee53279f898a7c6fa1af7e77ad26c0d2c0da46f26a9766fa701d365',
    'src/lib/nostr/envelope-bounds.ts':
      'b1e1d4a0d0673f358321c978726747ddaba3ebf94dd65e67adac408bb6574dae',
    'src/lib/nostr/exports.ts':
      '683fc933fadc515bd9999ddd337d85b408e0ad4fe891832f2825c35ffbe26f89',
    'src/lib/nostr/ingress.ts':
      'c433d459ad650c1fa9aa81aca24ddfaad8c251de9e43f92502e83fd5b673e772',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/nostr/public-store.ts':
      '330f7599f601956ed44795df1e016945e3564bbb7da6491c48f628d407de3d14',
    'src/lib/nostr/request-result.ts':
      '9bed0e13f94d38c3db471c0e750ef18d685776cb8832fedf7ddb7f92f663c2ac',
    'src/lib/nostr/request-scope.ts':
      'c6aa1005264e8ded37870c95f937d891afe67b8e07ef62dd137097c4ebb2388b',
    'src/lib/nostr/verified-envelope.ts':
      '01eb6224ece812fcafed5203c2cd117058a3d45cb8a2175b370e8cd74cdc0f77',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/persistence/database.ts':
      'd37d53378f531de376fb6d1b307b9b99e54736299a04eff56a66b4036261e170',
    'src/lib/persistence/local-private-blocks.ts':
      'd7e76c1122ff5479f261293db458a3240438ce0471c788c2f07ffaf854449cbc',
    'src/lib/persistence/local-public-hides.ts':
      '4f0e64ed0fabbe9f86014a0512e7dbd3c81711f28ca2c626e2dafa5cda349f83',
    'src/lib/persistence/private-storage.ts':
      '66bab9a0a0a699b70ecba75c002ac0de20a84f3d015f5caefbe1697d0f6456bf',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/lib/persistence/quota.ts':
      'e261a7c9c81d0685021663639b1bb543192e65580edb64fa58ccbb9a4a04cc95',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/selling-view.ts':
      '7d1f07c5b128512d3ed19a73409f1c64ada9988d77ac9bf60799352261797f5b',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/runtime/identity-session.ts':
      '7d665f6872bdc780f1f2b56245730adcc90f32cf1fed6997f7e2e44ea2447b49',
    'src/lib/runtime/private-session.ts':
      'ecffe8a4a735478669f46e666e96add6d4b32c615e3741fcc2277a07ca2ef6c3',
    'src/lib/runtime/public-runtime.ts':
      '679aa4cd51a1dfdc81459bdfccb29441ac60edfa8b1ddd579425787c85b5cff4',
    'src/lib/runtime/view-context.ts':
      'e2329113e29db35e983270217c47d38a44d4e0491fd710395e008786b5ae7033',
    'src/lib/support/report.ts':
      'd02ebbfa82cf415c1af192f691b5464fbbc51f898207263bf1422f049a5d9dec',
    'src/routes/+layout.svelte':
      'b013d2d2bba40ee894c122fbb352dc78110e35ad17ec7ebb830a4556d3204ff7',
    'src/routes/about/+page.svelte':
      'b89a886622bfaee20dc0bf3b8cb3f84bbdc94df249d746d690da4cf81619fcba',
    'src/routes/messages/+page.svelte':
      '8279a92770f38858e197e790839bc98b4d7889214ebecdf47e542d632afbc84f',
    'src/routes/messages/[conversationId=local_id]/+page.svelte':
      'b06c6189adee80b68bc6657fa0e8504551fe9a8b1655a6d81807ebee9930a5e7',
    'src/routes/privacy/+page.svelte':
      'b16f21dddcff172b837ab9980a5d8e93b59d5ddb5fa15ce463e07affdee79547',
    'src/routes/products/[naddr=naddr]/+page.svelte':
      '5f325300384e2156724f3f7ac3fa8512ac6b6090d11aef9e8e53f6aea9c51665',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const identity =
      name === 'src/routes/+layout.svelte'
        ? publicHideRootIdentity(bytes)
        : createHash('sha256').update(bytes).digest('hex');
    if (identity !== pin)
      throw new Error(
        name === 'src/routes/+layout.svelte'
          ? 'Invalid owned root runtime activation'
          : 'Invalid owned support reporting source'
      );
  }
  return true;
}
/** @param {string} web */
async function privateBlockingAdmission(web) {
  if (await supportReportingAdmission(web)) return true;
  const root = publicText(await readOwned(web, 'src/routes/+layout.svelte'));
  if (!root.includes('../lib/persistence/local-private-blocks.ts'))
    return false;
  /** @type {Record<string,string>} */
  const producers = {
    'src/lib/catalog/observations.ts':
      '7386470e698156f2480de1d6c9fcffacab507f97d2f4006df926db467327120c',
    'src/lib/catalog/product-view.ts':
      '3558230c9f7cd17666e27c032de4ba6f30d2fb6b4d54149a69e5b8f5dc5d5363',
    'src/lib/catalog/search-run.ts':
      '213a58cf897456a0dc0e99e46594cba013952ff08212802fb471f0d323c38573',
    'src/lib/catalog/search-view.ts':
      'bc4d0160574084a547d54d5ca582b15382f44d4649c255743ebdb4c8b398f795',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/components/ConversationOptions.svelte':
      'b40915c0b11456fbc852d67591fd4c629a33d5b271e79b79dd042fcf2c0f74cb',
    'src/lib/components/ConversationRow.svelte':
      'c5d0926ac9cdda18a6befe365227dfe62405f71598f59964ff36af9a0e85d743',
    'src/lib/components/ConversationThread.svelte':
      '07d7421ca00f5f0c6e68b38b790f863ca5c0d01de6567ac3875ef719b83462f4',
    'src/lib/components/InboxStatus.svelte':
      'a1015aac1d6c9ebb7a54bd5746e4cbcb4b93e376a30a2cbd7f7df212713fc930',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/components/MessageComposer.svelte':
      'bc8ac0e2123b60957f1c289acbb2b8740e171097029f640f657cd79f5aa8423b',
    'src/lib/components/MessageItem.svelte':
      '5c9af10ece18deeddd85e11453239c2a0b8e741cb84529e15c4ad87864704517',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/components/PublisherHide.svelte':
      '0fa30a68b44e9de68c3bb1edc9ed2c7fedefc2dc8a916c0a1df1f15440248927',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/config/budgets.ts':
      'e61806f1c2b580340e862b8dc7dd8063e90f74e03c64c721a7d112b0872f935d',
    'src/lib/config/deployment-relays.ts':
      '14443151ff819a11a4f9c71e4ff756d1fbdd7cf6c36242ac712c378e68583aa2',
    'src/lib/config/relays.ts':
      'ca8c63df89509728a7b87e65232f1da751621ed4777993a079730472a0923957',
    'src/lib/contracts/local-records.ts':
      '04ddd51339a439665a94d4a4de7b23c2257d4a673873a672d4427e9764d21f3f',
    'src/lib/contracts/public-key.ts':
      'd36d4001c2f4835e24d50f00460a35c9cf9fa6b86cc3b4a6937d8b4e10afdeb9',
    'src/lib/messaging/admit-conversation.ts':
      '1ad085aa530beeabbc9488676e77cf7cd0901e61e7705471f79a47bb1df70cf1',
    'src/lib/nostr/incoming-wire.ts':
      'd471dde674b31a62961bb6fcb503cd9e54791d83c7afdaabcb74686cc78a3723',
    'src/lib/messaging/cited-product.ts':
      'bed076c0902f6453971f88f107766c331b5737e488cb8c3c5f6c941e3428778d',
    'src/lib/messaging/conversation-thread.ts':
      '7367f0e6d96a9f249514f66c2459274e856eb83462523e37264d4cd97d2e36d1',
    'src/lib/messaging/decryption-queue.ts':
      'be704e6cbe777722c3eb80ec9843f00afd7bac8c52af0c2af7294f2437414553',
    'src/lib/messaging/inbox-list.ts':
      '9cc82877bfa8dbb2a7fa41bceb842793349416271f19569f49fdd5cfc1de07e5',
    'src/lib/messaging/inbox-setup-view.ts':
      '4ec9025232bb1355fef1ba039b46ce0a2be10e02e348e1b4eaadc3d1f3c2fd84',
    'src/lib/messaging/inbox-sync.ts':
      '0d91f5895e7eb054b036a5cb80897a3d60a212d470029a82d11124c8536b3d2c',
    'src/lib/messaging/inbox-view.ts':
      '6871707ae7395d3679856a01e87c7d8f273848e0b9ef0dc7b94f450d8417528c',
    'src/lib/messaging/reply-controller.ts':
      '8d2612b4788211b35ee8d56caa389bb88a53268bbba71407ee787c9dc23ee8a6',
    'src/lib/messaging/rumor-plan.ts':
      'b622889e96f911501524cd86f9fca9f2207ce853767494fb30799b00ae8275ed',
    'src/lib/persistence/private-send-reservations.ts':
      '5a57522fdb5acabecf836fb8d642a2adfb4bc7e041843b38fd1b96b673b3942d',
    'src/lib/messaging/send-identity.ts':
      'cc840afbf155e18030bea9b475884ca3e0068b79ef3da1459b661fe2822499df',
    'src/lib/messaging/unlocked-session.ts':
      'b7ecc1acea5ed210b39cfe9ec82b3d02c75f6092c5c31dd3e0f79946b319ef4e',
    'src/lib/nostr/enquiry-references.ts':
      'a17175337ee53279f898a7c6fa1af7e77ad26c0d2c0da46f26a9766fa701d365',
    'src/lib/nostr/envelope-bounds.ts':
      'b1e1d4a0d0673f358321c978726747ddaba3ebf94dd65e67adac408bb6574dae',
    'src/lib/nostr/exports.ts':
      '683fc933fadc515bd9999ddd337d85b408e0ad4fe891832f2825c35ffbe26f89',
    'src/lib/nostr/ingress.ts':
      'c433d459ad650c1fa9aa81aca24ddfaad8c251de9e43f92502e83fd5b673e772',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/nostr/public-store.ts':
      '330f7599f601956ed44795df1e016945e3564bbb7da6491c48f628d407de3d14',
    'src/lib/nostr/request-result.ts':
      '9bed0e13f94d38c3db471c0e750ef18d685776cb8832fedf7ddb7f92f663c2ac',
    'src/lib/nostr/request-scope.ts':
      'c6aa1005264e8ded37870c95f937d891afe67b8e07ef62dd137097c4ebb2388b',
    'src/lib/nostr/verified-envelope.ts':
      '01eb6224ece812fcafed5203c2cd117058a3d45cb8a2175b370e8cd74cdc0f77',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/persistence/database.ts':
      'd37d53378f531de376fb6d1b307b9b99e54736299a04eff56a66b4036261e170',
    'src/lib/persistence/local-private-blocks.ts':
      'd7e76c1122ff5479f261293db458a3240438ce0471c788c2f07ffaf854449cbc',
    'src/lib/persistence/local-public-hides.ts':
      '4f0e64ed0fabbe9f86014a0512e7dbd3c81711f28ca2c626e2dafa5cda349f83',
    'src/lib/persistence/private-storage.ts':
      '66bab9a0a0a699b70ecba75c002ac0de20a84f3d015f5caefbe1697d0f6456bf',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/lib/persistence/quota.ts':
      'e261a7c9c81d0685021663639b1bb543192e65580edb64fa58ccbb9a4a04cc95',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/selling-view.ts':
      '7d1f07c5b128512d3ed19a73409f1c64ada9988d77ac9bf60799352261797f5b',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/runtime/identity-session.ts':
      '7d665f6872bdc780f1f2b56245730adcc90f32cf1fed6997f7e2e44ea2447b49',
    'src/lib/runtime/private-session.ts':
      'ecffe8a4a735478669f46e666e96add6d4b32c615e3741fcc2277a07ca2ef6c3',
    'src/lib/runtime/public-runtime.ts':
      '679aa4cd51a1dfdc81459bdfccb29441ac60edfa8b1ddd579425787c85b5cff4',
    'src/lib/runtime/view-context.ts':
      'e2329113e29db35e983270217c47d38a44d4e0491fd710395e008786b5ae7033',
    'src/routes/+layout.svelte':
      'b013d2d2bba40ee894c122fbb352dc78110e35ad17ec7ebb830a4556d3204ff7',
    'src/routes/messages/+page.svelte':
      '8279a92770f38858e197e790839bc98b4d7889214ebecdf47e542d632afbc84f',
    'src/routes/messages/[conversationId=local_id]/+page.svelte':
      'b06c6189adee80b68bc6657fa0e8504551fe9a8b1655a6d81807ebee9930a5e7',
    'src/routes/privacy/+page.svelte':
      'b16f21dddcff172b837ab9980a5d8e93b59d5ddb5fa15ce463e07affdee79547',
    'src/routes/products/[naddr=naddr]/+page.svelte':
      'd3afe029ad6ed6384a300ad6719f32c324a1841271eddf9baee5b8f31f54e680',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const identity =
      name === 'src/routes/+layout.svelte'
        ? publicHideRootIdentity(bytes)
        : createHash('sha256').update(bytes).digest('hex');
    if (identity !== pin)
      throw new Error(
        name === 'src/routes/+layout.svelte'
          ? 'Invalid owned root runtime activation'
          : 'Invalid owned private blocking source'
      );
  }
  return true;
}
/** @param {string} web */
async function publicHidingAdmission(web) {
  if (await privateBlockingAdmission(web)) return true;
  const root = publicText(await readOwned(web, 'src/routes/+layout.svelte'));
  if (!root.includes('../lib/persistence/local-public-hides.ts')) return false;
  /** @type {Record<string,string>} */
  const producers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/runtime/public-runtime.ts':
      '679aa4cd51a1dfdc81459bdfccb29441ac60edfa8b1ddd579425787c85b5cff4',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/quota.ts':
      'e261a7c9c81d0685021663639b1bb543192e65580edb64fa58ccbb9a4a04cc95',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/publishing/selling-view.ts':
      '7d1f07c5b128512d3ed19a73409f1c64ada9988d77ac9bf60799352261797f5b',
    'src/lib/runtime/identity-session.ts':
      '7d665f6872bdc780f1f2b56245730adcc90f32cf1fed6997f7e2e44ea2447b49',
    'src/lib/persistence/local-public-hides.ts':
      '4f0e64ed0fabbe9f86014a0512e7dbd3c81711f28ca2c626e2dafa5cda349f83',
    'src/lib/catalog/product-view.ts':
      '3558230c9f7cd17666e27c032de4ba6f30d2fb6b4d54149a69e5b8f5dc5d5363',
    'src/lib/catalog/search-run.ts':
      '213a58cf897456a0dc0e99e46594cba013952ff08212802fb471f0d323c38573',
    'src/lib/catalog/search-view.ts':
      'bc4d0160574084a547d54d5ca582b15382f44d4649c255743ebdb4c8b398f795',
    'src/routes/+layout.svelte':
      '969a6cd6e25c3da845e897b68298800079b9e5caa8fff5967ed4d231b28b1523',
    'src/routes/products/[naddr=naddr]/+page.svelte':
      'd3afe029ad6ed6384a300ad6719f32c324a1841271eddf9baee5b8f31f54e680',
    'src/routes/privacy/+page.svelte':
      '48a01a3fba97ae6a21a5bf20383d42fd67fb8e5320bd35b266a4d9c771dccad7',
    'src/lib/components/PublisherHide.svelte':
      '0fa30a68b44e9de68c3bb1edc9ed2c7fedefc2dc8a916c0a1df1f15440248927'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const identity =
      name === 'src/routes/+layout.svelte'
        ? publicHideRootIdentity(bytes)
        : createHash('sha256').update(bytes).digest('hex');
    if (identity !== pin)
      throw new Error(
        name === 'src/routes/+layout.svelte'
          ? 'Invalid owned root runtime activation'
          : 'Invalid owned public hiding source'
      );
  }
  return true;
}
/** @param {string} web */
async function hasOwnedPublicRuntime(web) {
  if (await privateBlockingAdmission(web)) return 'public_identity';
  const hasHiding = await publicHidingAdmission(web);
  const layout = publicText(await readOwned(web, 'src/routes/+layout.svelte'));
  const component = parseSvelte(layout);
  if (!component.instance) return false;
  const script = layout.slice(
    component.instance.content.start,
    component.instance.content.end
  );
  const source = ts.createSourceFile(
    'root.ts',
    script,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  const runtimePath = '../lib/runtime/public-runtime.ts';
  const imports = source.statements.filter(ts.isImportDeclaration);
  const runtimeImports = imports.filter(
    (node) =>
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.moduleSpecifier.text === runtimePath
  );
  if (!runtimeImports.length) return false;
  /** @returns {never} */
  function fail() {
    throw new Error('Invalid owned root runtime activation');
  }
  if (runtimeImports.length !== 1 || component.module) fail();
  const required = new Map([
    [
      runtimePath,
      [
        'createPublicRuntimeContext',
        'mountPublicRuntime',
        'closePublicRuntime',
        'PUBLIC_RUNTIME_CONTEXT',
        ...(hasHiding ? ['bindPublicRuntimeHides'] : [])
      ]
    ],
    ['svelte', ['onMount', 'setContext']]
  ]);
  const identityPath = '../lib/runtime/view-context.ts';
  const hasIdentity = imports.some(
    (node) =>
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.moduleSpecifier.text === identityPath
  );
  if (hasIdentity)
    required.set(identityPath, [
      'createIdentityViewContext',
      'IDENTITY_VIEW_CONTEXT',
      'identityViewSnapshot',
      'mountIdentityView',
      'subscribeIdentityView',
      'connectIdentityView',
      'disconnectIdentityView',
      'closeIdentityView',
      'invalidateIdentityView'
    ]);
  if (hasHiding)
    required.set('../lib/persistence/local-public-hides.ts', [
      'createLocalPublicHides',
      'mountLocalPublicHides',
      'closeLocalPublicHides',
      'LOCAL_PUBLIC_HIDES_CONTEXT'
    ]);
  const allowedImports = new Set();
  for (const [module, names] of required) {
    const declarations = imports.filter(
      (node) =>
        ts.isStringLiteral(node.moduleSpecifier) &&
        node.moduleSpecifier.text === module
    );
    if (declarations.length !== 1) fail();
    const clause = declarations[0].importClause;
    if (
      !clause ||
      clause.isTypeOnly ||
      clause.name ||
      !clause.namedBindings ||
      !ts.isNamedImports(clause.namedBindings)
    )
      fail();
    const elements = clause.namedBindings.elements;
    for (const name of names) {
      const bindings = elements.filter((node) => node.name.text === name);
      if (
        bindings.length !== 1 ||
        bindings[0].propertyName ||
        bindings[0].isTypeOnly
      )
        fail();
    }
    if (
      (module === runtimePath || module === identityPath) &&
      elements.length !== names.length
    )
      fail();
    allowedImports.add(declarations[0]);
  }
  const printer = ts.createPrinter({ removeComments: true });
  /** @param {import("typescript").Node} node @param {import("typescript").SourceFile} file */
  const print = (node, file) =>
    printer.printNode(ts.EmitHint.Unspecified, node, file);
  const expected = ts.createSourceFile(
    'expected.ts',
    hasHiding
      ? `
    const publicContext = createPublicRuntimeContext();
    setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);
    onMount(() => {
      const runtime = mountPublicRuntime(publicContext);
      if(runtime) bindPublicRuntimeHides(runtime,publicHides);
      void mountLocalPublicHides(publicHides);
      return () => {
        closeLocalPublicHides(publicHides);
        closePublicRuntime(publicContext);
      };
    });`
      : `
    const publicContext = createPublicRuntimeContext();
    setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);
    onMount(() => {
      mountPublicRuntime(publicContext);
      return () => closePublicRuntime(publicContext);
    });`,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  const shapes = expected.statements.map((node) => print(node, expected));
  const lifecycle = source.statements.filter((node) =>
    shapes.includes(print(node, source))
  );
  if (
    lifecycle.length !== shapes.length ||
    lifecycle.some((node, i) => print(node, source) !== shapes[i])
  )
    fail();
  // HCP056 adds one separately owned identity lifecycle. The canonical public
  // activation above remains mandatory, unchanged and independently ordered.
  if (hasIdentity) {
    const identityExpected = ts.createSourceFile(
      'identity-expected.ts',
      `
  const identityContext = createIdentityViewContext();
  ${hasHiding ? 'const publicHides = createLocalPublicHides(identityContext); setContext(LOCAL_PUBLIC_HIDES_CONTEXT, publicHides);' : ''}
  setContext(IDENTITY_VIEW_CONTEXT, identityContext);
  let identityView = $state(identityViewSnapshot(identityContext));
  $effect(() => {
    if (page.url.pathname) invalidateIdentityView(identityContext);
  });
  function connect() {
    return connectIdentityView(identityContext);
  }
  function disconnect() {
    disconnectIdentityView(identityContext);
  }
  const shellIdentity = $derived(
    identityView.identity.state === 'guest' ||
      identityView.identity.state === 'pending'
      ? {
          kind: 'guest' as const,
          onconnect: identityView.mounted ? connect : undefined
        }
      : {
          kind: 'connected' as const,
          publicKey: identityView.identity.publicKey,
          ondisconnect: disconnect
        }
  );

  onMount(() => {
    const off = subscribeIdentityView(identityContext, (next) => {
      identityView = next;
    });
    mountIdentityView(identityContext);
    return () => {
      off();
      closeIdentityView(identityContext);
    };
  });
`,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS
    );
    const identityShapes = identityExpected.statements.map((node) =>
      print(node, identityExpected)
    );
    const identityLifecycle = source.statements.filter((node) =>
      identityShapes.includes(print(node, source))
    );
    if (
      identityLifecycle.length !== identityShapes.length ||
      identityLifecycle.some(
        (node, i) => print(node, source) !== identityShapes[i]
      )
    )
      fail();
    lifecycle.push(...identityLifecycle);
    publicText(await readOwned(web, 'src/lib/runtime/view-context.ts'));
    publicText(await readOwned(web, 'src/lib/runtime/identity-session.ts'));
  }
  // Reject duplicate, shadowed, reassigned, or extra uses of these bindings in
  // any other statement, including nested closures. Canonical callbacks have
  // no parameters and therefore cannot hide a second context binding.
  const bindings = new Set([
    'publicContext',
    ...[...required.values()].flat(),
    ...(hasIdentity
      ? [
          'identityContext',
          ...(hasHiding ? ['publicHides'] : []),
          'identityView',
          'connect',
          'disconnect',
          'shellIdentity'
        ]
      : [])
  ]);
  for (const statement of source.statements) {
    if (allowedImports.has(statement) || lifecycle.includes(statement))
      continue;
    /** @param {import("typescript").Node} node */
    const visit = (node) => {
      if (ts.isIdentifier(node) && bindings.has(node.text)) fail();
      ts.forEachChild(node, visit);
    };
    visit(statement);
  }
  for (const name of [
    'runtime/public-runtime.ts',
    'nostr/request-scope.ts',
    'nostr/public-pool.ts',
    'nostr/public-store.ts',
    'nostr/request-result.ts',
    'nostr/ingress.ts',
    'nostr/verified-envelope.ts',
    'nostr/envelope-bounds.ts',
    'nostr/exports.ts',
    'catalog/observations.ts',
    'config/budgets.ts',
    'config/relays.ts',
    'config/deployment-relays.ts'
  ])
    publicText(await readOwned(web, 'src/lib/' + name));
  return hasIdentity ? 'public_identity' : 'public';
}
/** @param {string} web */
async function productPresentationAdmission(web) {
  const route = publicText(
    await readOwned(web, 'src/routes/products/[naddr=naddr]/+page.svelte')
  );
  if (!route.includes('../../../lib/catalog/product-view.ts')) return false;
  const hasEnquiry = route.includes(
    '../../../lib/components/ProductEnquiry.svelte'
  );
  const hasRequest =
    hasEnquiry &&
    (await readOwned(web, 'src/lib/components/ProductEnquiry.svelte'))
      .toString()
      .includes('../messaging/product-request-controller.ts');
  const producers = hasRequest
    ? {
        'src/routes/products/[naddr=naddr]/+page.svelte':
          'e63a2c8b3833966a36e4d97aef7726ff3d087703cc9b475e7f52f5478fed5ce7',
        'src/lib/catalog/product-view.ts':
          'c2562d1470282c758889e9855f7e7e6c32a6b2eb090e4dace2e94fdfdb6d8363',
        'src/lib/components/ListingFacts.svelte':
          '65d009b4940e7ced61347919689877da9473a2a66a44f61cc5e7bd0b2d217215',
        'src/lib/components/PublisherIdentity.svelte':
          '90b156e2401d40079ee559d65ce66ec09ecc4246bc80cfdfc5def0e21e8ce9ff',
        'src/lib/contracts/food-availability-v1/contact-read.ts':
          '18f812812c15858768223d929bcea9d7eae5154612747016ccd17864c229420a',
        'src/lib/navigation-copy.ts':
          '1a17eac4b355d8a3fb2b55b3fe1bc856cc3b3bce07434836d708b68407686f20',
        'src/lib/runtime/public-runtime.ts':
          'fa2e27d72b82b6bef73693cf93f19614e61ea51ebd11eb40d8ca4ec723f0c14e',
        'src/lib/catalog/publishers.ts':
          'a6795a35ebbf97246e22235df1ca868432e2cccfb26a38f0ebdf4524911192e6',
        'src/lib/catalog/resolve-head.ts':
          '4f1c9f144164580da038d127ab463f9f8ba6b20e882fc31151cb29850782d633',
        'src/lib/components/ProductEnquiry.svelte':
          'b49f555a239ab31253f403cc98eaf82413b2584c71e336262202fe840b9227a2',
        'src/lib/messaging/product-enquiry.ts':
          '6652f4af6b45ae14d82bf4d6b56dea7e95af8b254d9b41da9528cf34e87d2052',
        'src/lib/messaging/enquiry-context.ts':
          'fc00bb77f601427fb73defc0c758332c3465c2d583bfa1f7b996f808f508b735',
        'src/lib/messaging/product-request-controller.ts':
          '9fb8bb41657de661d4fe32d05f78fdc453e215467b08ee7d8036d39093cb0979',
        'src/lib/components/CapabilityGate.svelte':
          '38584846f67135d03ad531199cfd4e5f9dbcede58ca7baa2ee2d14fd9f46d414',
        'src/lib/messaging/resolve-inbox.ts':
          '6c6438c518f13916a4a5664d8dd0e313338219bd52d5a91db8abab9bbc4a794c',
        'src/lib/messaging/enquiry-readiness.ts':
          '8c7a526c763d43c0ef04aec8ae00dd5d6858911d9c5a18a87127100c5b57782a'
      }
    : hasEnquiry
      ? {
          'src/routes/products/[naddr=naddr]/+page.svelte':
            '970f716e5f31bb0a7e463db84876d4f4b0801979e0893ef69fbeed1b17d69b0f',
          'src/lib/catalog/product-view.ts':
            'e608bcc5b7918366e2daabd5f5270cabf0ded22e1bbfd413d26450387ebac171',
          'src/lib/components/ListingFacts.svelte':
            '65d009b4940e7ced61347919689877da9473a2a66a44f61cc5e7bd0b2d217215',
          'src/lib/components/PublisherIdentity.svelte':
            '90b156e2401d40079ee559d65ce66ec09ecc4246bc80cfdfc5def0e21e8ce9ff',
          'src/lib/contracts/food-availability-v1/contact-read.ts':
            '18f812812c15858768223d929bcea9d7eae5154612747016ccd17864c229420a',
          'src/lib/navigation-copy.ts':
            '1a17eac4b355d8a3fb2b55b3fe1bc856cc3b3bce07434836d708b68407686f20',
          'src/lib/runtime/public-runtime.ts':
            'fa2e27d72b82b6bef73693cf93f19614e61ea51ebd11eb40d8ca4ec723f0c14e',
          'src/lib/catalog/publishers.ts':
            'a6795a35ebbf97246e22235df1ca868432e2cccfb26a38f0ebdf4524911192e6',
          'src/lib/catalog/resolve-head.ts':
            '4f1c9f144164580da038d127ab463f9f8ba6b20e882fc31151cb29850782d633',
          'src/lib/components/ProductEnquiry.svelte':
            '4935f091aa4db3b79b681e4bfdda4547b6654a87adee897b1da25abd17c03290',
          'src/lib/messaging/product-enquiry.ts':
            'fdabb058bd42b253d24e0cb2b0a27fb24ef1c65bdc1783e298d1098185e345f4',
          'src/lib/messaging/enquiry-context.ts':
            'fc00bb77f601427fb73defc0c758332c3465c2d583bfa1f7b996f808f508b735'
        }
      : {
          'src/routes/products/[naddr=naddr]/+page.svelte':
            '1869d4dc32ea794d5ee5c1d6cc6b4aac94483482d18cff0562c0cb10087b5278',
          'src/lib/catalog/product-view.ts':
            'e3739170e3c700246a80c1358c7b96541a5cbf1e4c26905e6ca60537adfce9cf',
          'src/lib/components/ListingFacts.svelte':
            '65d009b4940e7ced61347919689877da9473a2a66a44f61cc5e7bd0b2d217215',
          'src/lib/components/PublisherIdentity.svelte':
            '90b156e2401d40079ee559d65ce66ec09ecc4246bc80cfdfc5def0e21e8ce9ff',
          'src/lib/contracts/food-availability-v1/contact-read.ts':
            '18f812812c15858768223d929bcea9d7eae5154612747016ccd17864c229420a',
          'src/lib/navigation-copy.ts':
            '1a17eac4b355d8a3fb2b55b3fe1bc856cc3b3bce07434836d708b68407686f20',
          'src/lib/runtime/public-runtime.ts':
            'fa2e27d72b82b6bef73693cf93f19614e61ea51ebd11eb40d8ca4ec723f0c14e',
          'src/lib/catalog/publishers.ts':
            'a6795a35ebbf97246e22235df1ca868432e2cccfb26a38f0ebdf4524911192e6',
          'src/lib/catalog/resolve-head.ts':
            '4f1c9f144164580da038d127ab463f9f8ba6b20e882fc31151cb29850782d633'
        };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        name === 'src/lib/messaging/product-request-controller.ts' &&
        (await operationalFlagsAdmission(web))
      ) &&
      !(
        [
          'src/routes/products/[naddr=naddr]/+page.svelte',
          'src/lib/catalog/product-view.ts',
          'src/lib/runtime/public-runtime.ts'
        ].includes(name) && (await publicHidingAdmission(web))
      ) &&
      !(
        name === 'src/lib/runtime/public-runtime.ts' &&
        ((await listingEditorAdmission(web)) === 'owned-revision' ||
          (await listingEditorAdmission(web)) === 'owned-withdrawal')
      )
    )
      throw new Error('Invalid owned product presentation source');
  }
  return hasRequest ? 'request' : hasEnquiry ? 'enquiry' : true;
}
// This exact reviewed producer set owns the shared presentation/runtime split.
// A successor changes these identities only with new compiler qualification.
/** @param {string} web */
async function searchPresentationAdmission(web) {
  const route = publicText(
    await readOwned(web, 'src/routes/search/+page.svelte')
  );
  if (!route.includes('../../lib/catalog/search-view.ts')) return false;
  const producers = {
    'src/routes/search/+page.svelte':
      '0dea04fc5a5a41599774215c9157d8eb2076eb3fa4fe6355184a9a467a9cc72a',
    'src/lib/catalog/search-view.ts':
      '398cc2db59fb78d33df8cb8ce59d4e10a84feb3842709f2a7047033424798768',
    'src/lib/components/ListingRow.svelte':
      '48c6ab9b5fe887206b77a27bd0de697d346133ecb44d8ba973c8ffacc0cf5f03',
    'src/lib/components/SourceStatus.svelte':
      'd3cdb173fdeb4e7783f4b3b909a90c32ae4a66a5d87cba6a2766e718317775de',
    'src/lib/navigation-scroll.ts':
      '9a3dcd9bbde8bcdb18d8962816fa062262505bc3e8ad34d88c78d18dba3a8398',
    'src/lib/components/primitives/Disclosure.svelte':
      'c6d8361f31218644ac487a3d43412627cb8cc50906a921b95cf053047ce32bf3'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        ['src/lib/catalog/search-view.ts'].includes(name) &&
        (await publicHidingAdmission(web))
      )
    )
      throw new Error('Invalid owned search presentation source');
  }
  return true;
}
// HCP066 exact owned preference port changes the shared compiler chunk name.
// Preserve the full module/path/output rejection policy; no arbitrary admission.
/** @param {string} web */
async function preferencePublicationAdmission(web) {
  const pool = await readOwned(web, 'src/lib/nostr/public-pool.ts');
  if (!pool.toString().includes("'./inbox-preference-publication.ts'"))
    return false;
  const producers = {
    'src/lib/nostr/public-pool.ts':
      'd08c2c16936125e75a8195cfdc600f8c592cb942e1802d934823ee221c1c132b',
    'src/lib/nostr/inbox-preference-publication.ts':
      'fbad642340732bf7cf92905010af314829b80cc7b134121396f14e9bb8e765cc'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        name === 'src/lib/nostr/inbox-preference-publication.ts' &&
        (await operationalFlagsAdmission(web))
      ) &&
      !(
        name === 'src/lib/nostr/public-pool.ts' &&
        (await listingEditorAdmission(web)) === 'owned-withdrawal'
      )
    )
      throw new Error('Invalid owned preference publication source');
  }
  return true;
}
// HCP113 exact stored-artifact retry/resume ports retain original custody.
// Admit only their reviewed source pins and the complete observed closed graph.
/** @param {string} web */
async function listingPublicationAdmission(web) {
  const pool = await readOwned(web, 'src/lib/nostr/public-pool.ts');
  if (!pool.toString().includes("'./listing-publisher.ts'")) return false;
  const producers = {
    'src/lib/nostr/listing-publisher.ts':
      '1c8c3445513329d57fcddc75da63991f0c1c27c28caf87db381d32bba739c872',
    'src/lib/publishing/sign-listing.ts':
      '5051a2e0eb3353b9f099cb92dcbd3162003a9a32cebac6035b979ae3a9400bfe',
    'src/lib/publishing/retry-listing.ts':
      '67b6a110bb116ff2b0791833cc0df02889b206839baf04467e44fa5f2c1ec459',
    'src/lib/publishing/resume-listing.ts':
      '42cd0404f1e543e03350e6c8c92fce5a1c4f8eb02f9540831fc6c7cf84bba9a9',
    'src/lib/publishing/deliver-listing.ts':
      'a47d3ae5b4bc1d3cd85ff7385851953fbf21bd22e265e875e7710b192830d409',
    'src/lib/publishing/listing-operation-view.ts':
      '5f48bfb7056b56bd29357e605e20ef1375c559973fcc31f38d564a3b9eb07094',
    'src/lib/components/OperationStatus.svelte':
      'c6a1d741d16b577783e80b8f447cb959d5bf8b2d711cd5abcdc396be430a7472'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        [
          'src/lib/nostr/listing-publisher.ts',
          'src/lib/publishing/resume-listing.ts'
        ].includes(name) && (await operationalFlagsAdmission(web))
      ) &&
      !(
        (name === 'src/lib/nostr/listing-publisher.ts' &&
          (await listingEditorAdmission(web)) === 'owned-revision') ||
        ([
          'src/lib/nostr/listing-publisher.ts',
          'src/lib/publishing/sign-listing.ts',
          'src/lib/publishing/listing-operation-view.ts',
          'src/lib/components/OperationStatus.svelte'
        ].includes(name) &&
          (await listingEditorAdmission(web)) === 'owned-withdrawal')
      )
    )
      throw new Error('Invalid owned listing publication source');
  }
  return true;
}
/** @param {string} web */
async function sellingWorkspaceAdmission(web) {
  const source = await readOwned(web, 'src/routes/selling/+page.svelte');
  publicText(source);
  if (
    createHash('sha256').update(source).digest('hex') ===
    '23547dcfa10e85865e31b66c31ea0d043592e3c316c94da2d74bdf2dd419b38a'
  )
    return false;
  /** @type {Record<string,string>} */
  const producers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/runtime/public-runtime.ts':
      '8d2b840e94448b55642b1481d12785ed76d326735a4461e9795f57a98e8d2530',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/quota.ts':
      'e261a7c9c81d0685021663639b1bb543192e65580edb64fa58ccbb9a4a04cc95',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564',
    'src/routes/selling/+page.svelte':
      '65d982bdf3f625a38b6c886a31a2a699c92e9231636debc16d501e78d7c8faa0',
    'src/lib/components/SellingRow.svelte':
      'ce00de1bd033860b9c0a85a49c90c8a4d5b8a8223cfd5c0872572e548e79331d',
    'src/lib/publishing/selling-view.ts':
      'a9e896db6f0485699c069ee31eb20a6c5476095d0ffef1160aa76cc64a364dc2'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        [
          'src/routes/sell/+page.svelte',
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
          'src/routes/products/[naddr=naddr]/edit/+page.svelte',
          'src/lib/components/ListingEditor.svelte',
          'src/lib/publishing/listing-editor.ts',
          'src/lib/components/ListingReview.svelte',
          'src/lib/publishing/editor-workflow.ts',
          'src/lib/components/ConflictNotice.svelte',
          'src/lib/publishing/edit-source.ts',
          'src/lib/publishing/prepare-listing.ts',
          'src/lib/publishing/new-listing-readiness.ts',
          'src/lib/runtime/public-runtime.ts',
          'src/lib/persistence/public-publications.ts',
          'src/lib/persistence/quota.ts',
          'src/lib/publishing/revise-listing.ts',
          'src/lib/publishing/mark-sold.ts',
          'src/lib/components/ConfirmPanel.svelte',
          'src/lib/nostr/listing-publisher.ts',
          'src/lib/publishing/withdraw-listing.ts',
          'src/lib/publishing/sign-listing.ts',
          'src/lib/persistence/artifact-records.ts',
          'src/lib/publishing/listing-operation-view.ts',
          'src/lib/components/OperationStatus.svelte',
          'src/lib/runtime/effect-ownership.ts',
          'src/lib/nostr/public-pool.ts',
          'src/lib/persistence/public-receipts.ts',
          'src/routes/selling/+page.svelte',
          'src/lib/components/SellingRow.svelte',
          'src/lib/publishing/selling-view.ts',
          'src/lib/runtime/identity-session.ts',
          'src/lib/persistence/local-public-hides.ts',
          'src/lib/catalog/product-view.ts',
          'src/lib/catalog/search-run.ts',
          'src/lib/catalog/search-view.ts',
          'src/routes/+layout.svelte',
          'src/routes/products/[naddr=naddr]/+page.svelte',
          'src/routes/privacy/+page.svelte',
          'src/lib/components/PublisherHide.svelte'
        ].includes(name) && (await publicHidingAdmission(web))
      )
    )
      throw new Error('Invalid owned selling workspace source');
  }
  return true;
}
// HCP115 one shared editor activates all three routes together. Historical
// AccountGate routes remain labelled compatibility producers only.
/** @param {string} web */
async function listingEditorAdmission(web) {
  const routes = [
    'src/routes/sell/+page.svelte',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte'
  ];
  const enabled = await Promise.all(
    routes.map(async (name) =>
      publicText(await readOwned(web, name)).includes('ListingEditor.svelte')
    )
  );
  if (!enabled.some(Boolean)) return false;
  if (!enabled.every(Boolean))
    throw new Error('Invalid owned listing editor activation');
  const producers = {
    'src/routes/sell/+page.svelte':
      '3e99a6121e9ea317d30e631d9edfb05e6596301678f9ae0538e446f0438ae3f4',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '98935ebb01f950aee5da850758801c28303b042ba219759c4fb04c4974376a76',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '5c50dc2ba4c5734ffe4bd1c2e406e6699581b374fab3849d5c5a0072d9bad1e6',
    'src/lib/components/ListingEditor.svelte':
      'fed47cd7257ab3fe1878ef5779b74a4a3e01404d93956806477943190ffb2b18',
    'src/lib/publishing/listing-editor.ts':
      'f2f1be3c614c861f50293f33f5bf1d3ae1851f4b4452fb9e2f3a7b8963a1d1f9'
  };
  /** @type {Record<string,string>} */
  const continuity = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      'df9cc0f98620459a4667ccaab0c7ccfda42764c229bec61df2e06a48bfd8eb9d',
    'src/lib/publishing/listing-editor.ts':
      '9d7ea3f905188ab807b1779c2c0856e39a82cfe598bd071da33fabad66796012'
  };
  /** @type {Record<string,string>} */
  const reviewProducers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      '8be89452b0bfd50ff907efcf38873ef1b4007440464836db8b1ff2bc552b97ff',
    'src/lib/publishing/listing-editor.ts':
      'f781a9b028f4f441514bfb8622767c365b0425e7083acfd8deaaba57dc0b7dab',
    'src/lib/components/ListingReview.svelte':
      'bf4e6bf0fc76fa12672fabc441e1058d73c1f281a0a560aeafbdaa2aa36f6f90',
    'src/lib/publishing/editor-workflow.ts':
      '5bc8827c5ff167e0974f03a87e4e9db350e42f30c511f199b785c30ad365d838'
  };
  /** @type {Record<string,string>} */
  const editProducers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      '3b1d6248c256ce02a668a2c92654816dd0c2659b7b430d5e1d3aeba6f9326bfa',
    'src/lib/publishing/listing-editor.ts':
      'fa2c698892b820f10adc0c816641da6a2c40b5cfc3df8db2880f61659d81b4ec',
    'src/lib/components/ListingReview.svelte':
      'bf4e6bf0fc76fa12672fabc441e1058d73c1f281a0a560aeafbdaa2aa36f6f90',
    'src/lib/publishing/editor-workflow.ts':
      '5bc8827c5ff167e0974f03a87e4e9db350e42f30c511f199b785c30ad365d838',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/publishing/edit-source.ts':
      '985634e4c4eb2cd12839409552b1c56cd518021d4f031166a3c1ab8c4a0e7eff'
  };
  /** @type {Record<string,string>} */
  const revisionProducers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      'f0f19095619b4b4ab961ac58af86fd0c8e92f2034a55173382929894fe36bd6a',
    'src/lib/publishing/listing-editor.ts':
      '03e79904e6eb2dff06067a2e42c8d97b7081f7845221d4804649e9cb7df09f1f',
    'src/lib/components/ListingReview.svelte':
      '311d6bafb883f170faa8962698d5cafd4cca7b4a7237a6eca671eb1f19379a99',
    'src/lib/publishing/editor-workflow.ts':
      '0f8d718bbdad6697a8f9b519d8eafc80ca415e1d8ce245a3507111cc080f23ef',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/publishing/edit-source.ts':
      'bd0850df99dc0511fc0f9aa8bbb2526cab7a17ada9fd4d60464d5e3c9344811a',
    'src/lib/publishing/prepare-listing.ts':
      '0c396643de10d31fb8c28ef46386c3b54f331ddf1e784955139b2f834fce11ed',
    'src/lib/publishing/new-listing-readiness.ts':
      '46f4e89b435e732439add225166e80799f192a4f95ff5d8236a66b8614ac4172',
    'src/lib/runtime/public-runtime.ts':
      '8d2b840e94448b55642b1481d12785ed76d326735a4461e9795f57a98e8d2530',
    'src/lib/persistence/public-publications.ts':
      '58c3c230280616415d03020e0e4de6880afb754544fae205ae03d4a437c0424f',
    'src/lib/persistence/quota.ts':
      'c5332782042fa2561b237c8fd1f0c0c930390346c1f31f9b075dd9a0eacdf7e1',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/nostr/listing-publisher.ts':
      '1d7e1f99fe393626a3dda6796867be0d52ce4bcd064c185c9f4b3a24290183e2'
  };
  /** @type {Record<string,string>} */
  const withdrawalProducers = {
    'src/routes/sell/+page.svelte':
      '97f22f9a0309621e9acb5f1b328d3376bc9c5affbe5a7ea1b85ece76097b9c08',
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte':
      '1df59a753fbef2bc2ebf77890a79cbd3d7a5bb0dcef34c219df1b5e8043cf787',
    'src/routes/products/[naddr=naddr]/edit/+page.svelte':
      '09c3a6bde766045319cd36a00a82ca565f9f3b5cd5322982be9482836f312998',
    'src/lib/components/ListingEditor.svelte':
      '060f4863f6f4f2a4fb1471bb0d5eeedf31ab13b5c83d894f47757cf5a68099e6',
    'src/lib/publishing/listing-editor.ts':
      'fe067bc1609f783aeee8ad5a0f3178cff50cae81085377b2bbec7874b5f4349d',
    'src/lib/components/ListingReview.svelte':
      'deee41ca6c60b346afc7ed5e62e3101ee03b04fdd2a9aa8fa3f3dec585e3863b',
    'src/lib/publishing/editor-workflow.ts':
      'c87e4f46793fd9beeedb5027cdbb06d533cf7e5df7378c030355c4608738e0f6',
    'src/lib/components/ConflictNotice.svelte':
      'f0a71cab9e8f1f12a3421402e3bc3b2f96c2ddb35ad8949c6e9599b850c3a17c',
    'src/lib/publishing/edit-source.ts':
      '1432a8ada3d822cc9f634d40a7847a4d6d02c202ecfe59a9634543d57202f56b',
    'src/lib/publishing/prepare-listing.ts':
      '00b922e8b0bfc73cf5ac59857472e8dd627c8df62d28f1c646c5d7c76ce5ede9',
    'src/lib/publishing/new-listing-readiness.ts':
      'ba76be4d7309b1a6beb0c8e0f19b89f2595049e94709486b79eff38953e68893',
    'src/lib/runtime/public-runtime.ts':
      '8d2b840e94448b55642b1481d12785ed76d326735a4461e9795f57a98e8d2530',
    'src/lib/persistence/public-publications.ts':
      'be99c4114fb32880e0a9b48ab3593645532853fb4166fb506f398416db41a194',
    'src/lib/persistence/quota.ts':
      'e261a7c9c81d0685021663639b1bb543192e65580edb64fa58ccbb9a4a04cc95',
    'src/lib/publishing/revise-listing.ts':
      '1f03d43f460feb12a02489226131ec2f7b95df5f534fcbb0b49ccc686a1b0c06',
    'src/lib/publishing/mark-sold.ts':
      '80beaf8ad38b396154d5556e50e2cf5cd9f327bcdbc05f0db1ce190bf29f7234',
    'src/lib/components/ConfirmPanel.svelte':
      '9e25745303bdf408a14df5153a28e9579bfa019ab083225acb7d874e93a17d59',
    'src/lib/nostr/listing-publisher.ts':
      '10032c32482a766158d8a69027056e6a822a16b5f68ab9754b0f1cf42d0608f9',
    'src/lib/publishing/withdraw-listing.ts':
      '168064a7ce271f8342c7b7cbbf996bd96f8968e405eee0bb3e3dba1bdaf346fb',
    'src/lib/publishing/sign-listing.ts':
      'a1c123c40a190699b57378c5569fcbbfbc5e23ab4d4c0df499ca40ff82fe1a10',
    'src/lib/persistence/artifact-records.ts':
      'e3b235c5aaf19558b557feb6e17a35b2b1de39c1aa49cb2ef14084d670138d16',
    'src/lib/publishing/listing-operation-view.ts':
      '07f94625c0231a62f25c1db98c9459f8939c32505453633b58d2f72a894858ce',
    'src/lib/components/OperationStatus.svelte':
      'ca3a007899996f6331fadd2ead4c55dc7116c273626ecd2f36bcd4701065ae5f',
    'src/lib/runtime/effect-ownership.ts':
      'a181d1fa8b18d45b3e143de19af4dfdaab91779ac44df972d22e7fada645778b',
    'src/lib/nostr/public-pool.ts':
      '220b0b9ca0cea20c1a2e1a08a82fc4b68a1ba4dd6ffff1f45154b54735233806',
    'src/lib/persistence/public-receipts.ts':
      '10743a9d855c4f573eb42ca90a13c550dee9620e4ad3aac322858c696152e564'
  };
  let historical = true,
    current = true,
    reviewed = true,
    editing = true,
    revising = true;
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    const actual = createHash('sha256').update(bytes).digest('hex');
    historical = historical && actual === pin;
    current = current && actual === continuity[name];
    reviewed = reviewed && actual === reviewProducers[name];
    editing = editing && actual === editProducers[name];
    revising = revising && actual === revisionProducers[name];
  }
  if (reviewed) {
    for (const name of [
      'src/lib/components/ListingReview.svelte',
      'src/lib/publishing/editor-workflow.ts'
    ]) {
      const bytes = await readOwned(web, name);
      publicText(bytes);
      reviewed =
        reviewed &&
        createHash('sha256').update(bytes).digest('hex') ===
          reviewProducers[name];
    }
  }
  if (editing) {
    for (const name of [
      'src/lib/components/ListingReview.svelte',
      'src/lib/publishing/editor-workflow.ts',
      'src/lib/components/ConflictNotice.svelte',
      'src/lib/publishing/edit-source.ts'
    ]) {
      const bytes = await readOwned(web, name);
      publicText(bytes);
      editing =
        editing &&
        createHash('sha256').update(bytes).digest('hex') ===
          editProducers[name];
    }
  }
  if (revising) {
    for (const name of [
      'src/lib/components/ListingReview.svelte',
      'src/lib/publishing/editor-workflow.ts',
      'src/lib/components/ConflictNotice.svelte',
      'src/lib/publishing/edit-source.ts',
      'src/lib/publishing/prepare-listing.ts',
      'src/lib/publishing/new-listing-readiness.ts',
      'src/lib/runtime/public-runtime.ts',
      'src/lib/persistence/public-publications.ts',
      'src/lib/persistence/quota.ts',
      'src/lib/publishing/revise-listing.ts',
      'src/lib/publishing/mark-sold.ts',
      'src/lib/components/ConfirmPanel.svelte',
      'src/lib/nostr/listing-publisher.ts'
    ]) {
      const bytes = await readOwned(web, name);
      publicText(bytes);
      revising =
        revising &&
        createHash('sha256').update(bytes).digest('hex') ===
          revisionProducers[name];
    }
  }
  let withdrawing = publicText(
    await readOwned(web, 'src/lib/components/ListingEditor.svelte')
  ).includes('Request removal');
  if (withdrawing)
    for (const [name, pin] of Object.entries(withdrawalProducers)) {
      const bytes = await readOwned(web, name);
      publicText(bytes);
      withdrawing =
        withdrawing &&
        (createHash('sha256').update(bytes).digest('hex') === pin ||
          ([
            'src/lib/config/operation-flags.ts',
            'src/lib/runtime/capabilities.ts',
            'src/lib/components/AppShell.svelte',
            'src/lib/components/ListingEditor.svelte',
            'src/lib/components/MessageComposer.svelte',
            'src/lib/publishing/prepare-listing.ts',
            'src/lib/nostr/listing-publisher.ts',
            'src/lib/publishing/resume-listing.ts',
            'src/lib/messaging/inbox-setup.ts',
            'src/lib/messaging/inbox-setup-operation.ts',
            'src/lib/messaging/inbox-setup-view.ts',
            'src/lib/messaging/reply-controller.ts',
            'src/lib/messaging/product-request-controller.ts',
            'src/lib/nostr/inbox-preference-publication.ts'
          ].includes(name) &&
            (await operationalFlagsAdmission(web))) ||
          (name === 'src/lib/runtime/public-runtime.ts' &&
            (await publicHidingAdmission(web))) ||
          (name === 'src/lib/persistence/quota.ts' &&
            (await privacyDataAdmission(web))));
    }
  if (withdrawing) return 'owned-withdrawal';
  if (revising) return 'owned-revision';
  if (editing) return 'edit-source';
  if (!historical && !current && !reviewed)
    throw new Error('Invalid owned listing editor source');
  if (reviewed) return 'review';
  if (current) return 'continuity';
  return true;
}
/** @param {string} web */
async function inboxSetupAdmission(web) {
  const gate = await readOwned(web, 'src/lib/components/AccountGate.svelte');
  if (!gate.toString().includes("'./InboxSetup.svelte'")) return false;
  const producers = {
    'src/lib/components/InboxSetup.svelte':
      'd718e9ee2005af0be358fb3c3a051beb6d71568a81e2d43758bdec80edc959f6',
    'src/lib/messaging/inbox-setup-view.ts':
      '4ec9025232bb1355fef1ba039b46ce0a2be10e02e348e1b4eaadc3d1f3c2fd84',
    'src/lib/runtime/view-context.ts':
      'e2329113e29db35e983270217c47d38a44d4e0491fd710395e008786b5ae7033'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        name === 'src/lib/messaging/inbox-setup-view.ts' &&
        (await operationalFlagsAdmission(web))
      )
    )
      throw new Error('Invalid owned inbox compiler source');
  }
  return true;
}
// The historical inbox compiler may consume this exact current shared storage
// owner. Both producer identities are required; no old view pin is refreshed.
/** @param {string} web */
async function inboxProbeStorageAdmission(web) {
  const producers = {
    'src/lib/persistence/private-records.ts':
      '821ef96740cf644bff4ffe5c7816c6aa097aa419bd16b8b301809b963294dafe',
    'src/lib/persistence/private-storage.ts':
      'ba4a61dd1fa08fc89d00adac3c258c1f6ae7e5c1b15165facffe76ddd71d5e91',
    'src/lib/persistence/inbox-probes.ts':
      'b4d3089bcf08a5c9ac3a99dfcf8e2c2e2160a2c8996c19fccb2670614f7b4d7a'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (createHash('sha256').update(bytes).digest('hex') !== pin)
      throw new Error('Invalid owned probe storage source');
  }
  return true;
}
/** @param {string} web */
async function inboxPresentationAdmission(web) {
  const route = await readOwned(web, 'src/routes/messages/+page.svelte');
  if (!route.toString().includes("'../../lib/components/InboxStatus.svelte'"))
    return false;
  const producers = {
    'src/routes/messages/+page.svelte':
      '8279a92770f38858e197e790839bc98b4d7889214ebecdf47e542d632afbc84f',
    'src/lib/components/InboxStatus.svelte':
      '29f17df1ccece671ea2ba57578dc028fa541ce924e1cdc4b3e7753da500da53f',
    'src/lib/messaging/inbox-view.ts':
      'f725d1e122550232efa52b26139d3047d3bbfd6d3d11b1ef0a110b5e4b4410dc',
    'src/lib/messaging/inbox-sync.ts':
      '0d91f5895e7eb054b036a5cb80897a3d60a212d470029a82d11124c8536b3d2c',
    'src/lib/messaging/inbox-setup-view.ts':
      '4ec9025232bb1355fef1ba039b46ce0a2be10e02e348e1b4eaadc3d1f3c2fd84',
    'src/lib/messaging/decryption-queue.ts':
      '07661a883ee360e9fd7bd411893d637fd6b4b70aecb20101b826fe41b7ca4810',
    'src/lib/components/ConversationRow.svelte':
      'c5d0926ac9cdda18a6befe365227dfe62405f71598f59964ff36af9a0e85d743',
    'src/lib/messaging/inbox-list.ts':
      '9cc82877bfa8dbb2a7fa41bceb842793349416271f19569f49fdd5cfc1de07e5',
    'src/lib/components/ConversationThread.svelte':
      'ae59e6c06c0c5a86a58f49abed0ac9702631a694605b2c6686dfa65db0359080',
    'src/lib/components/MessageItem.svelte':
      '5c9af10ece18deeddd85e11453239c2a0b8e741cb84529e15c4ad87864704517',
    'src/lib/messaging/conversation-thread.ts':
      '7367f0e6d96a9f249514f66c2459274e856eb83462523e37264d4cd97d2e36d1',
    'src/lib/persistence/private-storage.ts':
      '66bab9a0a0a699b70ecba75c002ac0de20a84f3d015f5caefbe1697d0f6456bf',
    'src/routes/messages/[conversationId=local_id]/+page.svelte':
      'b06c6189adee80b68bc6657fa0e8504551fe9a8b1655a6d81807ebee9930a5e7',
    'src/lib/components/MessageComposer.svelte':
      'bc8ac0e2123b60957f1c289acbb2b8740e171097029f640f657cd79f5aa8423b',
    'src/lib/messaging/reply-controller.ts':
      'c13f1395b5317febcd23396babbcc116065ff96e8cbeb29c8eadfdfc14e45c2d',
    'src/lib/messaging/rumor-plan.ts':
      'b622889e96f911501524cd86f9fca9f2207ce853767494fb30799b00ae8275ed',
    'src/lib/persistence/private-send-reservations.ts':
      '5a57522fdb5acabecf836fb8d642a2adfb4bc7e041843b38fd1b96b673b3942d',
    'src/lib/messaging/send-identity.ts':
      'cc840afbf155e18030bea9b475884ca3e0068b79ef3da1459b661fe2822499df',
    'src/lib/components/ConversationOptions.svelte':
      'a33d9ffb13e28ce985799fa69342c8fa63816da70e1f9383d4e8e82f5a393c6d',
    'src/lib/messaging/cited-product.ts':
      'b4a43fad9b020cf647350350e5d0195df9a80d772ab6910e0e9fb8c11b4b0e48',
    'src/lib/nostr/enquiry-references.ts':
      'a17175337ee53279f898a7c6fa1af7e77ad26c0d2c0da46f26a9766fa701d365'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (
      createHash('sha256').update(bytes).digest('hex') !== pin &&
      !(
        name === 'src/lib/persistence/private-storage.ts' &&
        (await inboxProbeStorageAdmission(web))
      ) &&
      !(await privateBlockingAdmission(web))
    )
      throw new Error('Invalid owned inbox presentation source');
  }
  return true;
}
/** @param {string} webDirectory */
export async function auditOutput(webDirectory) {
  const web = path.resolve(webDirectory);
  if (path.basename(web) !== 'web' || (await realpath(web)) !== web)
    throw new Error('Output audit requires the owned web directory');
  // Pure derivation: auditing must not repair or overwrite poisoned metadata.
  const gitEnv = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
  );
  try {
    execFileSync(
      'git',
      [
        '-C',
        path.dirname(web),
        'check-ignore',
        '-q',
        '--',
        'web/static/build-info.json'
      ],
      { env: gitEnv, stdio: 'pipe' }
    );
  } catch {
    throw new Error('Generated public provenance must be ignored');
  }
  const expectedMetadata = Buffer.from(
    JSON.stringify(await deriveBuildInfo(web), null, 2) + '\n'
  );
  const generated = await readOwned(web, 'static/build-info.json');
  equal(generated, expectedMetadata, 'Stale or nonpublic generated provenance');
  const client = await inventory(path.join(web, '.svelte-kit/output/client'));
  const pages = await inventory(
    path.join(web, '.svelte-kit/output/prerendered/pages')
  );
  const output = await inventory(path.join(web, 'build'));
  const manifestBytes = client.get('.vite/manifest.json');
  if (!manifestBytes) throw new Error('Missing compiler manifest');
  const manifest = parse(publicText(manifestBytes));
  objectKeys(manifest, Object.keys(manifest));
  // Exact source-owned search route admission; root-only controlled consumers
  // retain their original compiler shape. Source ancestry/link bounds also apply.
  let hasSearch = false;
  try {
    await readOwned(web, 'src/routes/search/+page.svelte');
    hasSearch = true;
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT'))
      throw error;
  }
  const fullRouteSources = [
    'sell/+page.svelte',
    'selling/+page.svelte',
    'selling/drafts/[draftId=local_id]/+page.svelte',
    'products/[naddr=naddr]/+page.svelte',
    'products/[naddr=naddr]/edit/+page.svelte',
    'messages/+page.svelte',
    'messages/[conversationId=local_id]/+page.svelte',
    'about/+page.svelte',
    'privacy/+page.svelte'
  ];
  let presentRoutes = 0;
  for (const name of fullRouteSources) {
    try {
      await readOwned(web, 'src/routes/' + name);
      presentRoutes++;
    } catch (error) {
      if (!(
        error instanceof Error &&
        'code' in error &&
        error.code === 'ENOENT'
      ))
        throw error;
    }
  }
  const hasFullRoutes = presentRoutes !== 0;
  if (hasFullRoutes) {
    if (presentRoutes !== fullRouteSources.length || !hasSearch)
      throw new Error('Undeclared compiler module: incomplete route source');
    for (const name of [
      'products/[naddr=naddr]/+page.ts',
      'products/[naddr=naddr]/edit/+page.ts',
      'selling/drafts/[draftId=local_id]/+page.ts',
      'messages/[conversationId=local_id]/+page.ts'
    ])
      await readOwned(web, 'src/routes/' + name);
    for (const name of [
      'src/params.ts',
      'src/routes/+error.svelte',
      'src/lib/components/AccountGate.svelte'
    ])
      await readOwned(web, name);
  }
  const hasOperationalFlags =
    hasFullRoutes && (await operationalFlagsAdmission(web));
  const hasPrivacyData = hasFullRoutes && (await privacyDataAdmission(web));
  const hasSupportReporting =
    hasFullRoutes && (await supportReportingAdmission(web));
  const hasPrivateBlocking =
    hasFullRoutes && (await privateBlockingAdmission(web));
  const hasPublicHiding = hasFullRoutes && (await publicHidingAdmission(web));
  const hasPublicRuntime = await hasOwnedPublicRuntime(web);
  if (hasPublicRuntime && !hasFullRoutes)
    throw new Error('Incomplete owned root runtime routes');
  const hasSearchPresentation =
    hasSearch && hasPublicRuntime && (await searchPresentationAdmission(web));
  const hasProductPresentation =
    hasFullRoutes &&
    hasPublicRuntime &&
    (await productPresentationAdmission(web));
  const hasProductRequest = hasProductPresentation === 'request';
  const hasProductEnquiry =
    hasProductRequest || hasProductPresentation === 'enquiry';
  const hasPreferencePublication =
    hasPublicRuntime && (await preferencePublicationAdmission(web));
  const hasListingPublication =
    hasPreferencePublication && (await listingPublicationAdmission(web));
  const identityViewCompilerModule = hasListingPublication
    ? 'CapabilityGate'
    : 'view-context';
  const hasInboxSetup =
    hasFullRoutes &&
    hasPublicRuntime === 'public_identity' &&
    (await inboxSetupAdmission(web));
  const hasInboxPresentation =
    hasInboxSetup && (await inboxPresentationAdmission(web));
  const hasListingEditor = hasFullRoutes && (await listingEditorAdmission(web));
  if (hasListingEditor && !(hasListingPublication && hasInboxSetup))
    throw new Error('Invalid owned listing editor runtime authority');
  const hasSellingWorkspace =
    hasFullRoutes && (await sellingWorkspaceAdmission(web));
  if (hasSellingWorkspace && hasListingEditor !== 'owned-withdrawal')
    throw new Error('Invalid owned selling workspace activation');
  const hasListingRevision =
    hasListingEditor === 'owned-revision' ||
    hasListingEditor === 'owned-withdrawal';
  const hasListingEdit =
    hasListingEditor === 'edit-source' || hasListingRevision;
  const hasListingReview = hasListingEditor === 'review' || hasListingEdit;
  const hasListingContinuity =
    hasListingEditor === 'continuity' || hasListingReview;
  const identityCompilerModule = hasListingEditor
    ? 'view-context'
    : hasInboxSetup
      ? 'public-runtime'
      : hasPreferencePublication
        ? 'identity-session'
        : 'heads';
  // Admit compiler-owned module identities, not arbitrary extensions/copy roots.
  const names = new Set([
    'entry/app',
    'entry/start',
    'entry/payload',
    'nodes/0',
    'nodes/1',
    'nodes/2',
    'error-template',
    'preload-helper',
    'client',
    'client.svelte',
    'client-entry',
    'state',
    'Button'
  ]);
  if (hasSearch) {
    names.add('nodes/3');
    names.add('routes');
  }
  if (hasFullRoutes) {
    for (let index = 4; index <= 12; index++) names.add('nodes/' + index);
    for (const name of [
      'AccountGate',
      'references',
      'legacy',
      ...(hasPublicRuntime === 'public_identity' ? [] : ['rolldown-runtime']),
      'shared-errors'
    ])
      names.add(name);
  }
  if (hasPublicRuntime)
    for (const name of [
      'public-key',
      ...(hasSearchPresentation ? ['Disclosure', 'navigation'] : ['dist']),
      'negentropy',
      ...(hasPublicRuntime === 'public_identity'
        ? [identityViewCompilerModule, identityCompilerModule]
        : ['budgets'])
    ])
      names.add(name);
  if (hasProductPresentation) names.add('publishers');
  if (hasInboxSetup) names.add('private-handles');
  if (hasProductEnquiry)
    for (const name of hasProductRequest
      ? ['inbox-view', 'MessageComposer']
      : ['inbox-view', 'inbox-setup-view', 'CapabilityGate'])
      names.add(name);
  if (hasInboxPresentation)
    for (const name of hasProductRequest
      ? ['InboxStatus', 'InboxSetup', 'read']
      : ['InboxStatus', 'InboxSetup', 'read', 'resolve-head'])
      names.add(name);
  if (hasListingEditor)
    for (const name of ['ListingEditor', 'view-context', 'public-runtime'])
      names.add(name);
  if (hasListingContinuity) {
    names.clear();
    for (const name of [
      'entry/app',
      'nodes/0',
      'nodes/1',
      'nodes/10',
      'nodes/11',
      'nodes/12',
      'nodes/2',
      'nodes/3',
      'nodes/4',
      'nodes/5',
      'nodes/6',
      'nodes/7',
      'nodes/8',
      'nodes/9',
      'error-template',
      'MessageComposer',
      'ListingEditor',
      'preload-helper',
      'Disclosure',
      'InboxStatus',
      'inbox-view',
      'public-key',
      'routes',
      'Button',
      'client',
      'InboxSetup',
      'legacy',
      'navigation',
      'shared-errors',
      'AccountGate',
      'read',
      'client.svelte',
      'publishers',
      'CapabilityGate',
      'public-runtime',
      'SearchForm',
      'state',
      'references',
      'client-entry',
      'entry/start',
      'entry/payload',
      'negentropy'
    ])
      names.add(name);
  }
  if (hasListingEdit) names.add('resolve-head');
  if (hasListingRevision) {
    for (const name of ['read', 'public-key', 'resolve-head'])
      names.delete(name);
    names.add('rolldown-runtime');
  }
  if (hasSellingWorkspace) {
    names.delete('AccountGate');
    names.add('OperationStatus');
  }
  if (hasPublicHiding) {
    names.delete('rolldown-runtime');
    names.add('local-public-hides');
    names.add('public-key');
  }
  if (hasPrivateBlocking) {
    names.delete('local-public-hides');
    names.add('local-private-blocks');
    names.add('private-session');
  }
  if (hasSupportReporting) {
    names.add('ReportPreview');
    names.add('navigation-url');
  }
  if (hasPrivacyData) names.add('SupportLink');
  if (hasOperationalFlags) names.add('capabilities');
  const admitted = new Map([['build-info.json', expectedMetadata]]);
  const seen = new Set();
  for (const [key, record] of Object.entries(manifest)) {
    objectKeys(record, [
      'file',
      'name',
      'src',
      'isEntry',
      'isDynamicEntry',
      'imports',
      'dynamicImports',
      'css'
    ]);
    if (!names.has(record.name) || seen.has(record.name))
      throw new Error('Undeclared compiler module');
    seen.add(record.name);
    const generatedModule = hasFullRoutes
      ? /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/(?:[0-9]|1[012]))|shared\/error-template)\.js$/
      : hasSearch
        ? /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/[0123])|shared\/error-template)\.js$/
        : /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/[012])|shared\/error-template)\.js$/;
    const frameworkModule =
      /^node_modules\/\.pnpm\/[^/]+\/node_modules\/@sveltejs\/kit\/src\/runtime\/client\/(?:client-entry|entry|payload)\.js$/;
    if (!(
      (hasPublicRuntime && key === sdkNegentropy) ||
      generatedModule.test(key) ||
      frameworkModule.test(key) ||
      /^_[A-Za-z0-9_-]+\.js$/.test(key)
    ))
      throw new Error('Undeclared compiler input');
    if (
      record.src !== undefined &&
      (record.src !== key ||
        !(
          generatedModule.test(key) ||
          frameworkModule.test(key) ||
          (hasPublicRuntime && key === sdkNegentropy)
        ))
    )
      throw new Error('Invalid compiler source');
    if (
      !/^_app\/immutable\/(?:(?:entry|nodes)\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+|chunks\/[A-Za-z0-9_-]+)\.js$/.test(
        record.file
      )
    )
      throw new Error('Undeclared compiler output');
    for (const flag of ['isEntry', 'isDynamicEntry'])
      if (record[flag] !== undefined && typeof record[flag] !== 'boolean')
        throw new Error('Invalid compiler flag');
    for (const field of ['imports', 'dynamicImports', 'css']) {
      if (
        record[field] !== undefined &&
        (!Array.isArray(record[field]) ||
          record[field].some((item) => typeof item !== 'string') ||
          new Set(record[field]).size !== record[field].length)
      )
        throw new Error('Invalid compiler references');
    }
    for (const dependency of [
      ...(record.imports ?? []),
      ...(record.dynamicImports ?? [])
    ])
      if (!Object.hasOwn(manifest, dependency))
        throw new Error('Unresolved compiler import');
    for (const file of [record.file, ...(record.css ?? [])]) {
      safeName(file);
      if (
        file !== record.file &&
        !/^_app\/immutable\/assets\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.css$/.test(
          file
        )
      )
        throw new Error('Undeclared compiler resource');
      const bytes = client.get(file);
      if (!bytes) throw new Error('Missing compiler output');
      publicText(bytes);
      const previous = admitted.get(file);
      if (previous && !previous.equals(bytes))
        throw new Error('Conflicting compiler output');
      admitted.set(file, bytes);
    }
  }
  equal([...seen].sort(), [...names].sort(), 'Incomplete compiler modules');
  if (hasPublicRuntime && !hasListingContinuity) {
    const records = Object.entries(manifest);
    /** @param {string} name */
    function requiredRecord(name) {
      const entry = records.find(([, record]) => record.name === name);
      if (!entry) throw new Error('Incomplete owned SDK compiler module');
      return entry;
    }
    const dist = requiredRecord(
      hasInboxSetup
        ? 'public-runtime'
        : hasSearchPresentation
          ? 'Disclosure'
          : 'dist'
    );
    const root = requiredRecord('nodes/0')[1];
    const [budgetKey, budget] = requiredRecord(
      hasPublicRuntime === 'public_identity' ? 'Button' : 'budgets'
    );
    if (
      !/^_[A-Za-z0-9_-]+\.js$/.test(budgetKey) ||
      Object.keys(budget).some(
        (field) =>
          !(
            hasPublicRuntime === 'public_identity'
              ? ['file', 'name', 'imports']
              : ['file', 'name']
          ).includes(field)
      )
    )
      throw new Error('Invalid owned budgets compiler identity');
    if (hasPublicRuntime === 'public_identity')
      equal(
        budget.imports,
        [requiredRecord('client')[0]],
        'Invalid owned budgets compiler dependencies'
      );
    for (const consumer of hasSearchPresentation
      ? [
          hasInboxSetup ? 'public-runtime' : 'Disclosure',
          'nodes/9',
          'routes',
          ...(hasProductPresentation ? ['nodes/7', 'publishers'] : [])
        ]
      : ['nodes/0', 'routes']) {
      if (!requiredRecord(consumer)[1].imports?.includes(budgetKey))
        throw new Error('Invalid owned budgets compiler edge');
    }
    const sdk = manifest[sdkNegentropy];
    if (
      !sdk ||
      sdk.name !== 'negentropy' ||
      sdk.src !== sdkNegentropy ||
      sdk.isDynamicEntry !== true ||
      sdk.isEntry !== undefined ||
      sdk.css !== undefined ||
      sdk.dynamicImports !== undefined
    )
      throw new Error('Invalid owned SDK compiler identity');
    equal(
      sdk.imports,
      hasListingEditor
        ? [requiredRecord('view-context')[0], dist[0]]
        : hasInboxSetup
          ? [dist[0]]
          : hasPublicRuntime === 'public_identity'
            ? [requiredRecord(identityCompilerModule)[0], dist[0]]
            : [dist[0]],
      'Invalid owned SDK compiler dependency'
    );
    equal(
      hasSearchPresentation ? dist[1].dynamicImports : root.dynamicImports,
      [sdkNegentropy],
      'Invalid owned root compiler edge'
    );
    if (hasSearchPresentation) {
      if (
        root.dynamicImports !== undefined ||
        !root.imports?.includes(dist[0]) ||
        !requiredRecord('nodes/9')[1].imports?.includes(dist[0])
      )
        throw new Error('Invalid owned shared compiler edge');
      const navigation = requiredRecord('navigation');
      if (!requiredRecord('nodes/9')[1].imports?.includes(navigation[0]))
        throw new Error('Invalid owned navigation compiler edge');
      equal(
        navigation[1].imports,
        ['entry/payload', 'shared-errors', 'client', 'client.svelte'].map(
          (name) => requiredRecord(name)[0]
        ),
        'Invalid owned navigation compiler dependencies'
      );
      if (navigation[1].dynamicImports !== undefined)
        throw new Error('Invalid owned navigation compiler dynamic imports');
      equal(
        dist[1].imports,
        (hasInboxSetup
          ? [
              'public-key',
              'preload-helper',
              'private-handles',
              ...(hasListingEditor ? ['view-context'] : []),
              'Button'
            ]
          : hasPublicRuntime === 'public_identity'
            ? [
                'public-key',
                'preload-helper',
                identityCompilerModule,
                'client',
                'Button'
              ]
            : [
                'rolldown-runtime',
                'preload-helper',
                'public-key',
                'client',
                'budgets',
                ...(hasListingPublication && hasProductPresentation
                  ? ['Button']
                  : [])
              ]
        ).map((name) => requiredRecord(name)[0]),
        'Invalid owned shared compiler dependencies'
      );
      // Historical public-only producers still compile current shared source.
      // The exact HCP112 ports add only this existing, closed Button edge.
      if (
        hasPublicRuntime !== 'public_identity' &&
        hasListingPublication &&
        hasProductPresentation
      ) {
        const [buttonKey, button] = requiredRecord('Button');
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(buttonKey) ||
          Object.keys(button).some(
            (field) => !['file', 'name', 'imports'].includes(field)
          )
        )
          throw new Error('Invalid owned public-only Button compiler identity');
        equal(
          button.imports,
          [requiredRecord('client')[0]],
          'Invalid owned public-only Button compiler dependencies'
        );
      }
      for (const [key, record] of [dist, navigation]) {
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
          record.src !== undefined ||
          record.isEntry !== undefined ||
          record.isDynamicEntry !== undefined ||
          record.css !== undefined
        )
          throw new Error('Invalid owned search compiler chunk');
      }
    }
    if (hasPublicRuntime === 'public_identity') {
      /** @type {[string, string[]][]} */
      const identityDependencies = [
        [
          identityViewCompilerModule,
          hasListingPublication
            ? ['client', 'Button']
            : hasProductRequest
              ? ['public-runtime', 'client', 'Button']
              : hasProductEnquiry
                ? [identityCompilerModule]
                : [
                    ...(hasPreferencePublication ? [] : ['public-key']),
                    identityCompilerModule,
                    'client',
                    'Button'
                  ]
        ],
        [
          identityCompilerModule,
          hasListingEditor
            ? ['public-key', 'private-handles', 'Button']
            : hasInboxSetup
              ? ['public-key', 'preload-helper', 'private-handles', 'Button']
              : ['public-key', 'Button']
        ],
        ...(hasInboxSetup
          ? [/** @type {[string, string[]]} */ (['private-handles', []])]
          : [])
      ];
      for (const [name, dependencies] of identityDependencies) {
        const [key, record] = requiredRecord(name);
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
          Object.keys(record).some(
            (field) =>
              ![
                'file',
                'name',
                'imports',
                ...(hasInboxSetup && name === 'public-runtime'
                  ? ['dynamicImports']
                  : [])
              ].includes(field)
          )
        )
          throw new Error('Invalid owned identity compiler chunk');
        equal(
          record.imports ?? [],
          dependencies.map((dependency) => requiredRecord(dependency)[0]),
          'Invalid owned identity compiler dependencies'
        );
      }
      if (hasInboxSetup) {
        const [disclosureKey, disclosure] = requiredRecord('Disclosure');
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(disclosureKey) ||
          Object.keys(disclosure).some(
            (field) => !['file', 'name', 'imports'].includes(field)
          )
        )
          throw new Error('Invalid owned inbox disclosure compiler identity');
        equal(
          disclosure.imports,
          [requiredRecord('client')[0]],
          'Invalid owned inbox disclosure compiler dependencies'
        );
        const [, gate] = requiredRecord('AccountGate');
        equal(
          gate.imports,
          (hasListingPublication
            ? [
                hasListingEditor ? 'view-context' : 'public-runtime',
                'client',
                identityViewCompilerModule,
                'InboxSetup'
              ]
            : hasProductRequest
              ? ['client', identityViewCompilerModule, 'InboxSetup']
              : hasProductEnquiry
                ? [
                    'client',
                    'CapabilityGate',
                    identityViewCompilerModule,
                    'InboxSetup'
                  ]
                : hasInboxPresentation
                  ? ['client', identityViewCompilerModule, 'InboxSetup']
                  : [
                      'public-key',
                      'private-handles',
                      'public-runtime',
                      'client',
                      'Button',
                      identityViewCompilerModule
                    ]
          ).map((name) => requiredRecord(name)[0]),
          'Invalid owned inbox compiler dependencies'
        );
        if (gate.dynamicImports !== undefined)
          throw new Error('Invalid owned inbox compiler dynamic imports');
      }
      const [identityKey] = requiredRecord(identityViewCompilerModule);
      if (
        !root.imports?.includes(identityKey) ||
        !requiredRecord('AccountGate')[1].imports?.includes(identityKey)
      )
        throw new Error('Invalid owned identity compiler consumer');
    }
    for (const name of hasSearchPresentation
      ? ['public-key']
      : ['dist', 'public-key']) {
      const [key, record] = requiredRecord(name);
      if (
        !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
        record.src !== undefined ||
        record.isEntry !== undefined ||
        record.isDynamicEntry !== undefined ||
        record.css !== undefined
      )
        throw new Error('Invalid owned SDK compiler chunk');
    }
  }
  if (hasInboxPresentation && !hasListingContinuity) {
    const records = Object.entries(manifest);
    const entry = (/** @type {string} */ name) => {
      const row = records.find(([, r]) => r.name === name);
      if (!row) throw new Error('Missing inbox compiler module');
      return row;
    };
    /** @type {[string,string[]][]} */
    const dependencies = hasListingPublication
      ? [
          [
            'InboxStatus',
            [
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'client',
              'Button',
              'navigation',
              'CapabilityGate',
              'InboxSetup',
              'inbox-view'
            ]
          ],
          [
            'InboxSetup',
            [
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'client',
              'Button'
            ]
          ],
          ['read', [hasListingEditor ? 'view-context' : 'public-runtime']],
          ['nodes/4', ['client', 'AccountGate', 'InboxStatus']],
          [
            'nodes/5',
            [
              'public-key',
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'state',
              'legacy',
              'inbox-view',
              'InboxStatus',
              'publishers',
              'MessageComposer'
            ]
          ],
          [
            'inbox-view',
            [
              'public-key',
              'private-handles',
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'references',
              'Button',
              'read'
            ]
          ],
          [
            'MessageComposer',
            [
              'private-handles',
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'references',
              'client',
              'Button',
              'navigation',
              'inbox-view',
              'publishers'
            ]
          ],
          [
            'nodes/0',
            [
              'public-key',
              ...(hasListingEditor ? ['view-context'] : []),
              'public-runtime',
              'client',
              'Button',
              'client.svelte',
              'state',
              'CapabilityGate',
              'Disclosure'
            ]
          ]
        ]
      : hasProductRequest
        ? [
            [
              'InboxStatus',
              [
                'public-runtime',
                'client',
                'Button',
                'navigation',
                identityViewCompilerModule,
                'InboxSetup',
                'inbox-view'
              ]
            ],
            [
              'InboxSetup',
              [
                'public-key',
                'private-handles',
                'public-runtime',
                'client',
                'Button',
                identityViewCompilerModule
              ]
            ],
            ['read', ['public-runtime']],
            ['nodes/4', ['client', 'AccountGate', 'InboxStatus']],
            [
              'nodes/5',
              [
                'public-key',
                'public-runtime',
                'references',
                'client',
                'Button',
                'client.svelte',
                'state',
                identityViewCompilerModule,
                'legacy',
                'InboxSetup',
                'inbox-view',
                'InboxStatus',
                'publishers',
                'MessageComposer'
              ]
            ],
            [
              'inbox-view',
              [
                'public-key',
                'private-handles',
                'public-runtime',
                'references',
                'Button',
                'InboxSetup',
                'read'
              ]
            ],
            [
              'MessageComposer',
              [
                'private-handles',
                'public-runtime',
                'references',
                'client',
                'Button',
                'navigation',
                'InboxSetup',
                'inbox-view',
                'publishers'
              ]
            ],
            [
              'nodes/0',
              [
                'public-key',
                'public-runtime',
                'client',
                'Button',
                'client.svelte',
                'state',
                identityViewCompilerModule,
                'Disclosure'
              ]
            ]
          ]
        : hasProductEnquiry
          ? [
              [
                'InboxStatus',
                [
                  'public-runtime',
                  'client',
                  'Button',
                  'navigation',
                  'CapabilityGate',
                  identityViewCompilerModule,
                  'inbox-setup-view',
                  'InboxSetup',
                  'inbox-view'
                ]
              ],
              [
                'InboxSetup',
                [
                  'public-runtime',
                  'client',
                  'Button',
                  identityViewCompilerModule,
                  'inbox-setup-view'
                ]
              ],
              ['read', ['public-runtime']],
              ['nodes/4', ['client', 'AccountGate', 'InboxStatus']],
              [
                'nodes/5',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'references',
                  'client',
                  'Button',
                  'client.svelte',
                  'navigation',
                  'state',
                  identityViewCompilerModule,
                  'legacy',
                  'inbox-setup-view',
                  'inbox-view',
                  'InboxStatus',
                  'resolve-head'
                ]
              ],
              ['resolve-head', ['public-runtime', 'Button', 'read']],
              [
                'inbox-view',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'references',
                  'Button',
                  'inbox-setup-view',
                  'read'
                ]
              ],
              [
                'inbox-setup-view',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'Button',
                  identityViewCompilerModule
                ]
              ],
              ['CapabilityGate', ['client', 'Button']],
              [
                'nodes/0',
                [
                  'public-key',
                  'public-runtime',
                  'client',
                  'Button',
                  'client.svelte',
                  'state',
                  'CapabilityGate',
                  'Disclosure',
                  identityViewCompilerModule
                ]
              ]
            ]
          : [
              [
                'InboxStatus',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'references',
                  'client',
                  'Button',
                  'navigation',
                  identityViewCompilerModule,
                  'InboxSetup',
                  'read'
                ]
              ],
              [
                'InboxSetup',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'client',
                  'Button',
                  identityViewCompilerModule
                ]
              ],
              ['read', ['public-runtime']],
              ['nodes/4', ['client', 'AccountGate', 'InboxStatus']],
              [
                'nodes/5',
                [
                  'public-key',
                  'private-handles',
                  'public-runtime',
                  'references',
                  'client',
                  'Button',
                  'client.svelte',
                  'navigation',
                  'state',
                  identityViewCompilerModule,
                  'legacy',
                  'InboxSetup',
                  'InboxStatus',
                  'resolve-head'
                ]
              ],
              ['resolve-head', ['public-runtime', 'Button', 'read']]
            ];
    for (const [name, imports] of dependencies) {
      const [key, row] = entry(name);
      equal(
        (row.imports ?? []).toSorted(),
        imports.map((value) => entry(value)[0]).sort(),
        'Invalid owned inbox presentation compiler dependencies'
      );
      if (row.dynamicImports !== undefined)
        throw new Error(
          'Invalid owned inbox presentation compiler dynamic imports'
        );
      if (name.startsWith('nodes/')) {
        if (
          hasProductEnquiry &&
          name === 'nodes/0' &&
          (row.css?.length !== 1 ||
            !/^_app\/immutable\/assets\/0\.[A-Za-z0-9_-]+\.css$/.test(
              row.css[0]
            ))
        )
          throw new Error(
            'Invalid owned inbox presentation compiler entry role'
          );
        if (
          row.src !== key ||
          row.isEntry !== true ||
          row.isDynamicEntry !== undefined ||
          (!(hasProductEnquiry && name === 'nodes/0') && row.css !== undefined)
        )
          throw new Error(
            'Invalid owned inbox presentation compiler entry role'
          );
      }
      if (!name.startsWith('nodes/')) {
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
          Object.keys(row).some(
            (field) => !['file', 'name', 'imports'].includes(field)
          )
        )
          throw new Error('Invalid owned inbox presentation compiler identity');
      }
    }
  }
  if (hasProductPresentation && !hasListingContinuity) {
    const records = Object.entries(manifest);
    const entry = (/** @type {string} */ name) => {
      const row = records.find(([, r]) => r.name === name);
      if (!row) throw new Error('Missing product compiler module');
      return row;
    };
    const [publisherKey, publisher] = entry('publishers');
    if (
      !/^_[A-Za-z0-9_-]+\.js$/.test(publisherKey) ||
      Object.keys(publisher).some(
        (field) => !['file', 'name', 'imports'].includes(field)
      )
    )
      throw new Error('Invalid owned product compiler identity');
    equal(
      publisher.imports,
      (hasProductRequest
        ? [
            'public-key',
            ...(hasListingEditor ? ['view-context'] : []),
            'public-runtime',
            'Button',
            'read'
          ]
        : hasInboxSetup
          ? [
              'public-key',
              identityCompilerModule,
              'Button',
              ...(hasInboxPresentation ? ['resolve-head'] : [])
            ]
          : hasPublicRuntime === 'public_identity'
            ? ['public-key', identityCompilerModule, 'Disclosure', 'Button']
            : ['public-key', 'Disclosure', 'budgets']
      ).map((name) => entry(name)[0]),
      'Invalid owned product publisher compiler dependencies'
    );
    equal(
      entry('nodes/7')[1].imports?.toSorted(),
      (hasProductRequest
        ? [
            'public-key',
            ...(hasListingEditor ? ['view-context'] : []),
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'state',
            identityViewCompilerModule,
            'Disclosure',
            'InboxSetup',
            'MessageComposer'
          ]
        : hasInboxSetup
          ? [
              'public-key',
              'references',
              identityCompilerModule,
              'client',
              'Button',
              'client.svelte',
              'state',
              'Disclosure',
              'publishers',
              ...(hasInboxPresentation ? ['resolve-head'] : []),
              ...(hasProductEnquiry ? ['inbox-view'] : [])
            ]
          : hasPublicRuntime === 'public_identity'
            ? [
                'public-key',
                'references',
                identityCompilerModule,
                'Disclosure',
                'client',
                'Button',
                'client.svelte',
                'state',
                'publishers'
              ]
            : [
                'rolldown-runtime',
                'references',
                'Disclosure',
                'client',
                'budgets',
                'client.svelte',
                'state',
                'Button',
                'publishers'
              ]
      )
        .map((name) => entry(name)[0])
        .sort(),
      'Invalid owned product compiler dependencies'
    );
    equal(
      entry('nodes/9')[1].imports?.toSorted(),
      (hasProductRequest
        ? [
            ...(hasListingEditor ? ['view-context'] : []),
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'Disclosure',
            'routes',
            'publishers'
          ]
        : hasInboxSetup
          ? [
              'references',
              identityCompilerModule,
              'client',
              'Button',
              'client.svelte',
              'navigation',
              'state',
              'Disclosure',
              'routes',
              'publishers',
              ...(hasInboxPresentation ? ['resolve-head'] : [])
            ]
          : hasPublicRuntime === 'public_identity'
            ? [
                'references',
                identityCompilerModule,
                'Disclosure',
                'client',
                'Button',
                'client.svelte',
                'navigation',
                'state',
                'routes',
                'publishers'
              ]
            : [
                'references',
                'Disclosure',
                'client',
                'budgets',
                'client.svelte',
                'navigation',
                'state',
                'Button',
                'routes',
                'publishers'
              ]
      )
        .map((name) => entry(name)[0])
        .sort(),
      'Invalid owned search product compiler dependencies'
    );
    if (
      entry('nodes/7')[1].dynamicImports !== undefined ||
      entry('nodes/9')[1].dynamicImports !== undefined
    )
      throw new Error('Invalid owned product compiler dynamic imports');
  }
  if (hasListingEditor) {
    // Complete measured HCP115 compiler graph, including roles and ordered
    // edges for all existing consumers. Only actual compiler hashes vary.
    /** @type {Record<string, unknown>} */
    const expected = hasListingContinuity
      ? {
          'entry/app': {
            file: '_app/immutable/entry/app.HASH.js',
            name: 'entry/app',
            src: '.svelte-kit/generated/build/client-optimized/app.js',
            isEntry: true,
            imports: [
              'public-key',
              'preload-helper',
              'shared-errors',
              'references'
            ],
            dynamicImports: [
              'nodes/0',
              'nodes/1',
              'nodes/2',
              'nodes/3',
              'nodes/4',
              'nodes/5',
              'nodes/6',
              'nodes/7',
              'nodes/8',
              'nodes/9',
              'nodes/10',
              'nodes/11',
              'nodes/12',
              'error-template'
            ]
          },
          'nodes/0': {
            file: '_app/immutable/nodes/0.HASH.js',
            name: 'nodes/0',
            src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
            isEntry: true,
            imports: [
              'public-key',
              'public-runtime',
              'client',
              'Button',
              'client.svelte',
              'state',
              'CapabilityGate',
              'Disclosure'
            ],
            css: ['_app/immutable/assets/0.HASH.css']
          },
          'nodes/1': {
            file: '_app/immutable/nodes/1.HASH.js',
            name: 'nodes/1',
            src: '.svelte-kit/generated/build/client-optimized/nodes/1.js',
            isEntry: true,
            imports: ['client', 'client.svelte', 'state', 'legacy']
          },
          'nodes/10': {
            file: '_app/immutable/nodes/10.HASH.js',
            name: 'nodes/10',
            src: '.svelte-kit/generated/build/client-optimized/nodes/10.js',
            isEntry: true,
            imports: [
              'public-key',
              'client',
              'Button',
              'client.svelte',
              'navigation',
              'state',
              'legacy',
              'routes',
              'ListingEditor'
            ]
          },
          'nodes/11': {
            file: '_app/immutable/nodes/11.HASH.js',
            name: 'nodes/11',
            src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
            isEntry: true,
            imports: ['client', 'legacy', 'AccountGate']
          },
          'nodes/12': {
            file: '_app/immutable/nodes/12.HASH.js',
            name: 'nodes/12',
            src: '.svelte-kit/generated/build/client-optimized/nodes/12.js',
            isEntry: true,
            imports: [
              'public-key',
              'client',
              'client.svelte',
              'navigation',
              'state',
              'ListingEditor'
            ]
          },
          'nodes/2': {
            file: '_app/immutable/nodes/2.HASH.js',
            name: 'nodes/2',
            src: '.svelte-kit/generated/build/client-optimized/nodes/2.js',
            isEntry: true,
            imports: ['client', 'Button', 'SearchForm', 'routes']
          },
          'nodes/3': {
            file: '_app/immutable/nodes/3.HASH.js',
            name: 'nodes/3',
            src: '.svelte-kit/generated/build/client-optimized/nodes/3.js',
            isEntry: true,
            imports: ['client', 'legacy']
          },
          'nodes/4': {
            file: '_app/immutable/nodes/4.HASH.js',
            name: 'nodes/4',
            src: '.svelte-kit/generated/build/client-optimized/nodes/4.js',
            isEntry: true,
            imports: ['client', 'AccountGate', 'InboxStatus']
          },
          'nodes/5': {
            file: '_app/immutable/nodes/5.HASH.js',
            name: 'nodes/5',
            src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
            isEntry: true,
            imports: [
              'public-key',
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'state',
              'legacy',
              'inbox-view',
              'InboxStatus',
              'publishers',
              'MessageComposer'
            ]
          },
          'nodes/6': {
            file: '_app/immutable/nodes/6.HASH.js',
            name: 'nodes/6',
            src: '.svelte-kit/generated/build/client-optimized/nodes/6.js',
            isEntry: true,
            imports: ['client', 'legacy']
          },
          'nodes/7': {
            file: '_app/immutable/nodes/7.HASH.js',
            name: 'nodes/7',
            src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
            isEntry: true,
            imports: [
              'public-key',
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'state',
              'CapabilityGate',
              'Disclosure',
              'InboxSetup',
              'MessageComposer'
            ]
          },
          'nodes/8': {
            file: '_app/immutable/nodes/8.HASH.js',
            name: 'nodes/8',
            src: '.svelte-kit/generated/build/client-optimized/nodes/8.js',
            isEntry: true,
            imports: [
              'public-key',
              'client',
              'client.svelte',
              'navigation',
              'state',
              'ListingEditor'
            ]
          },
          'nodes/9': {
            file: '_app/immutable/nodes/9.HASH.js',
            name: 'nodes/9',
            src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
            isEntry: true,
            imports: [
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'navigation',
              'state',
              'Disclosure',
              'SearchForm',
              'routes',
              'publishers'
            ]
          },
          'error-template': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'error-template',
            src: '.svelte-kit/generated/build/shared/error-template.js',
            isDynamicEntry: true
          },
          MessageComposer: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'MessageComposer',
            imports: [
              'public-key',
              'public-runtime',
              'references',
              'client',
              'Button',
              'navigation',
              'inbox-view',
              'publishers'
            ]
          },
          'preload-helper': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'preload-helper'
          },
          Disclosure: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'Disclosure',
            imports: ['client']
          },
          InboxStatus: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'InboxStatus',
            imports: [
              'public-runtime',
              'client',
              'Button',
              'navigation',
              'CapabilityGate',
              'InboxSetup',
              'inbox-view'
            ]
          },
          'inbox-view': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'inbox-view',
            imports: [
              'public-key',
              'public-runtime',
              'references',
              'Button',
              'read'
            ]
          },
          'public-key': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'public-key'
          },
          routes: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'routes',
            imports: ['public-key', 'references', 'Button']
          },
          Button: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'Button',
            imports: ['client']
          },
          ListingEditor: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'ListingEditor',
            imports: [
              'public-key',
              'public-runtime',
              'references',
              'client',
              'Button',
              'CapabilityGate',
              'read'
            ]
          },
          client: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client'
          },
          InboxSetup: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'InboxSetup',
            imports: ['public-runtime', 'client', 'Button']
          },
          legacy: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'legacy',
            imports: ['client']
          },
          navigation: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'navigation',
            imports: [
              'entry/payload',
              'shared-errors',
              'client',
              'client.svelte'
            ]
          },
          'shared-errors': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'shared-errors'
          },
          AccountGate: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'AccountGate',
            imports: [
              'public-runtime',
              'client',
              'CapabilityGate',
              'InboxSetup'
            ]
          },
          read: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'read',
            imports: ['public-runtime']
          },
          'client.svelte': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client.svelte',
            imports: ['entry/payload', 'shared-errors', 'client']
          },
          publishers: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'publishers',
            imports: ['public-key', 'public-runtime', 'Button', 'read']
          },
          CapabilityGate: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'CapabilityGate',
            imports: ['client', 'Button']
          },
          'public-runtime': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'public-runtime',
            imports: ['public-key', 'preload-helper', 'Button'],
            dynamicImports: ['negentropy']
          },
          SearchForm: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'SearchForm',
            imports: ['client', 'Button']
          },
          state: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'state',
            imports: ['client.svelte']
          },
          references: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'references',
            imports: ['public-key']
          },
          'client-entry': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client-entry',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/client-entry.js',
            isDynamicEntry: true,
            imports: ['navigation']
          },
          'entry/start': {
            file: '_app/immutable/entry/start.HASH.js',
            name: 'entry/start',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/entry.js',
            isEntry: true,
            imports: ['entry/payload', 'preload-helper'],
            dynamicImports: ['client-entry']
          },
          'entry/payload': {
            file: '_app/immutable/entry/payload.HASH.js',
            name: 'entry/payload',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/payload.js',
            isEntry: true
          },
          negentropy: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'negentropy',
            src: 'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js',
            isDynamicEntry: true,
            imports: ['public-runtime']
          }
        }
      : {
          'entry/app': {
            file: '_app/immutable/entry/app.HASH.js',
            name: 'entry/app',
            src: '.svelte-kit/generated/build/client-optimized/app.js',
            isEntry: true,
            imports: [
              'preload-helper',
              'shared-errors',
              'private-handles',
              'references'
            ],
            dynamicImports: [
              'nodes/0',
              'nodes/1',
              'nodes/2',
              'nodes/3',
              'nodes/4',
              'nodes/5',
              'nodes/6',
              'nodes/7',
              'nodes/8',
              'nodes/9',
              'nodes/10',
              'nodes/11',
              'nodes/12',
              'error-template'
            ]
          },
          'nodes/0': {
            file: '_app/immutable/nodes/0.HASH.js',
            name: 'nodes/0',
            src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
            isEntry: true,
            imports: [
              'public-key',
              'view-context',
              'public-runtime',
              'client',
              'Button',
              'client.svelte',
              'state',
              'CapabilityGate',
              'Disclosure'
            ],
            css: ['_app/immutable/assets/0.HASH.css']
          },
          'nodes/1': {
            file: '_app/immutable/nodes/1.HASH.js',
            name: 'nodes/1',
            src: '.svelte-kit/generated/build/client-optimized/nodes/1.js',
            isEntry: true,
            imports: ['client', 'client.svelte', 'state', 'legacy']
          },
          'nodes/10': {
            file: '_app/immutable/nodes/10.HASH.js',
            name: 'nodes/10',
            src: '.svelte-kit/generated/build/client-optimized/nodes/10.js',
            isEntry: true,
            imports: ['client', 'legacy', 'ListingEditor']
          },
          'nodes/11': {
            file: '_app/immutable/nodes/11.HASH.js',
            name: 'nodes/11',
            src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
            isEntry: true,
            imports: ['client', 'legacy', 'AccountGate']
          },
          'nodes/12': {
            file: '_app/immutable/nodes/12.HASH.js',
            name: 'nodes/12',
            src: '.svelte-kit/generated/build/client-optimized/nodes/12.js',
            isEntry: true,
            imports: [
              'public-key',
              'client',
              'client.svelte',
              'state',
              'ListingEditor'
            ]
          },
          'nodes/2': {
            file: '_app/immutable/nodes/2.HASH.js',
            name: 'nodes/2',
            src: '.svelte-kit/generated/build/client-optimized/nodes/2.js',
            isEntry: true,
            imports: ['client', 'Button', 'routes']
          },
          'nodes/3': {
            file: '_app/immutable/nodes/3.HASH.js',
            name: 'nodes/3',
            src: '.svelte-kit/generated/build/client-optimized/nodes/3.js',
            isEntry: true,
            imports: ['client', 'legacy']
          },
          'nodes/4': {
            file: '_app/immutable/nodes/4.HASH.js',
            name: 'nodes/4',
            src: '.svelte-kit/generated/build/client-optimized/nodes/4.js',
            isEntry: true,
            imports: ['client', 'AccountGate', 'InboxStatus']
          },
          'nodes/5': {
            file: '_app/immutable/nodes/5.HASH.js',
            name: 'nodes/5',
            src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
            isEntry: true,
            imports: [
              'public-key',
              'view-context',
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'state',
              'legacy',
              'inbox-view',
              'InboxStatus',
              'publishers',
              'MessageComposer'
            ]
          },
          'nodes/6': {
            file: '_app/immutable/nodes/6.HASH.js',
            name: 'nodes/6',
            src: '.svelte-kit/generated/build/client-optimized/nodes/6.js',
            isEntry: true,
            imports: ['client', 'legacy']
          },
          'nodes/7': {
            file: '_app/immutable/nodes/7.HASH.js',
            name: 'nodes/7',
            src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
            isEntry: true,
            imports: [
              'public-key',
              'view-context',
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'state',
              'CapabilityGate',
              'Disclosure',
              'InboxSetup',
              'MessageComposer'
            ]
          },
          'nodes/8': {
            file: '_app/immutable/nodes/8.HASH.js',
            name: 'nodes/8',
            src: '.svelte-kit/generated/build/client-optimized/nodes/8.js',
            isEntry: true,
            imports: [
              'public-key',
              'client',
              'client.svelte',
              'state',
              'ListingEditor'
            ]
          },
          'nodes/9': {
            file: '_app/immutable/nodes/9.HASH.js',
            name: 'nodes/9',
            src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
            isEntry: true,
            imports: [
              'view-context',
              'public-runtime',
              'references',
              'client',
              'Button',
              'client.svelte',
              'navigation',
              'state',
              'Disclosure',
              'routes',
              'publishers'
            ]
          },
          'error-template': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'error-template',
            src: '.svelte-kit/generated/build/shared/error-template.js',
            isDynamicEntry: true
          },
          'inbox-view': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'inbox-view',
            imports: [
              'public-key',
              'private-handles',
              'view-context',
              'public-runtime',
              'references',
              'Button',
              'read'
            ]
          },
          'private-handles': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'private-handles'
          },
          routes: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'routes',
            imports: ['references', 'client', 'Button']
          },
          InboxSetup: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'InboxSetup',
            imports: ['view-context', 'public-runtime', 'client', 'Button']
          },
          'preload-helper': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'preload-helper'
          },
          Disclosure: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'Disclosure',
            imports: ['client']
          },
          AccountGate: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'AccountGate',
            imports: ['view-context', 'client', 'CapabilityGate', 'InboxSetup']
          },
          publishers: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'publishers',
            imports: [
              'public-key',
              'view-context',
              'public-runtime',
              'Button',
              'read'
            ]
          },
          InboxStatus: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'InboxStatus',
            imports: [
              'view-context',
              'public-runtime',
              'client',
              'Button',
              'navigation',
              'CapabilityGate',
              'InboxSetup',
              'inbox-view'
            ]
          },
          Button: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'Button',
            imports: ['client']
          },
          references: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'references',
            imports: ['public-key']
          },
          client: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client'
          },
          legacy: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'legacy',
            imports: ['client']
          },
          ListingEditor: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'ListingEditor',
            imports: [
              'private-handles',
              'view-context',
              'references',
              'client',
              'Button',
              'CapabilityGate',
              'read'
            ]
          },
          'public-key': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'public-key'
          },
          navigation: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'navigation',
            imports: [
              'entry/payload',
              'shared-errors',
              'client',
              'client.svelte'
            ]
          },
          'public-runtime': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'public-runtime',
            imports: [
              'public-key',
              'preload-helper',
              'private-handles',
              'view-context',
              'Button'
            ],
            dynamicImports: ['negentropy']
          },
          'shared-errors': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'shared-errors'
          },
          'client.svelte': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client.svelte',
            imports: ['entry/payload', 'shared-errors', 'client']
          },
          MessageComposer: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'MessageComposer',
            imports: [
              'private-handles',
              'view-context',
              'public-runtime',
              'references',
              'client',
              'Button',
              'navigation',
              'inbox-view',
              'publishers'
            ]
          },
          'view-context': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'view-context',
            imports: ['public-key', 'private-handles', 'Button']
          },
          CapabilityGate: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'CapabilityGate',
            imports: ['client', 'Button']
          },
          read: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'read',
            imports: ['view-context']
          },
          state: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'state',
            imports: ['client.svelte']
          },
          'client-entry': {
            file: '_app/immutable/chunks/HASH.js',
            name: 'client-entry',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/client-entry.js',
            isDynamicEntry: true,
            imports: ['navigation']
          },
          'entry/start': {
            file: '_app/immutable/entry/start.HASH.js',
            name: 'entry/start',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/entry.js',
            isEntry: true,
            imports: ['entry/payload', 'preload-helper'],
            dynamicImports: ['client-entry']
          },
          'entry/payload': {
            file: '_app/immutable/entry/payload.HASH.js',
            name: 'entry/payload',
            src: 'node_modules/.pnpm/@sveltejs+kit@3.0.0_@opentelemetry+api@1.9.1_@sveltejs+vite-plugin-svelte@7.3.1_svelte@_6edc02734bfa45cbf85e92e62b52fb36/node_modules/@sveltejs/kit/src/runtime/client/payload.js',
            isEntry: true
          },
          negentropy: {
            file: '_app/immutable/chunks/HASH.js',
            name: 'negentropy',
            src: 'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js',
            isDynamicEntry: true,
            imports: ['view-context', 'public-runtime']
          }
        };
    if (hasListingReview)
      expected.ListingEditor = {
        file: '_app/immutable/chunks/HASH.js',
        name: 'ListingEditor',
        imports: [
          'public-key',
          'public-runtime',
          'references',
          'client',
          'Button',
          'CapabilityGate',
          'InboxSetup',
          'read'
        ]
      };
    if (hasListingEdit)
      Object.assign(expected, {
        'nodes/5': {
          file: '_app/immutable/nodes/5.HASH.js',
          name: 'nodes/5',
          src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
          isEntry: true,
          imports: [
            'public-key',
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'state',
            'legacy',
            'inbox-view',
            'InboxStatus',
            'resolve-head',
            'MessageComposer'
          ]
        },
        'nodes/9': {
          file: '_app/immutable/nodes/9.HASH.js',
          name: 'nodes/9',
          src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
          isEntry: true,
          imports: [
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'Disclosure',
            'SearchForm',
            'routes',
            'resolve-head',
            'publishers'
          ]
        },
        'resolve-head': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'resolve-head',
          imports: ['public-runtime', 'Button', 'read']
        },
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'public-key',
            'public-runtime',
            'references',
            'client',
            'Button',
            'CapabilityGate',
            'InboxSetup',
            'read',
            'resolve-head'
          ]
        },
        MessageComposer: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'MessageComposer',
          imports: [
            'public-key',
            'public-runtime',
            'references',
            'client',
            'Button',
            'navigation',
            'inbox-view',
            'resolve-head',
            'publishers'
          ]
        },
        publishers: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'publishers',
          imports: ['public-key', 'public-runtime', 'Button', 'resolve-head']
        }
      });
    if (hasListingRevision)
      Object.assign(expected, {
        'entry/app': {
          file: '_app/immutable/entry/app.HASH.js',
          name: 'entry/app',
          src: '.svelte-kit/generated/build/client-optimized/app.js',
          isEntry: true,
          imports: ['preload-helper', 'shared-errors', 'references'],
          dynamicImports: [
            'nodes/0',
            'nodes/1',
            'nodes/2',
            'nodes/3',
            'nodes/4',
            'nodes/5',
            'nodes/6',
            'nodes/7',
            'nodes/8',
            'nodes/9',
            'nodes/10',
            'nodes/11',
            'nodes/12',
            'error-template'
          ]
        },
        'nodes/0': {
          file: '_app/immutable/nodes/0.HASH.js',
          name: 'nodes/0',
          src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
          isEntry: true,
          imports: [
            'rolldown-runtime',
            'public-runtime',
            'client',
            'Button',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure'
          ],
          css: ['_app/immutable/assets/0.HASH.css']
        },
        'nodes/10': {
          file: '_app/immutable/nodes/10.HASH.js',
          name: 'nodes/10',
          src: '.svelte-kit/generated/build/client-optimized/nodes/10.js',
          isEntry: true,
          imports: [
            'references',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'legacy',
            'routes',
            'ListingEditor'
          ]
        },
        'nodes/12': {
          file: '_app/immutable/nodes/12.HASH.js',
          name: 'nodes/12',
          src: '.svelte-kit/generated/build/client-optimized/nodes/12.js',
          isEntry: true,
          imports: [
            'rolldown-runtime',
            'client',
            'client.svelte',
            'navigation',
            'state',
            'ListingEditor'
          ]
        },
        'nodes/5': {
          file: '_app/immutable/nodes/5.HASH.js',
          name: 'nodes/5',
          src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
          isEntry: true,
          imports: [
            'rolldown-runtime',
            'references',
            'public-runtime',
            'client',
            'Button',
            'client.svelte',
            'state',
            'legacy',
            'inbox-view',
            'InboxStatus',
            'MessageComposer'
          ]
        },
        'nodes/7': {
          file: '_app/immutable/nodes/7.HASH.js',
          name: 'nodes/7',
          src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
          isEntry: true,
          imports: [
            'rolldown-runtime',
            'references',
            'public-runtime',
            'client',
            'Button',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'MessageComposer'
          ]
        },
        'nodes/8': {
          file: '_app/immutable/nodes/8.HASH.js',
          name: 'nodes/8',
          src: '.svelte-kit/generated/build/client-optimized/nodes/8.js',
          isEntry: true,
          imports: [
            'rolldown-runtime',
            'client',
            'client.svelte',
            'navigation',
            'state',
            'ListingEditor'
          ]
        },
        'nodes/9': {
          file: '_app/immutable/nodes/9.HASH.js',
          name: 'nodes/9',
          src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
          isEntry: true,
          imports: [
            'references',
            'public-runtime',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'Disclosure',
            'SearchForm',
            'routes',
            'publishers'
          ]
        },
        MessageComposer: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'MessageComposer',
          imports: [
            'references',
            'public-runtime',
            'client',
            'Button',
            'navigation',
            'inbox-view',
            'publishers'
          ]
        },
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'references',
            'public-runtime',
            'client',
            'Button',
            'CapabilityGate',
            'InboxSetup'
          ]
        },
        'public-runtime': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'public-runtime',
          imports: [
            'rolldown-runtime',
            'preload-helper',
            'references',
            'Button'
          ],
          dynamicImports: ['negentropy']
        },
        routes: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'routes',
          imports: ['references', 'Button']
        },
        'rolldown-runtime': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'rolldown-runtime'
        },
        references: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'references',
          imports: ['rolldown-runtime']
        },
        'inbox-view': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'inbox-view',
          imports: ['references', 'public-runtime', 'Button']
        },
        publishers: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'publishers',
          imports: ['references', 'public-runtime', 'Button']
        }
      });
    if (hasSellingWorkspace) {
      delete expected.AccountGate;
      Object.assign(expected, {
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'references',
            'public-runtime',
            'client',
            'Button',
            'CapabilityGate',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        OperationStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'OperationStatus',
          imports: ['references', 'public-runtime', 'client']
        },
        'nodes/11': {
          file: '_app/immutable/nodes/11.HASH.js',
          name: 'nodes/11',
          src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
          isEntry: true,
          imports: [
            'references',
            'public-runtime',
            'client',
            'Button',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        'nodes/4': {
          file: '_app/immutable/nodes/4.HASH.js',
          name: 'nodes/4',
          src: '.svelte-kit/generated/build/client-optimized/nodes/4.js',
          isEntry: true,
          imports: [
            'public-runtime',
            'client',
            'CapabilityGate',
            'InboxSetup',
            'InboxStatus'
          ]
        }
      });
    }
    if (hasPublicHiding) {
      delete expected['rolldown-runtime'];
      Object.assign(expected, {
        InboxSetup: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxSetup',
          imports: ['local-public-hides', 'public-runtime', 'client', 'Button']
        },
        InboxStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxStatus',
          imports: [
            'local-public-hides',
            'public-runtime',
            'client',
            'Button',
            'navigation',
            'CapabilityGate',
            'InboxSetup',
            'inbox-view'
          ]
        },
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'CapabilityGate',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        MessageComposer: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'MessageComposer',
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'navigation',
            'inbox-view',
            'publishers'
          ]
        },
        OperationStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'OperationStatus',
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'client'
          ]
        },
        'entry/app': {
          file: '_app/immutable/entry/app.HASH.js',
          name: 'entry/app',
          src: '.svelte-kit/generated/build/client-optimized/app.js',
          isEntry: true,
          imports: [
            'public-key',
            'preload-helper',
            'shared-errors',
            'references'
          ],
          dynamicImports: [
            'nodes/0',
            'nodes/1',
            'nodes/2',
            'nodes/3',
            'nodes/4',
            'nodes/5',
            'nodes/6',
            'nodes/7',
            'nodes/8',
            'nodes/9',
            'nodes/10',
            'nodes/11',
            'nodes/12',
            'error-template'
          ]
        },
        'inbox-view': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'inbox-view',
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'references',
            'Button'
          ]
        },
        'local-public-hides': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'local-public-hides',
          imports: ['public-key', 'Button']
        },
        negentropy: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'negentropy',
          src: 'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js',
          isDynamicEntry: true,
          imports: ['local-public-hides', 'public-runtime']
        },
        'nodes/0': {
          file: '_app/immutable/nodes/0.HASH.js',
          name: 'nodes/0',
          src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
          isEntry: true,
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'client',
            'Button',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure'
          ],
          css: ['_app/immutable/assets/0.HASH.css']
        },
        'nodes/10': {
          file: '_app/immutable/nodes/10.HASH.js',
          name: 'nodes/10',
          src: '.svelte-kit/generated/build/client-optimized/nodes/10.js',
          isEntry: true,
          imports: [
            'public-key',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'legacy',
            'routes',
            'ListingEditor'
          ]
        },
        'nodes/11': {
          file: '_app/immutable/nodes/11.HASH.js',
          name: 'nodes/11',
          src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
          isEntry: true,
          imports: [
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        'nodes/12': {
          file: '_app/immutable/nodes/12.HASH.js',
          name: 'nodes/12',
          src: '.svelte-kit/generated/build/client-optimized/nodes/12.js',
          isEntry: true,
          imports: [
            'public-key',
            'client',
            'client.svelte',
            'navigation',
            'state',
            'ListingEditor'
          ]
        },
        'nodes/4': {
          file: '_app/immutable/nodes/4.HASH.js',
          name: 'nodes/4',
          src: '.svelte-kit/generated/build/client-optimized/nodes/4.js',
          isEntry: true,
          imports: [
            'local-public-hides',
            'client',
            'CapabilityGate',
            'InboxSetup',
            'InboxStatus'
          ]
        },
        'nodes/5': {
          file: '_app/immutable/nodes/5.HASH.js',
          name: 'nodes/5',
          src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
          isEntry: true,
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'state',
            'legacy',
            'inbox-view',
            'InboxStatus',
            'MessageComposer'
          ]
        },
        'nodes/6': {
          file: '_app/immutable/nodes/6.HASH.js',
          name: 'nodes/6',
          src: '.svelte-kit/generated/build/client-optimized/nodes/6.js',
          isEntry: true,
          imports: ['local-public-hides', 'client', 'Button']
        },
        'nodes/7': {
          file: '_app/immutable/nodes/7.HASH.js',
          name: 'nodes/7',
          src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
          isEntry: true,
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'MessageComposer'
          ]
        },
        'nodes/8': {
          file: '_app/immutable/nodes/8.HASH.js',
          name: 'nodes/8',
          src: '.svelte-kit/generated/build/client-optimized/nodes/8.js',
          isEntry: true,
          imports: [
            'public-key',
            'client',
            'client.svelte',
            'navigation',
            'state',
            'ListingEditor'
          ]
        },
        'nodes/9': {
          file: '_app/immutable/nodes/9.HASH.js',
          name: 'nodes/9',
          src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
          isEntry: true,
          imports: [
            'local-public-hides',
            'public-runtime',
            'references',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'Disclosure',
            'SearchForm',
            'routes',
            'publishers'
          ]
        },
        'public-key': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'public-key'
        },
        'public-runtime': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'public-runtime',
          imports: [
            'public-key',
            'preload-helper',
            'local-public-hides',
            'references',
            'Button'
          ],
          dynamicImports: ['negentropy']
        },
        publishers: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'publishers',
          imports: [
            'public-key',
            'local-public-hides',
            'public-runtime',
            'Button'
          ]
        },
        references: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'references',
          imports: ['public-key']
        },
        routes: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'routes',
          imports: ['public-key', 'references', 'Button']
        }
      });
    }
    if (hasPrivateBlocking) {
      delete expected['local-public-hides'];
      Object.assign(expected, {
        InboxSetup: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxSetup',
          imports: ['private-session', 'client', 'Button', 'public-runtime']
        },
        InboxStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxStatus',
          imports: [
            'private-session',
            'client',
            'Button',
            'public-runtime',
            'navigation',
            'CapabilityGate',
            'InboxSetup',
            'inbox-view'
          ]
        },
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'CapabilityGate',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        MessageComposer: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'MessageComposer',
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'navigation',
            'local-private-blocks',
            'inbox-view',
            'publishers'
          ]
        },
        OperationStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'OperationStatus',
          imports: ['public-key', 'private-session', 'client', 'public-runtime']
        },
        'inbox-view': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'inbox-view',
          imports: [
            'public-key',
            'private-session',
            'references',
            'Button',
            'public-runtime',
            'local-private-blocks'
          ]
        },
        'local-private-blocks': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'local-private-blocks',
          imports: ['public-key', 'private-session', 'Button']
        },
        negentropy: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'negentropy',
          src: 'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js',
          isDynamicEntry: true,
          imports: ['private-session', 'public-runtime']
        },
        'nodes/0': {
          file: '_app/immutable/nodes/0.HASH.js',
          name: 'nodes/0',
          src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'client',
            'Button',
            'public-runtime',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure',
            'local-private-blocks'
          ],
          css: ['_app/immutable/assets/0.HASH.css']
        },
        'nodes/11': {
          file: '_app/immutable/nodes/11.HASH.js',
          name: 'nodes/11',
          src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
          isEntry: true,
          imports: [
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        'nodes/4': {
          file: '_app/immutable/nodes/4.HASH.js',
          name: 'nodes/4',
          src: '.svelte-kit/generated/build/client-optimized/nodes/4.js',
          isEntry: true,
          imports: [
            'private-session',
            'client',
            'CapabilityGate',
            'InboxSetup',
            'InboxStatus'
          ]
        },
        'nodes/5': {
          file: '_app/immutable/nodes/5.HASH.js',
          name: 'nodes/5',
          src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'client.svelte',
            'state',
            'legacy',
            'inbox-view',
            'InboxStatus',
            'MessageComposer'
          ]
        },
        'nodes/6': {
          file: '_app/immutable/nodes/6.HASH.js',
          name: 'nodes/6',
          src: '.svelte-kit/generated/build/client-optimized/nodes/6.js',
          isEntry: true,
          imports: [
            'private-session',
            'client',
            'Button',
            'local-private-blocks'
          ]
        },
        'nodes/7': {
          file: '_app/immutable/nodes/7.HASH.js',
          name: 'nodes/7',
          src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'client.svelte',
            'state',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'MessageComposer'
          ]
        },
        'nodes/9': {
          file: '_app/immutable/nodes/9.HASH.js',
          name: 'nodes/9',
          src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
          isEntry: true,
          imports: [
            'private-session',
            'references',
            'client',
            'Button',
            'public-runtime',
            'client.svelte',
            'navigation',
            'state',
            'Disclosure',
            'SearchForm',
            'routes',
            'publishers'
          ]
        },
        'private-session': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'private-session',
          imports: ['public-key', 'Button']
        },
        'public-runtime': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'public-runtime',
          imports: [
            'public-key',
            'preload-helper',
            'private-session',
            'references',
            'Button'
          ],
          dynamicImports: ['negentropy']
        },
        publishers: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'publishers',
          imports: ['public-key', 'private-session', 'Button', 'public-runtime']
        }
      });
    }
    if (hasSupportReporting)
      Object.assign(expected, {
        Button: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'Button',
          imports: ['client', 'navigation-url']
        },
        CapabilityGate: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'CapabilityGate',
          imports: ['client', 'navigation-url', 'Button']
        },
        InboxSetup: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxSetup',
          imports: ['private-session', 'client', 'public-runtime', 'Button']
        },
        InboxStatus: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'InboxStatus',
          imports: [
            'private-session',
            'client',
            'navigation-url',
            'public-runtime',
            'navigation',
            'Button',
            'CapabilityGate',
            'inbox-view',
            'InboxSetup'
          ]
        },
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'CapabilityGate',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        MessageComposer: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'MessageComposer',
          imports: [
            'public-key',
            'private-session',
            'client',
            'navigation-url',
            'public-runtime',
            'navigation',
            'local-private-blocks',
            'ReportPreview',
            'inbox-view'
          ]
        },
        ReportPreview: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ReportPreview',
          imports: [
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'publishers',
            'inbox-view'
          ]
        },
        'inbox-view': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'inbox-view',
          imports: [
            'public-key',
            'private-session',
            'references',
            'navigation-url',
            'public-runtime',
            'local-private-blocks'
          ]
        },
        'local-private-blocks': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'local-private-blocks',
          imports: ['public-key', 'private-session', 'navigation-url']
        },
        'navigation-url': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'navigation-url'
        },
        'nodes/0': {
          file: '_app/immutable/nodes/0.HASH.js',
          name: 'nodes/0',
          src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'client',
            'navigation-url',
            'public-runtime',
            'client.svelte',
            'state',
            'Button',
            'CapabilityGate',
            'Disclosure',
            'local-private-blocks'
          ],
          css: ['_app/immutable/assets/0.HASH.css']
        },
        'nodes/10': {
          file: '_app/immutable/nodes/10.HASH.js',
          name: 'nodes/10',
          src: '.svelte-kit/generated/build/client-optimized/nodes/10.js',
          isEntry: true,
          imports: [
            'public-key',
            'client',
            'navigation-url',
            'client.svelte',
            'navigation',
            'state',
            'legacy',
            'routes',
            'ListingEditor'
          ]
        },
        'nodes/11': {
          file: '_app/immutable/nodes/11.HASH.js',
          name: 'nodes/11',
          src: '.svelte-kit/generated/build/client-optimized/nodes/11.js',
          isEntry: true,
          imports: [
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'CapabilityGate',
            'Disclosure',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        'nodes/2': {
          file: '_app/immutable/nodes/2.HASH.js',
          name: 'nodes/2',
          src: '.svelte-kit/generated/build/client-optimized/nodes/2.js',
          isEntry: true,
          imports: ['client', 'navigation-url', 'SearchForm', 'routes']
        },
        'nodes/3': {
          file: '_app/immutable/nodes/3.HASH.js',
          name: 'nodes/3',
          src: '.svelte-kit/generated/build/client-optimized/nodes/3.js',
          isEntry: true,
          imports: [
            'private-session',
            'client',
            'public-runtime',
            'ReportPreview'
          ]
        },
        'nodes/5': {
          file: '_app/immutable/nodes/5.HASH.js',
          name: 'nodes/5',
          src: '.svelte-kit/generated/build/client-optimized/nodes/5.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'client.svelte',
            'state',
            'Button',
            'legacy',
            'ReportPreview',
            'inbox-view',
            'InboxStatus',
            'MessageComposer'
          ]
        },
        'nodes/7': {
          file: '_app/immutable/nodes/7.HASH.js',
          name: 'nodes/7',
          src: '.svelte-kit/generated/build/client-optimized/nodes/7.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'client.svelte',
            'state',
            'Button',
            'CapabilityGate',
            'Disclosure',
            'ReportPreview',
            'InboxSetup',
            'MessageComposer'
          ]
        },
        'nodes/9': {
          file: '_app/immutable/nodes/9.HASH.js',
          name: 'nodes/9',
          src: '.svelte-kit/generated/build/client-optimized/nodes/9.js',
          isEntry: true,
          imports: [
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'client.svelte',
            'navigation',
            'state',
            'Button',
            'Disclosure',
            'SearchForm',
            'routes',
            'publishers'
          ]
        },
        'private-session': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'private-session',
          imports: ['public-key', 'navigation-url']
        },
        'public-runtime': {
          file: '_app/immutable/chunks/HASH.js',
          name: 'public-runtime',
          imports: [
            'public-key',
            'preload-helper',
            'private-session',
            'references',
            'navigation-url'
          ],
          dynamicImports: ['negentropy']
        },
        publishers: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'publishers',
          imports: [
            'public-key',
            'private-session',
            'navigation-url',
            'public-runtime'
          ]
        },
        routes: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'routes',
          imports: ['public-key', 'references', 'navigation-url']
        }
      });
    if (hasPrivacyData)
      Object.assign(expected, {
        ReportPreview: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ReportPreview',
          imports: [
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'SupportLink',
            'publishers',
            'inbox-view'
          ]
        },
        SupportLink: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'SupportLink',
          imports: ['private-session', 'client', 'navigation-url']
        },
        'nodes/3': {
          file: '_app/immutable/nodes/3.HASH.js',
          name: 'nodes/3',
          src: '.svelte-kit/generated/build/client-optimized/nodes/3.js',
          isEntry: true,
          imports: [
            'private-session',
            'client',
            'public-runtime',
            'SupportLink',
            'ReportPreview'
          ]
        },
        'nodes/6': {
          file: '_app/immutable/nodes/6.HASH.js',
          name: 'nodes/6',
          src: '.svelte-kit/generated/build/client-optimized/nodes/6.js',
          isEntry: true,
          imports: [
            ...(hasOperationalFlags ? ['public-key'] : []),
            'private-session',
            'client',
            'navigation-url',
            'Button',
            'local-private-blocks',
            'SupportLink'
          ]
        }
      });
    if (hasOperationalFlags)
      Object.assign(expected, {
        ListingEditor: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'ListingEditor',
          imports: [
            'public-key',
            'private-session',
            'references',
            'client',
            'navigation-url',
            'public-runtime',
            'CapabilityGate',
            'capabilities',
            'InboxSetup',
            'OperationStatus'
          ]
        },
        capabilities: {
          file: '_app/immutable/chunks/HASH.js',
          name: 'capabilities',
          imports: ['public-runtime']
        },
        'nodes/0': {
          file: '_app/immutable/nodes/0.HASH.js',
          name: 'nodes/0',
          src: '.svelte-kit/generated/build/client-optimized/nodes/0.js',
          isEntry: true,
          imports: [
            'public-key',
            'private-session',
            'client',
            'navigation-url',
            'public-runtime',
            'client.svelte',
            'state',
            'Button',
            'CapabilityGate',
            'Disclosure',
            'capabilities',
            'local-private-blocks'
          ],
          css: ['_app/immutable/assets/0.HASH.css']
        }
      });
    for (const record of Object.values(manifest)) {
      const actual = { ...record };
      actual.file = actual.file.includes('/chunks/')
        ? actual.file.replace(/(?<=\/chunks\/)[A-Za-z0-9_-]+(?=\.js$)/, 'HASH')
        : actual.file.replace(/\.[A-Za-z0-9_-]+\.js$/, '.HASH.js');
      for (const field of ['imports', 'dynamicImports'])
        if (actual[field] !== undefined)
          actual[field] = actual[field].map(
            (/** @type {string} */ key) => manifest[key].name
          );
      if (actual.css !== undefined)
        actual.css = actual.css.map((/** @type {string} */ file) =>
          file.replace(/\.[A-Za-z0-9_-]+\.css$/, '.HASH.css')
        );
      equal(
        actual,
        expected[record.name],
        'Invalid owned listing editor compiler graph'
      );
    }
  }
  if (hasFullRoutes) {
    const robots = await readOwned(web, 'static/robots.txt');
    publicText(robots);
    equal(
      client.get('robots.txt'),
      robots,
      'Robots differs from owned static source'
    );
    admitted.set('robots.txt', robots);
  }
  const versionBytes = client.get('_app/version.json');
  if (!versionBytes) throw new Error('Missing compiler version');
  const version = parse(publicText(versionBytes));
  equal(Object.keys(version), ['version']);
  if (
    typeof version.version !== 'string' ||
    !/^[A-Za-z0-9._-]{1,128}$/.test(version.version)
  )
    throw new Error('Invalid compiler version');
  admitted.set('_app/version.json', versionBytes);
  equal(
    [...client.keys()].sort(),
    [...admitted.keys(), '.vite/manifest.json'].sort(),
    'Compiler inventory mismatch'
  );
  equal(
    client.get('build-info.json'),
    expectedMetadata,
    'Compiler provenance mismatch'
  );
  const expectedPages = hasFullRoutes
    ? [
        'about.html',
        'index.html',
        'messages.html',
        'privacy.html',
        'search.html',
        'sell.html',
        'selling.html'
      ]
    : hasSearch
      ? ['index.html', 'search.html']
      : ['index.html'];
  equal(
    [...pages.keys()].sort(),
    expectedPages,
    'Undeclared prerendered pages'
  );
  for (const name of expectedPages) admitted.set(name, pages.get(name));
  // Adapter fallback has no separate disk input. Reconstruct the exact current
  // compiler bootstrap shell from the actual index/manifest/version and source
  // template; future framework/resource shapes require scoped tested admission.
  const index = publicText(pages.get('index.html'));
  const id = /(__sveltekit_[a-z0-9]+) = \{/.exec(index)?.[1];
  if (!id) throw new Error('Missing compiler bootstrap identity');
  const start = Object.values(manifest).find(
    (record) => record.name === 'entry/start'
  ).file;
  const app = Object.values(manifest).find(
    (record) => record.name === 'entry/app'
  ).file;
  const fallbackModules = new Set();
  const moduleVisited = new Set();
  /** @param {string} key */
  function collectModules(key) {
    if (moduleVisited.has(key)) return;
    moduleVisited.add(key);
    const record = manifest[key];
    fallbackModules.add(record.file);
    for (const dependency of record.imports ?? []) collectModules(dependency);
  }
  for (const [key, record] of Object.entries(manifest))
    if (
      ['entry/start', 'entry/app', 'nodes/0', 'client-entry'].includes(
        record.name
      )
    )
      collectModules(key);
  const preload = [
    ...index.matchAll(/<link href="\.\/([^"<>]+)" rel="modulepreload">/g)
  ]
    .map((match) => match[1])
    .filter((file) => fallbackModules.has(file));
  if (!preload.length || preload.some((file) => !admitted.has(file)))
    throw new Error('Invalid compiler preloads');
  const fallbackCSS = new Set();
  const cssVisited = new Set();
  /** @param {string} key */
  function collectCSS(key) {
    if (cssVisited.has(key)) return;
    cssVisited.add(key);
    const record = manifest[key];
    for (const file of record.css ?? []) fallbackCSS.add(file);
    for (const dependency of record.imports ?? []) collectCSS(dependency);
  }
  for (const [key, record] of Object.entries(manifest))
    if (['entry/start', 'entry/app', 'nodes/0'].includes(record.name))
      collectCSS(key);
  const stylesheetLinks = [
    ...index.matchAll(/<link href="\.\/([^"<>]+)" rel="stylesheet">/g)
  ]
    .map((match) => match[1])
    .filter((file) => fallbackCSS.has(file));
  equal(
    [...stylesheetLinks].sort(),
    [...fallbackCSS].sort(),
    'Missing owned fallback stylesheet'
  );
  const head =
    preload
      .map((file) => `<link href="/${file}" rel="modulepreload">`)
      .join('\n\t\t') +
    '\n\t\t' +
    (stylesheetLinks.length ? '\n\t\t' : '') +
    stylesheetLinks
      .map((file) => `<link href="/${file}" rel="stylesheet">`)
      .join('\n\t\t');
  const body = `\n\t\t\t<script>\n\t\t\t\t{\n\t\t\t\t\t${id} = {\n\t\t\t\t\t\tbase: "",\n\t\t\t\t\t\tversion: ${JSON.stringify(version.version)}\n\t\t\t\t\t};\n\n\t\t\t\t\tconst element = document.currentScript.parentElement;\n\n\t\t\t\t\timport("/${start}").then(async (kit) => {\n\t\t\t\t\t\tkit.init(${id});\n\t\t\t\t\t\tconst app = await import("/${app}");\n\t\t\t\t\t\tkit.start(app, element);\n\t\t\t\t\t});\n\t\t\t\t}\n\t\t\t</script>\n\t\t`;
  const template = publicText(await readOwned(web, 'src/app.html'));
  admitted.set(
    '200.html',
    Buffer.from(
      template
        .replace('%sveltekit.head%', head)
        .replace('%sveltekit.body%', body)
    )
  );
  equal(
    [...output.keys()].sort(),
    [...admitted.keys()].sort(),
    'Static output inventory mismatch'
  );
  for (const [file, expected] of admitted) {
    const bytes = output.get(file);
    publicText(bytes);
    equal(bytes, expected, 'Static output differs from owned compiler input');
  }
  return [...output.keys()].sort();
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const web = fileURLToPath(new URL('../', import.meta.url));
  if ((await realpath(process.cwd())) !== (await realpath(web)))
    throw new Error('Run the output audit from web/');
  const files = await auditOutput(web);
  console.log(`Static output boundary: ${files.length} owned files checked`);
}
