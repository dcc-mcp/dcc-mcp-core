#!/usr/bin/env node
/** Internal JSON-lines adapter for the official Framer SDK; this is not MCP. */
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { FramerSdkSession, FramerSessionError } from "./framer_session.mjs";

export const PROTOCOL = "framer-sdk-jsonl/v1";
const MAX_LINE_BYTES = 8 * 1024 * 1024;

function safeError(error) {
  return error instanceof FramerSessionError
    ? { code: error.code, message: error.message }
    : { code: "adapter_error", message: "The Framer SDK adapter failed." };
}

function writeLine(output, value) {
  output.write(`${JSON.stringify(value)}\n`);
}

function requestId(request) {
  return (typeof request?.id === "string" ||
    (typeof request?.id === "number" && Number.isSafeInteger(request.id)))
    ? request.id : null;
}

function projectArgument(argv, env) {
  if (argv.length === 0) return env.FRAMER_PROJECT ?? env.FRAMER_PROJECT_ID;
  if (argv.length === 2 && argv[0] === "--project") return argv[1];
  throw new FramerSessionError("invalid_arguments");
}

export async function runFramerFacade({
  input = process.stdin,
  output = process.stdout,
  env = process.env,
  argv = process.argv.slice(2),
  loadSdk,
  signals = null,
} = {}) {
  let session;
  let lines;
  let interrupted = false;
  let closePromise;
  const closeSession = () => {
    if (!closePromise) closePromise = session ? session.close() : Promise.resolve();
    return closePromise;
  };
  const onSignal = () => {
    interrupted = true;
    lines?.close();
    // A pending SDK call may be indivisible. The caller owns bounded termination.
    void closeSession().catch(() => {});
  };
  try {
    session = await FramerSdkSession.open({
      project: projectArgument(argv, env), apiKey: env.FRAMER_API_KEY, loadSdk,
    });
  } catch (error) {
    writeLine(output, { ready: false, protocol: PROTOCOL, error: safeError(error) });
    return 1;
  }
  lines = createInterface({ input, crlfDelay: Infinity });
  signals?.on("SIGTERM", onSignal);
  signals?.on("SIGINT", onSignal);
  writeLine(output, { ready: true, protocol: PROTOCOL, provider: "framer", transport: "official_sdk" });
  let exitCode = 0;
  try {
    for await (const line of lines) {
      if (interrupted) break;
      let request;
      try {
        if (Buffer.byteLength(line) > MAX_LINE_BYTES) throw new Error("Large request");
        request = JSON.parse(line);
      } catch {
        writeLine(output, { id: null, error: { code: "invalid_request", message: "A bounded JSON request is required." } });
        continue;
      }
      const id = requestId(request);
      if (id === null || !request || Array.isArray(request)) {
        writeLine(output, { id: null, error: { code: "invalid_request", message: "A request ID is required." } });
        continue;
      }
      try {
        if (request.method === "list_tools") {
          if (request.cursor != null) throw new FramerSessionError("invalid_arguments");
          writeLine(output, { id, result: { tools: session.listTools() } });
        } else if (request.method === "call_tool") {
          const result = await session.callTool(request.name, request.arguments ?? {});
          writeLine(output, { id, result });
        } else if (request.method === "close") {
          await closeSession();
          writeLine(output, { id, result: { closed: true } });
          break;
        } else {
          writeLine(output, { id, error: { code: "invalid_request", message: "This internal adapter method is unavailable." } });
        }
      } catch (error) {
        writeLine(output, { id, error: safeError(error) });
        if (request.method === "close") {
          exitCode = 1;
          break;
        }
      }
    }
  } finally {
    lines.close();
    signals?.off("SIGTERM", onSignal);
    signals?.off("SIGINT", onSignal);
    try {
      await closeSession();
    } catch {
      exitCode = 1;
    }
  }
  return exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await runFramerFacade({ signals: process });
}
