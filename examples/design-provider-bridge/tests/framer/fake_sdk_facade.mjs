/** Test-only process fixture. It never imports framer-api or connects to Framer. */
import { runFramerFacade } from "../../framer_facade.mjs";

process.exitCode = await runFramerFacade({
  env: { FRAMER_API_KEY: "test-only-not-a-credential" },
  loadSdk: async () => ({
    connect: async (project) => ({
      getProjectInfo: async () => ({ id: project, name: "Host-free fixture" }),
      getCanvasRoot: async () => ({ id: "root" }),
      getNode: async (nodeId) => {
        if (nodeId === "slow") await new Promise((resolve) => setTimeout(resolve, 250));
        if (nodeId === "error") throw new Error("test-only-not-a-credential");
        if (nodeId === "auth") throw Object.assign(new Error("test-only-not-a-credential"), { statusCode: 401 });
        if (nodeId === "denied") throw Object.assign(new Error("test-only-not-a-credential"), { status_code: 403 });
        if (nodeId === "unserializable") return 1n;
        return nodeId === "missing" ? null : { id: nodeId };
      },
      getChildren: async () => [{ id: "child" }],
      createFrameNode: async (attributes, parentId) => ({ id: "created", attributes, parentId: parentId ?? null }),
      disconnect: async () => {},
    }),
  }),
  signals: process,
});
