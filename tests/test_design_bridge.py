"""Host-free contracts for injected design sessions; no native acceptance claim."""

from __future__ import annotations

import asyncio
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

from dcc_mcp_core.design_bridge import DesignBridgeError
from dcc_mcp_core.design_bridge import DesignSessionBridge
from dcc_mcp_core.design_bridge import FramerApiFacade
from dcc_mcp_core.design_bridge import get_design_provider_profile


def descriptor(name="vendor_operation"):
    return {
        "name": name,
        "title": "Vendor operation",
        "description": "Contract fixture, not a native application tool.",
        "inputSchema": {
            "type": "object",
            "properties": {"value": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
        },
        "outputSchema": {"type": "object", "properties": {"artifact": {"type": "string"}}},
        "annotations": {"readOnlyHint": False, "destructiveHint": True, "openWorldHint": True},
        "_meta": {"vendor.extension": {"retained": ["a", "b"]}},
        "vendor_future_field": {"preserve": True},
    }


class ContractSession:
    """Protocol fixture with no host, transport, account or persistent authority."""

    def __init__(self, pages=None, result=None):
        self.pages = {None: {"tools": [descriptor()]}} if pages is None else pages
        self.result = (
            {"content": [{"type": "text", "text": "Fixture response"}], "isError": False} if result is None else result
        )
        self.listed = []
        self.called = []
        self.closed = False

    async def list_tools(self, cursor=None):
        self.listed.append(cursor)
        page = self.pages[cursor]
        if isinstance(page, BaseException):
            raise page
        return page

    async def call_tool(self, name, arguments=None):
        self.called.append((name, arguments))
        if isinstance(self.result, BaseException):
            raise self.result
        return self.result

    async def close(self):
        self.closed = True


class SdkModel:
    """Exercise SDK alias serialization without depending on the optional SDK."""

    def __init__(self, payload):
        self.payload = payload
        self.dump_options = None

    def model_dump(self, **options):
        self.dump_options = options
        return deepcopy(self.payload)


class SdkV1Model:
    """Exercise the legacy optional SDK serialization seam and wire aliases."""

    def __init__(self, payload):
        self.payload = payload

    def dict(self, **options):
        assert options == {"by_alias": True, "exclude_none": True}
        return deepcopy(self.payload)


def test_catalog_pagination_retains_exact_vendor_schema_metadata_and_future_fields():
    async def exercise():
        pages = {
            None: {"tools": [descriptor()], "nextCursor": "opaque:second", "_meta": {"page": 1}},
            "opaque:second": {"tools": [descriptor("second_operation")], "_meta": {"page": 2}},
        }
        session = ContractSession(pages)
        bridge = DesignSessionBridge("figma")
        capabilities = await bridge.connect(session)
        assert session.listed == [None, "opaque:second"]
        assert capabilities["tools"] == [descriptor(), descriptor("second_operation")]
        assert capabilities["catalogue_pages"] == list(pages.values())
        assert capabilities["tool_count"] == 2
        assert capabilities["native_acceptance"] == "unverified"
        assert capabilities["authentication"] == "unknown"
        assert capabilities["document_binding"] == "unverified"
        assert capabilities["host_availability"] == "unverified"
        assert capabilities["live_capabilities_verified"] is True
        capabilities["tools"][0]["annotations"]["destructiveHint"] = False
        assert bridge.describe_tool("vendor_operation") == descriptor()
        assert session.pages[None]["tools"][0] == descriptor()

    asyncio.run(exercise())


@pytest.mark.parametrize(
    "provider,protocol", [("figma", "mcp"), ("penpot", "mcp"), ("pencil", "mcp"), ("sketch", "mcp"), ("framer", "api")]
)
def test_profiles_identify_boundaries_without_advertising_host_capabilities(provider, protocol):
    bridge = DesignSessionBridge(provider.upper())
    status = bridge.status()
    assert status["provider"] == provider
    assert status["protocol"] == protocol
    assert status["connected"] is False
    assert status["state"] == "disconnected"
    assert status["live_capabilities_verified"] is False
    assert status["authentication"] == "unknown"
    assert status["document_binding"] == "unverified"
    assert status["host_availability"] == "unverified"
    assert bridge.capabilities()["tools"] == []


@pytest.mark.parametrize("provider", ["", "unreal", None])
def test_unknown_profiles_fail_without_echoing_user_data(provider):
    with pytest.raises(ValueError, match=r"^Unknown design provider$"):
        get_design_provider_profile(provider)


def test_sketch_rejects_unsupported_upstream_platform_before_session_access():
    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("sketch")
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.connect(session, host_platform="win32")
        assert caught.value.code == "unsupported_host"
        assert session.listed == []
        assert bridge.status()["state"] == "unsupported_host"
        assert bridge.capabilities()["tools"] == []

    asyncio.run(exercise())


def test_sketch_can_use_an_explicitly_mac_hosted_session_without_launching_a_host():
    async def exercise():
        bridge = DesignSessionBridge("sketch")
        status = await bridge.connect(ContractSession(), host_platform="darwin")
        assert status["state"] == "ready"
        assert status["native_acceptance"] == "unverified"

    asyncio.run(exercise())


def test_framer_requires_explicit_api_facade_and_never_claims_official_mcp():
    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("framer")
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.connect(session)
        assert caught.value.reason == "invalid_session"
        assert session.listed == []
        facade = FramerApiFacade(list_tools=session.list_tools, call_tool=session.call_tool)
        capabilities = await bridge.connect(facade)
        generation = capabilities["generation"]
        result = await bridge.call_tool("vendor_operation", {"value": "api"}, expected_generation=generation)
        assert session.called == [("vendor_operation", {"value": "api"})]
        assert bridge.status()["protocol"] == "api"
        assert result.to_dict()["context"]["native_acceptance"] == "unverified"

    asyncio.run(exercise())


def test_sdk_models_serialize_aliases_and_preserve_all_result_content_blocks():
    async def exercise():
        payload = {
            "content": [
                {"type": "text", "text": "Vendor result"},
                {"type": "image", "data": "YWJj", "mimeType": "image/png", "_meta": {"vendor.image": 1}},
                {
                    "type": "resource",
                    "resource": {"uri": "vendor://asset/1", "mimeType": "text/plain", "text": "Editable asset"},
                },
                {"type": "resource_link", "uri": "vendor://export/1", "name": "Export"},
            ],
            "structuredContent": {"artifact": "vendor://asset/1"},
            "isError": False,
            "_meta": {"vendor.result": {"fidelity": True}},
            "futureResultField": {"retained": [1, 2]},
        }
        page = SdkModel({"tools": [descriptor()], "_meta": {"vendor.list": "retained"}})
        response = SdkModel(payload)
        bridge = DesignSessionBridge("penpot")
        await bridge.connect(ContractSession({None: page}, response))
        result = await bridge.call_tool("vendor_operation", {"value": "unchanged"})
        assert page.dump_options == {"mode": "json", "by_alias": True, "exclude_none": True}
        assert response.dump_options == page.dump_options
        assert result.upstream_result == payload
        envelope = result.to_dict()
        assert envelope["context"]["upstream_result"] == payload
        assert envelope["context"]["transport_success"] is True
        assert result.tool_error is False
        assert envelope["postcondition"]["verified"] is False
        envelope["context"]["upstream_result"]["_meta"]["vendor.result"]["fidelity"] = False
        assert result.upstream_result == payload

    asyncio.run(exercise())


def test_arguments_are_preserved_and_isolated_without_invented_provider_parameters():
    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("pencil")
        await bridge.connect(session)
        arguments = {"value": "unchanged", "nested": {"vendor_key": [1, 2]}}
        await bridge.call_tool("vendor_operation", arguments)
        assert session.called == [("vendor_operation", arguments)]
        session.called[0][1]["nested"]["vendor_key"].append(3)
        assert arguments["nested"]["vendor_key"] == [1, 2]

    asyncio.run(exercise())


def test_sdk_v1_models_nested_in_mapping_pages_and_content_keep_wire_aliases():
    async def exercise():
        text = {"type": "text", "text": "SDK-v1 fixture", "_meta": {"vendor.text": True}}
        payload = {"content": [SdkV1Model(text)], "isError": False, "_meta": {"vendor.result": 1}}
        page = {"tools": [SdkV1Model(descriptor())], "_meta": {"vendor.list": 2}}
        bridge = DesignSessionBridge("penpot")
        capabilities = await bridge.connect(ContractSession({None: page}, SdkV1Model(payload)))
        assert capabilities["tools"] == [descriptor()]
        assert capabilities["catalogue_pages"][0]["tools"] == [descriptor()]
        result = await bridge.call_tool("vendor_operation")
        assert result.upstream_result == {"content": [text], "isError": False, "_meta": {"vendor.result": 1}}
        assert result.tool_error is None
        json.dumps(capabilities, allow_nan=False)
        json.dumps(result.to_dict(), allow_nan=False)

    asyncio.run(exercise())


@pytest.mark.parametrize("flag", [False, None])
@pytest.mark.parametrize("include_structured_null", [False, True])
def test_plain_text_business_failure_is_unverified_even_when_transport_succeeds(flag, include_structured_null):
    async def exercise():
        payload = {"content": [{"type": "text", "text": "Cannot save: the document is read-only."}]}
        if flag is not None:
            payload["isError"] = flag
        if include_structured_null:
            payload["structuredContent"] = None
        bridge = DesignSessionBridge("figma")
        await bridge.connect(ContractSession(result=payload))
        result = await bridge.call_tool("vendor_operation")
        context = result.to_dict()["context"]
        assert result.transport_success is True
        assert result.tool_error is None
        assert context["outcome"] == "unverified"
        assert context["native_acceptance"] == "unverified"
        assert context["upstream_result"] == payload

    asyncio.run(exercise())


def test_explicit_upstream_tool_error_is_separate_from_transport_failure():
    async def exercise():
        payload = {"content": [{"type": "text", "text": "The upstream tool rejected the request."}], "isError": True}
        bridge = DesignSessionBridge("figma")
        await bridge.connect(ContractSession(result=payload))
        result = await bridge.call_tool("vendor_operation")
        envelope = result.to_dict()
        assert envelope["success"] is False
        assert envelope["error"] == "upstream_error"
        assert envelope["context"]["transport_success"] is True
        assert envelope["context"]["tool_error"] is True
        assert envelope["context"]["upstream_result"] == payload

    asyncio.run(exercise())


def test_unadvertised_tool_never_reaches_session_and_empty_catalog_does_not_guess_tools():
    async def exercise():
        session = ContractSession({None: {"tools": []}})
        bridge = DesignSessionBridge("penpot")
        await bridge.connect(session)
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.call_tool("guessed_canvas_write")
        assert caught.value.reason == "tool_not_advertised"
        assert caught.value.dispatched is False
        assert session.called == []

    asyncio.run(exercise())


@pytest.mark.parametrize(
    "pages,max_pages",
    [
        ({None: {"tools": [descriptor()], "nextCursor": "loop"}, "loop": {"tools": [], "nextCursor": "loop"}}, 10),
        ({None: {"tools": [descriptor()], "nextCursor": "second"}, "second": {"tools": [descriptor()]}}, 10),
        ({None: {"tools": [descriptor()], "nextCursor": "second"}}, 1),
        ({None: {"tools": [descriptor()], "nextCursor": 2}}, 10),
        ({None: {"tools": [{"name": "missing_schema"}]}}, 10),
        ({None: {"tools": {"incorrect": "object"}}}, 10),
    ],
)
def test_malformed_or_truncated_catalog_is_never_advertised_as_partial_success(pages, max_pages):
    async def exercise():
        bridge = DesignSessionBridge("penpot", max_pages=max_pages)
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.connect(ContractSession(pages))
        assert caught.value.code == "upstream_error"
        assert bridge.status()["connected"] is False
        assert bridge.capabilities()["tools"] == []
        assert bridge.capabilities()["catalogue_pages"] == []

    asyncio.run(exercise())


class HttpFailure(RuntimeError):
    def __init__(self, status_code):
        self.status_code = status_code
        super().__init__("synthetic-secret must not enter the public error")


@pytest.mark.parametrize(
    "failure,code",
    [
        (HttpFailure(401), "auth_required"),
        (HttpFailure(403), "permission_denied"),
        (PermissionError("synthetic-secret"), "permission_denied"),
        (ConnectionError("synthetic-secret"), "unavailable"),
        (TimeoutError("synthetic-secret"), "unavailable"),
        (RuntimeError("synthetic-secret"), "upstream_error"),
        (DesignBridgeError("auth_required"), "auth_required"),
    ],
)
def test_failures_are_classified_without_returning_exception_messages_or_secrets(failure, code):
    async def exercise():
        bridge = DesignSessionBridge("figma")
        session = ContractSession({None: failure})
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.connect(session)
        assert caught.value.code == code
        assert "synthetic-secret" not in str(caught.value)
        assert "synthetic-secret" not in json.dumps(caught.value.to_dict())
        assert bridge.status()["last_error"] == code
        assert caught.value.dispatched is False

    asyncio.run(exercise())


def test_permission_denied_call_invalidates_authority_and_is_never_automatically_retried():
    async def exercise():
        session = ContractSession(result=PermissionError("synthetic-secret"))
        bridge = DesignSessionBridge("pencil")
        discovered = await bridge.connect(session)
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.call_tool("vendor_operation", expected_generation=discovered["generation"])
        assert caught.value.code == "permission_denied"
        assert caught.value.to_dict()["context"]["outcome"] == "indeterminate"
        assert bridge.status()["state"] == "permission_denied"
        assert bridge.capabilities()["tools"] == []
        with pytest.raises(DesignBridgeError):
            await bridge.call_tool("vendor_operation")
        assert len(session.called) == 1

    asyncio.run(exercise())


def test_refresh_changes_generation_removed_tools_and_rejects_stale_discovery():
    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("figma")
        initial = await bridge.connect(session)
        session.pages = {None: {"tools": [descriptor("replacement_operation")]}}
        refreshed = await bridge.refresh_tools(expected_generation=initial["generation"])
        assert refreshed["generation"] > initial["generation"]
        with pytest.raises(DesignBridgeError) as stale:
            await bridge.call_tool("replacement_operation", expected_generation=initial["generation"])
        assert stale.value.reason == "stale_session"
        with pytest.raises(DesignBridgeError) as missing:
            await bridge.call_tool("vendor_operation", expected_generation=refreshed["generation"])
        assert missing.value.reason == "tool_not_advertised"
        assert session.called == []

    asyncio.run(exercise())


def test_failed_refresh_discards_old_catalog_until_explicit_reconnection():
    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("figma")
        await bridge.connect(session)
        session.pages = {None: ConnectionError("synthetic-secret")}
        with pytest.raises(DesignBridgeError):
            await bridge.refresh_tools()
        assert bridge.capabilities()["tools"] == []
        with pytest.raises(DesignBridgeError):
            await bridge.call_tool("vendor_operation")
        assert session.called == []

    asyncio.run(exercise())


def test_reconnect_rejects_old_inflight_result_without_clobbering_new_session():
    async def exercise():
        entered = asyncio.Event()
        release = asyncio.Event()

        class SlowCall(ContractSession):
            async def call_tool(self, name, arguments=None):
                entered.set()
                await release.wait()
                return await super().call_tool(name, arguments=arguments)

        bridge = DesignSessionBridge("figma")
        old = SlowCall()
        initial = await bridge.connect(old)
        pending = asyncio.create_task(bridge.call_tool("vendor_operation", expected_generation=initial["generation"]))
        await entered.wait()
        replacement = ContractSession({None: {"tools": [descriptor("replacement_operation")]}})
        current = await bridge.connect(replacement)
        release.set()
        with pytest.raises(DesignBridgeError) as caught:
            await pending
        assert caught.value.reason == "stale_session"
        assert caught.value.dispatched is True
        assert caught.value.to_dict()["context"]["outcome"] == "indeterminate"
        assert bridge.status()["state"] == "ready"
        assert bridge.status()["generation"] == current["generation"]
        assert bridge.capabilities()["tools"] == [descriptor("replacement_operation")]
        assert replacement.called == []

    asyncio.run(exercise())


def test_disconnect_rejects_late_discovery_without_closing_caller_owned_session():
    async def exercise():
        entered = asyncio.Event()
        release = asyncio.Event()

        class SlowList(ContractSession):
            async def list_tools(self, cursor=None):
                entered.set()
                await release.wait()
                return await super().list_tools(cursor=cursor)

        session = SlowList()
        bridge = DesignSessionBridge("figma")
        connecting = asyncio.create_task(bridge.connect(session))
        await entered.wait()
        disconnected = bridge.disconnect()
        release.set()
        with pytest.raises(DesignBridgeError) as caught:
            await connecting
        assert caught.value.reason == "stale_session"
        assert bridge.status() == disconnected
        assert bridge.capabilities()["tools"] == []
        assert session.closed is False

    asyncio.run(exercise())


@pytest.mark.parametrize(
    "result",
    [
        {"content": [], "isError": "false"},
        {"content": [None], "isError": False},
        {"content": [{"text": "Missing type"}], "isError": False},
        {"unexpected": "shape"},
        "unexpected text",
    ],
)
def test_malformed_call_response_is_an_indeterminate_error_instead_of_transport_success(result):
    async def exercise():
        bridge = DesignSessionBridge("figma")
        await bridge.connect(ContractSession(result=result))
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.call_tool("vendor_operation")
        assert caught.value.code == "upstream_error"
        assert caught.value.dispatched is True
        assert caught.value.to_dict()["context"]["outcome"] == "indeterminate"

    asyncio.run(exercise())


def test_local_argument_copy_failure_is_not_marked_as_an_upstream_dispatch():
    class Uncopyable:
        def __deepcopy__(self, memo):
            raise RuntimeError("synthetic-secret")

    async def exercise():
        session = ContractSession()
        bridge = DesignSessionBridge("figma")
        await bridge.connect(session)
        with pytest.raises(DesignBridgeError) as caught:
            await bridge.call_tool("vendor_operation", {"value": Uncopyable()})
        assert caught.value.reason == "invalid_arguments"
        assert caught.value.dispatched is False
        assert caught.value.to_dict()["context"]["outcome"] == "not_dispatched"
        assert session.called == []
        assert "synthetic-secret" not in str(caught.value)

    asyncio.run(exercise())


@pytest.mark.parametrize("operation", ["discover", "call"])
def test_cancelled_operations_detach_the_session_without_claiming_host_cancellation(operation):
    async def exercise():
        entered = asyncio.Event()
        release = asyncio.Event()

        class CancellableSession(ContractSession):
            async def list_tools(self, cursor=None):
                if operation == "discover":
                    entered.set()
                    await release.wait()
                return await super().list_tools(cursor=cursor)

            async def call_tool(self, name, arguments=None):
                entered.set()
                await release.wait()
                return await super().call_tool(name, arguments=arguments)

        session = CancellableSession()
        bridge = DesignSessionBridge("figma")
        if operation == "discover":
            pending = asyncio.create_task(bridge.connect(session))
        else:
            await bridge.connect(session)
            pending = asyncio.create_task(bridge.call_tool("vendor_operation"))
        await entered.wait()
        pending.cancel()
        with pytest.raises(asyncio.CancelledError):
            await pending
        assert bridge.status()["state"] == "unavailable"
        assert bridge.status()["connected"] is False
        assert bridge.capabilities()["tools"] == []
        assert bridge.status()["native_acceptance"] == "unverified"
        assert session.closed is False

    asyncio.run(exercise())


def test_import_is_dependency_light_and_does_not_load_native_extension_or_vendor_sdk():
    import dcc_mcp_core.design_bridge as module

    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(Path(module.__file__).resolve().parents[1])
    code = (
        "import sys; import dcc_mcp_core.design_bridge; "
        "assert 'dcc_mcp_core._core' not in sys.modules; "
        "assert 'mcp' not in sys.modules; "
        "assert 'framer' not in sys.modules"
    )
    completed = subprocess.run(
        [sys.executable, "-c", code], env=environment, capture_output=True, text=True, check=False
    )
    assert completed.returncode == 0, completed.stderr
