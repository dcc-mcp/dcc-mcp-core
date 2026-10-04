"""Host-free JSON-lines adapter tests; none is native Framer acceptance."""

from __future__ import annotations

import asyncio
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

_EXAMPLE = Path(__file__).resolve().parents[2]
_SPEC = importlib.util.spec_from_file_location("framer_example_adapter", _EXAMPLE / "framer_adapter.py")
_MODULE = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = _MODULE
_SPEC.loader.exec_module(_MODULE)
FramerNodeSession = _MODULE.FramerNodeSession
FramerAdapterError = _MODULE.FramerAdapterError
_FIXTURE = Path(__file__).with_name("fake_sdk_facade.mjs")


@unittest.skipUnless(shutil.which("node"), "Node is required for host-free process tests")
class FramerAdapterTests(unittest.TestCase):
    def run_async(self, coroutine):
        loop = asyncio.ProactorEventLoop() if sys.platform == "win32" else asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            return loop.run_until_complete(coroutine)
        finally:
            try:
                loop.run_until_complete(loop.shutdown_asyncgens())
            finally:
                asyncio.set_event_loop(None)
                loop.close()

    def session(self, **kwargs):
        return FramerNodeSession(project="fixture-project", facade_path=_FIXTURE, **kwargs)

    def test_core_callbacks_and_result_preservation(self):
        async def scenario():
            async with self.session() as session:
                inventory = await session.list_tools(cursor=None)
                names = {tool["name"] for tool in inventory["tools"]}
                self.assertIn("createFrameNode", names)
                self.assertNotIn("publish", names)
                self.assertNotIn("deploy", names)
                result = await session.call_tool("getProjectInfo")
                self.assertFalse(result["isError"])
                self.assertEqual(result["structuredContent"], {"id": "fixture-project", "name": "Host-free fixture"})
                self.assertEqual(json.loads(result["content"][0]["text"]), result["structuredContent"])
                children = await session.call_tool("getChildren", {"nodeId": "root"})
                self.assertEqual(children["structuredContent"], {"result": [{"id": "child"}]})
                self.assertEqual(json.loads(children["content"][0]["text"]), [{"id": "child"}])
                missing = await session.call_tool("getNode", {"nodeId": "missing"})
                self.assertEqual(missing["structuredContent"], {"result": None})
                attributes = {"name": "Editable Card", "width": "320px"}
                created = await session.call_tool("createFrameNode", {"attributes": attributes, "parentId": "root"})
                self.assertEqual(
                    created["structuredContent"], {"id": "created", "attributes": attributes, "parentId": "root"}
                )
            self.assertIsNotNone(session._process.returncode)

        self.run_async(scenario())

    def test_sdk_rejection_raises_and_invalidates_the_session(self):
        async def scenario():
            async with self.session() as session:
                with self.assertRaises(FramerAdapterError) as error:
                    await session.call_tool("getNode", {"nodeId": "error"})
                self.assertEqual(error.exception.code, "upstream_error")
                self.assertIsInstance(error.exception, ConnectionError)
                self.assertNotIn("test-only-not-a-credential", str(error.exception))
                with self.assertRaises(FramerAdapterError) as closed:
                    await session.call_tool("getProjectInfo")
                self.assertEqual(closed.exception.code, "session_closed")
            async with self.session() as session:
                with self.assertRaises(FramerAdapterError) as blocked:
                    await session.call_tool("publish")
                self.assertEqual(blocked.exception.code, "tool_unavailable")

        self.run_async(scenario())

    def test_serialized_slow_and_fast_calls_remain_correlated(self):
        async def scenario():
            async with self.session(request_timeout=2) as session:
                slow, fast = await asyncio.gather(
                    session.call_tool("getNode", {"nodeId": "slow"}),
                    session.call_tool("getNode", {"nodeId": "fast"}),
                )
                self.assertEqual(slow["structuredContent"], {"id": "slow"})
                self.assertEqual(fast["structuredContent"], {"id": "fast"})

        self.run_async(scenario())

    def test_timeout_invalidates_session_without_replaying(self):
        async def scenario():
            async with self.session(request_timeout=0.1) as session:
                with self.assertRaises(FramerAdapterError) as timed_out:
                    await session.call_tool("getNode", {"nodeId": "slow"})
                self.assertEqual(timed_out.exception.code, "request_timeout")
                self.assertIsInstance(timed_out.exception, ConnectionError)
                with self.assertRaises(FramerAdapterError) as subsequent:
                    await session.call_tool("getProjectInfo")
                self.assertEqual(subsequent.exception.code, "session_closed")
                self.assertIsNotNone(session._process.returncode)

        self.run_async(scenario())

    def test_mismatched_response_is_never_delivered_as_call_result(self):
        async def scenario(script):
            async with FramerNodeSession(project="fixture-project", facade_path=script) as session:
                with self.assertRaises(FramerAdapterError) as response:
                    await session.call_tool("getProjectInfo")
                self.assertEqual(response.exception.code, "response_id_mismatch")
                self.assertIsInstance(response.exception, ConnectionError)
                self.assertNotIn("stale-payload", str(response.exception))

        with tempfile.TemporaryDirectory(prefix="framer-contract-") as directory:
            script = Path(directory) / "mismatch.mjs"
            script.write_text(
                'import {createInterface} from "node:readline";\n'
                'console.log(JSON.stringify({ready:true,protocol:"framer-sdk-jsonl/v1"}));\n'
                "const lines=createInterface({input:process.stdin});\n"
                "for await(const line of lines){JSON.parse(line);"
                'console.log(JSON.stringify({id:"wrong",result:{id:"stale-payload"}}));}\n',
                encoding="utf-8",
            )
            self.run_async(scenario(script))

    def test_missing_project_fails_without_spawning_a_process(self):
        async def scenario():
            session = FramerNodeSession(project=" ", facade_path=_FIXTURE)
            with self.assertRaises(FramerAdapterError) as error:
                await session.__aenter__()
            self.assertEqual(error.exception.code, "project_required")
            self.assertIsNone(session._process)

        self.run_async(scenario())

    def test_typed_auth_failures_raise_and_invalidate_the_session(self):
        async def scenario(node_id, status_code, code):
            async with self.session() as session:
                with self.assertRaises(FramerAdapterError) as failure:
                    await session.call_tool("getNode", {"nodeId": node_id})
                self.assertEqual(failure.exception.code, code)
                self.assertEqual(failure.exception.status_code, status_code)
                self.assertNotIn("test-only-not-a-credential", str(failure.exception))
                with self.assertRaises(FramerAdapterError) as closed:
                    await session.list_tools()
                self.assertEqual(closed.exception.code, "session_closed")

        self.run_async(scenario("auth", 401, "auth_required"))
        self.run_async(scenario("denied", 403, "permission_denied"))

    def test_result_serialization_failure_is_a_typed_adapter_failure(self):
        async def scenario():
            async with self.session() as session:
                with self.assertRaises(FramerAdapterError) as failure:
                    await session.call_tool("getNode", {"nodeId": "unserializable"})
                self.assertEqual(failure.exception.code, "result_not_serializable")
                self.assertIsInstance(failure.exception, ConnectionError)
                with self.assertRaises(FramerAdapterError):
                    await session.list_tools()

        self.run_async(scenario())

    def test_core_marks_sdk_rejections_and_timeouts_indeterminate(self):
        # Optional integration evidence: base adapter tests require no Core import.
        core_path = _EXAMPLE.parents[1] / "python" / "dcc_mcp_core" / "experimental" / "design_bridge.py"
        if not core_path.is_file():
            self.skipTest("The Core design bridge source is not available")
        spec = importlib.util.spec_from_file_location("framer_core_integration_fixture", core_path)
        core = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = core
        try:
            spec.loader.exec_module(core)
        except ImportError:
            self.skipTest("The Core runtime is not available; base adapter tests still run")

        async def scenario(node_id, timeout):
            async with self.session(request_timeout=timeout) as session:
                bridge = core.DesignSessionBridge("framer")
                await bridge.connect(core.FramerApiFacade(list_tools=session.list_tools, call_tool=session.call_tool))
                before = bridge.status()["generation"]
                with self.assertRaises(core.DesignBridgeError) as failure:
                    await bridge.call_tool("getNode", {"nodeId": node_id})
                self.assertEqual(failure.exception.code, "unavailable")
                self.assertTrue(failure.exception.dispatched)
                self.assertEqual(failure.exception.to_dict()["context"]["outcome"], "indeterminate")
                self.assertEqual(bridge.status()["state"], "unavailable")
                self.assertFalse(bridge.status()["connected"])
                self.assertGreater(bridge.status()["generation"], before)

        self.run_async(scenario("error", 2.0))
        self.run_async(scenario("slow", 0.1))


if __name__ == "__main__":
    unittest.main()
