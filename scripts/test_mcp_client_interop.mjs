import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const [endpoint, clientDir, outputDir] = process.argv.slice(2);
assert.equal(new URL(endpoint).hostname, '127.0.0.1');
const root = resolve(clientDir);
const output = resolve(outputDir);
mkdirSync(output, { recursive: true });
const mcpc = join(root, 'node_modules/@apify/mcpc/bin/mcpc');
const mcporter = join(root, 'node_modules/mcporter/dist/cli.js');
const nativeCli = process.env.DCC_MCP_INTEROP_CLI;
assert.ok(nativeCli, 'set DCC_MCP_INTEROP_CLI to the candidate native CLI binary');
assert.equal(JSON.parse(readFileSync(join(root, 'node_modules/@apify/mcpc/package.json'))).version, '0.7.0');
assert.equal(JSON.parse(readFileSync(join(root, 'node_modules/mcporter/package.json'))).version, '0.14.2');
const environment = { ...process.env, MCPC_HOME_DIR: join(output, 'mcpc-home') };
if (process.platform === 'win32') {
  const preload = join(dirname(fileURLToPath(import.meta.url)), 'mcp_interop_windows_hide.cjs');
  environment.NODE_OPTIONS = [process.env.NODE_OPTIONS, '--require=' + preload].filter(Boolean).join(' ');
}
const records = [];
function run(program, arguments_, label, succeeds = true, useNative = true) {
  const throughNativeCli = program === mcpc && useNative;
  const forwarded = arguments_[0] === '--json' ? arguments_.slice(1) : arguments_;
  const executable = throughNativeCli ? nativeCli : process.execPath;
  const argv = throughNativeCli
    ? ['--output', 'json', 'mcp', '--client-entry', mcpc, '--state-dir', environment.MCPC_HOME_DIR, '--', ...forwarded]
    : [program, ...arguments_];
  const result = spawnSync(executable, argv, {
    env: environment, cwd: output, encoding: 'utf8', timeout: 45000, windowsHide: true,
  });
  const record = { label, executable, arguments: argv, throughNativeCli, status: result.status, stdout: result.stdout, stderr: result.stderr, error: result.error?.message };
  records.push(record);
  writeFileSync(join(output, 'commands.json'), JSON.stringify(records, null, 2));
  assert.equal(result.status === 0, succeeds, JSON.stringify(record));
  return result;
}
const session = '@core-fixture';
try {
  const connected = JSON.parse(run(mcpc, ['--json', 'connect', endpoint, session, '--no-profile', '--protocol-version', '2025-03-26'], 'mcpc-connect').stdout);
  assert.ok(JSON.stringify(connected).includes('Production reuse gate'));
  const info = JSON.parse(run(mcpc, ['--json', session], 'mcpc-overview').stdout);
  assert.ok(JSON.stringify(info).includes('Production reuse gate'));
  const list = JSON.parse(run(mcpc, ['--json', session, 'resources-list'], 'mcpc-resource-list').stdout);
  assert.ok(JSON.stringify(list).includes('gateway://docs/agent-workflows'));
  const guideResult = JSON.parse(run(mcpc, ['--json', session, 'resources-read', 'gateway://docs/agent-workflows'], 'mcpc-guide-json').stdout);
  const guide = JSON.parse(guideResult.contents[0].text);
  assert.equal(guide.production_reuse_contract, 'dcc-mcp-production-reuse/v1');
  assert.ok(guide.document.includes('## Production reuse gate'));
  assert.ok(guide.document.includes('search_blender_extensions'));
  const raw = run(mcpc, [session, 'resources-read', 'gateway://docs/agent-workflows', '--raw'], 'mcpc-guide-raw', true, false).stdout;
  assert.equal(JSON.parse(raw).document, guide.document);
  const prompts = JSON.parse(run(mcpc, ['--json', session, 'prompts-list'], 'mcpc-prompts-list').stdout);
  assert.ok(Array.isArray(prompts), JSON.stringify(prompts));
  const prompt = prompts.find(item => item.name.endsWith('__tree_U_recipe'));
  assert.ok(prompt, JSON.stringify(prompts));
  const arguments_ = { seed: '42', label: 'tree + canal & test' };
  const rendered = JSON.parse(run(mcpc, ['--json', session, 'prompts-get', prompt.name, JSON.stringify(arguments_)], 'mcpc-prompts-get').stdout);
  assert.deepEqual(JSON.parse(rendered.messages[0].content.text), arguments_);
  const unsupported = run(mcpc, ['--json', session, 'resources-templates-list'], 'mcpc-unsupported-templates', false);
  assert.match(unsupported.stderr + unsupported.stdout, /-32601|Method not found/);
  const badPrompt = run(mcpc, ['--json', session, 'prompts-get', 'missing'], 'mcpc-missing-prompt', false);
  assert.match(badPrompt.stderr + badPrompt.stdout, /prefix|routing|prompt|-32602/i);
  const config = join(output, 'mcporter.json');
  writeFileSync(config, JSON.stringify({ imports: [], mcpServers: { dcc: { url: endpoint } } }));
  const porterList = run(mcporter, ['--config', config, 'list', 'dcc', '--json', '--no-oauth'], 'mcporter-list').stdout;
  assert.ok(porterList.includes('Production reuse gate'));
  const porterGuide = run(mcporter, ['--config', config, 'resource', 'dcc', 'gateway://docs/agent-workflows', '--output', 'raw', '--no-oauth'], 'mcporter-resource').stdout;
  assert.ok(porterGuide.includes('dcc-mcp-production-reuse/v1'));
  const oldSession = '@old-fixture';
  try {
    run(mcpc, ['--json', 'connect', endpoint.replace('/mcp', '/old/mcp'), oldSession, '--no-profile', '--protocol-version', '2025-03-26'], 'mcpc-old-connect');
    const oldResource = JSON.parse(run(mcpc, ['--json', oldSession, 'resources-read', 'gateway://docs/agent-workflows'], 'mcpc-old-readable-guide').stdout);
    const oldGuide = JSON.parse(oldResource.contents[0].text);
    assert.notEqual(oldGuide.production_reuse_contract, 'dcc-mcp-production-reuse/v1');
    assert.ok(!oldGuide.document.includes('## Production reuse gate'));
    const absentCapability = run(mcpc, ['--json', oldSession, 'prompts-list'], 'mcpc-old-missing-prompts', false);
    assert.match(absentCapability.stderr + absentCapability.stdout, /MCP_CAPABILITY_UNSUPPORTED/);
    // Preserve the upstream behavior as a limitation, not a successful capability check.
    const bareClient = run(mcpc, ['--json', oldSession, 'prompts-list'], 'bare-mcpc-old-empty-prompts', true, false);
    assert.deepEqual(JSON.parse(bareClient.stdout), []);
  } finally {
    run(mcpc, ['--json', 'close', oldSession], 'mcpc-old-close');
  }
  writeFileSync(join(output, 'summary.json'), JSON.stringify({ versions: { mcpc: '0.7.0', mcporter: '0.14.2' }, endpoint, legacyProtocol: '2025-03-26', guideContract: guide.production_reuse_contract, promptArguments: arguments_, templatesSupported: false, checks: records.map(record => record.label) }, null, 2));
  console.log('External MCP clients consumed Core instructions, the versioned guide and parameterized prompt; unsupported templates remained an error.');
} finally {
  // Release only the task-owned bridge/session, including on failed assertions.
  run(mcpc, ['--json', 'close', session], 'mcpc-close');
}
