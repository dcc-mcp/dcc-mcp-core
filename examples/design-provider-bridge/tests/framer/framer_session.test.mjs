/** Host-free contract tests. Injected SDK fixtures are not native Framer evidence. */
import assert from "node:assert/strict";
import test from "node:test";
import { Readable, Writable } from "node:stream";
import { FramerSdkSession } from "../../framer_session.mjs";
import { PROTOCOL, runFramerFacade } from "../../framer_facade.mjs";

const TEST_KEY = "fixture-key-not-a-credential";
const PROJECT = "fixture-project";
const wait = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function fixture(api) {
  const connections = [];
  const session = await FramerSdkSession.open({
    project: PROJECT,
    apiKey: TEST_KEY,
    loadSdk: async () => ({
      connect: async (...args) => { connections.push(args); return api; },
    }),
  });
  return { session, connections };
}

async function protocolFixture(requests, api, options = {}) {
  let output = "";
  const exitCode = await runFramerFacade({
    input: Readable.from(requests.map((request) => `${typeof request === "string" ? request : JSON.stringify(request)}\n`)),
    output: new Writable({ write(chunk, _encoding, done) { output += chunk.toString(); done(); } }),
    argv: ["--project", PROJECT],
    env: { FRAMER_API_KEY: TEST_KEY },
    loadSdk: async () => ({ connect: async () => api }),
    ...options,
  });
  return { exitCode, output, responses: output.trim().split("\n").map((line) => JSON.parse(line)) };
}

test("one official connect, dynamic method inventory, and exactly one explicit disconnect", async () => {
  let disconnects = 0;
  const api = {
    getProjectInfo() { return { id: PROJECT, name: "Editable fixture" }; },
    publish() { throw new Error("Must never be called"); },
    deploy() { throw new Error("Must never be called"); },
    disconnect() { disconnects += 1; },
  };
  const { session, connections } = await fixture(api);
  assert.deepEqual(connections, [[PROJECT, TEST_KEY]]);
  assert.deepEqual(session.listTools().map((tool) => tool.name), ["getProjectInfo"]);
  assert.equal(session.listTools()[0].annotations.readOnlyHint, true);
  const tool = session.listTools()[0];
  tool.inputSchema.type = "changed-by-client";
  assert.equal(session.listTools()[0].inputSchema.type, "object");
  assert.deepEqual(await session.callTool("getProjectInfo"), { id: PROJECT, name: "Editable fixture" });
  await assert.rejects(session.callTool("publish"), { code: "tool_unavailable" });
  await assert.rejects(session.callTool("deploy"), { code: "tool_unavailable" });
  await session.close();
  await session.close();
  assert.equal(disconnects, 1);
  assert.throws(() => session.listTools(), { code: "session_closed" });
});

test("forward official argument order without rewriting attributes or array results", async () => {
  const calls = [];
  const attributes = { name: "Editable Card", width: "320px", height: "200px", backgroundColor: "#102030" };
  const api = {
    getCanvasRoot() { return { id: "root", type: "FrameNode" }; },
    getNode(...args) { calls.push(["getNode", ...args]); return null; },
    getChildren(...args) { calls.push(["getChildren", ...args]); return [{ id: "child", name: "Button" }]; },
    createFrameNode(...args) { calls.push(["createFrameNode", ...args]); return { id: "created", ...args[0] }; },
    disconnect() {},
  };
  const { session } = await fixture(api);
  assert.equal(await session.callTool("getNode", { nodeId: "missing" }), null);
  assert.deepEqual(await session.callTool("getChildren", { nodeId: "root" }), [{ id: "child", name: "Button" }]);
  const created = await session.callTool("createFrameNode", { attributes, parentId: "root" });
  assert.deepEqual(created, { id: "created", ...attributes });
  await session.callTool("createFrameNode", { attributes });
  assert.deepEqual(calls, [
    ["getNode", "missing"], ["getChildren", "root"],
    ["createFrameNode", attributes, "root"], ["createFrameNode", attributes],
  ]);
  assert.equal(session.listTools().find((tool) => tool.name === "createFrameNode").annotations.idempotentHint, false);
  await session.close();
});

test("validate inputs before invoking any SDK mutation", async () => {
  let writes = 0;
  const { session } = await fixture({ createFrameNode() { writes += 1; }, disconnect() {} });
  for (const args of [{}, { attributes: [] }, { attributes: {}, parentId: "" }, { attributes: {}, unknown: true }, []]) {
    await assert.rejects(session.callTool("createFrameNode", args), { code: "invalid_arguments" });
  }
  assert.equal(writes, 0);
  await session.close();
});

test("fixed errors omit SDK credential-bearing exception details", async () => {
  const { session } = await fixture({
    getProjectInfo() { throw new Error(`Bearer ${TEST_KEY} private endpoint`); },
    disconnect() { throw new Error(TEST_KEY); },
  });
  await assert.rejects(session.callTool("getProjectInfo"), (error) => {
    assert.equal(error.code, "upstream_error");
    assert.equal(error.message.includes(TEST_KEY), false);
    assert.equal(error.message.includes("private endpoint"), false);
    return true;
  });
  await assert.rejects(session.close(), { code: "disconnect_failed" });
  await session.close();
});

test("redact the exact connection secret while preserving other JSON fields", async () => {
  const { session } = await fixture({
    getProjectInfo() { return { designToken: "brand.primary", notes: `prefix ${TEST_KEY} suffix`, nested: [null, true, 42] }; },
    disconnect() {},
  });
  assert.deepEqual(await session.callTool("getProjectInfo"), {
    designToken: "brand.primary", notes: "prefix [REDACTED] suffix", nested: [null, true, 42],
  });
  await session.close();
});

test("unrepresentable SDK result fails explicitly rather than pretending acceptance", async () => {
  const cycle = {};
  cycle.self = cycle;
  const { session } = await fixture({ getProjectInfo() { return cycle; }, disconnect() {} });
  await assert.rejects(session.callTool("getProjectInfo"), { code: "result_not_serializable" });
  await session.close();
});

test("startup preconditions and connection failure are safe without a native host", async () => {
  let loads = 0;
  const loadSdk = async () => { loads += 1; throw new Error(TEST_KEY); };
  await assert.rejects(FramerSdkSession.open({ project: "", apiKey: TEST_KEY, loadSdk }), { code: "project_required" });
  await assert.rejects(FramerSdkSession.open({ project: PROJECT, apiKey: "", loadSdk }), { code: "auth_required" });
  assert.equal(loads, 0);
  await assert.rejects(FramerSdkSession.open({ project: PROJECT, apiKey: TEST_KEY, loadSdk }), { code: "dependency_missing" });
  const result = await protocolFixture([], {}, {
    loadSdk: async () => ({ connect: async () => { throw new Error(TEST_KEY); } }),
  });
  assert.equal(result.exitCode, 1);
  assert.equal(result.responses[0].ready, false);
  assert.equal(result.responses[0].error.code, "connection_failed");
  assert.equal(result.output.includes(TEST_KEY), false);
});

test("slow call followed by fast requests preserves every response ID", async () => {
  let disconnects = 0;
  const requests = [
    { id: "slow", method: "call_tool", name: "getNode", arguments: { nodeId: "slow-node" } },
    { id: "fast", method: "call_tool", name: "getProjectInfo" },
    { id: "list", method: "list_tools" },
    { id: "close", method: "close" },
  ];
  const result = await protocolFixture(requests, {
    async getNode(id) { await wait(25); return { id }; },
    getProjectInfo() { return { id: PROJECT }; },
    disconnect() { disconnects += 1; },
  });
  assert.equal(result.exitCode, 0);
  assert.equal(result.responses[0].protocol, PROTOCOL);
  assert.deepEqual(result.responses.slice(1).map((response) => response.id), ["slow", "fast", "list", "close"]);
  assert.deepEqual(result.responses[1].result, { id: "slow-node" });
  assert.deepEqual(result.responses[2].result, { id: PROJECT });
  assert.equal(disconnects, 1);
});

test("malformed requests do not execute SDK methods or contaminate the next result", async () => {
  let calls = 0;
  let disconnects = 0;
  const result = await protocolFixture([
    "not JSON",
    { id: true, method: "call_tool", name: "getProjectInfo" },
    { id: "blocked", method: "call_tool", name: "publish" },
    { id: "valid", method: "call_tool", name: "getProjectInfo" },
  ], {
    getProjectInfo() { calls += 1; return { id: PROJECT }; },
    disconnect() { disconnects += 1; },
  });
  assert.equal(result.responses[1].error.code, "invalid_request");
  assert.equal(result.responses[2].error.code, "invalid_request");
  assert.equal(result.responses[3].error.code, "tool_unavailable");
  assert.deepEqual(result.responses[4], { id: "valid", result: { id: PROJECT } });
  assert.equal(calls, 1);
  assert.equal(disconnects, 1);
});
