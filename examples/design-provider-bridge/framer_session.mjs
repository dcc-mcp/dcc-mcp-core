/** A curated session over the existing official framer-api SDK.
 * Official method references:
 * https://www.framer.com/developers/server-api-quick-start
 * https://www.framer.com/developers/reference/plugins-create-frame-node
 * https://www.framer.com/developers/reference/plugins-get-node
 * https://www.framer.com/developers/reference/plugins-get-children
 * This module does not install dependencies, create credentials, or publish.
 */

const MESSAGES = Object.freeze({
  project_required: "An existing Framer project ID or URL is required.",
  auth_required: "An existing FRAMER_API_KEY is required.",
  permission_denied: "The existing Framer authorization denied this operation.",
  dependency_missing: "The official framer-api package must already be installed.",
  sdk_incompatible: "The installed SDK does not expose the required session methods.",
  connection_failed: "The official SDK connection failed; verify existing project access.",
  session_closed: "The Framer SDK session is closed.",
  tool_unavailable: "This method is not available in the curated SDK session.",
  invalid_arguments: "The supplied arguments do not match the curated method schema.",
  upstream_error: "The official SDK call failed; inspect the project with the operator.",
  result_not_serializable: "The SDK result cannot be represented as JSON.",
  disconnect_failed: "The official SDK did not confirm disconnection.",
});

export class FramerSessionError extends Error {
  constructor(code) {
    super(MESSAGES[code] ?? MESSAGES.upstream_error);
    this.name = "FramerSessionError";
    this.code = Object.hasOwn(MESSAGES, code) ? code : "upstream_error";
  }
}

function sdkError(error, fallback) {
  const status = error?.status_code ?? error?.statusCode ?? error?.status ??
    error?.response?.status_code ?? error?.response?.status;
  if (status === 401) return new FramerSessionError("auth_required");
  if (status === 403) return new FramerSessionError("permission_denied");
  return new FramerSessionError(fallback);
}

function descriptor(name, description, properties, required, readOnly) {
  return {
    name,
    description,
    inputSchema: {
      type: "object", properties, required, additionalProperties: false,
    },
    annotations: {
      readOnlyHint: readOnly,
      destructiveHint: false,
      idempotentHint: readOnly,
      openWorldHint: true,
    },
  };
}

const NODE_ID = { type: "string", minLength: 1 };
const METHODS = Object.freeze([
  descriptor("getProjectInfo", "Read metadata from the connected Framer project.", {}, [], true),
  descriptor("getCanvasRoot", "Read the canvas root from the connected Framer project.", {}, [], true),
  descriptor("getNode", "Read a Framer node by its existing ID.", { nodeId: NODE_ID }, ["nodeId"], true),
  descriptor("getChildren", "Read the children of a Framer node by its existing ID.", { nodeId: NODE_ID }, ["nodeId"], true),
  descriptor("createFrameNode", "Create an editable frame using the official SDK attributes and optional parent ID.", {
    attributes: { type: "object", additionalProperties: true },
    parentId: NODE_ID,
  }, ["attributes"], false),
]);

function object(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validateArguments(tool, args) {
  if (!object(args)) throw new FramerSessionError("invalid_arguments");
  if (Object.keys(args).some((key) => !Object.hasOwn(tool.inputSchema.properties, key))) {
    throw new FramerSessionError("invalid_arguments");
  }
  for (const key of tool.inputSchema.required) {
    if (!Object.hasOwn(args, key)) throw new FramerSessionError("invalid_arguments");
  }
  for (const [key, value] of Object.entries(args)) {
    if (key === "attributes") {
      if (!object(value)) throw new FramerSessionError("invalid_arguments");
    } else if (typeof value !== "string" || value.trim().length === 0) {
      throw new FramerSessionError("invalid_arguments");
    }
  }
}

export class FramerSdkSession {
  #api;
  #secret;
  #closed = false;

  constructor(api, secret) {
    this.#api = api;
    this.#secret = secret;
  }

  static async open({ project, apiKey, loadSdk = () => import("framer-api") }) {
    if (typeof project !== "string" || project.trim().length === 0) {
      throw new FramerSessionError("project_required");
    }
    if (typeof apiKey !== "string" || apiKey.trim().length === 0) {
      throw new FramerSessionError("auth_required");
    }
    let sdk;
    try {
      sdk = await loadSdk();
    } catch {
      throw new FramerSessionError("dependency_missing");
    }
    if (typeof sdk?.connect !== "function") throw new FramerSessionError("sdk_incompatible");
    let api;
    try {
      api = await sdk.connect(project, apiKey);
    } catch (error) {
      // Do not expose SDK exception messages: they may include credentials or URLs.
      throw sdkError(error, "connection_failed");
    }
    if (!api || typeof api.disconnect !== "function") {
      throw new FramerSessionError("sdk_incompatible");
    }
    return new FramerSdkSession(api, apiKey);
  }

  listTools() {
    if (this.#closed) throw new FramerSessionError("session_closed");
    // SDK versions differ. Documentation is not proof that a method exists here.
    return METHODS.filter((tool) => typeof this.#api[tool.name] === "function")
      .map((tool) => structuredClone(tool));
  }

  async callTool(name, args = {}) {
    if (this.#closed) throw new FramerSessionError("session_closed");
    const tool = METHODS.find((candidate) => candidate.name === name);
    if (!tool || typeof this.#api[name] !== "function") {
      throw new FramerSessionError("tool_unavailable");
    }
    validateArguments(tool, args);
    let result;
    try {
      if (name === "getNode" || name === "getChildren") {
        result = await this.#api[name](args.nodeId);
      } else if (name === "createFrameNode") {
        result = Object.hasOwn(args, "parentId")
          ? await this.#api.createFrameNode(args.attributes, args.parentId)
          : await this.#api.createFrameNode(args.attributes);
      } else {
        result = await this.#api[name]();
      }
    } catch (error) {
      throw sdkError(error, "upstream_error");
    }
    if (this.#closed) throw new FramerSessionError("session_closed");
    return this.jsonPayload(result);
  }

  jsonPayload(result) {
    try {
      const json = JSON.stringify(result, (_key, value) =>
        typeof value === "string" ? value.split(this.#secret).join("[REDACTED]") : value);
      if (json === undefined) throw new Error("No JSON result");
      return JSON.parse(json);
    } catch {
      throw new FramerSessionError("result_not_serializable");
    }
  }

  async close() {
    if (this.#closed) return;
    this.#closed = true;
    try {
      await this.#api.disconnect();
    } catch {
      throw new FramerSessionError("disconnect_failed");
    } finally {
      this.#secret = "";
      this.#api = null;
    }
  }
}
