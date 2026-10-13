# Owned MCP worker launch dependency

`PixelsMcpHostClient` and `OwnedCuaMcpTransport` accept an optional,
constructor-only `managed_launcher(command, environment)` dependency. The
default process launch and public task protocol are unchanged. No environment
variable, tool argument, endpoint discovery, shared Host attachment, or lease
import enables this dependency.

This is a preparatory embedding interface. Core does not supply the launcher.
It does not fix `SupervisorNotIndependent` in an existing deployment or establish
that an execution runtime can create an independent worker.

## Current caller and capability boundary

The normal adapter route injects the server-owned `UiControlRuntimeOptions`
through `_server/execution_bridge.py` and `_server/inprocess_executor.py` into
`skills/ui-control/scripts/_cua_backend.py`. That backend constructs
`PixelsMcpHostClient` without `managed_launcher`; only a trusted embedding that
directly constructs the client or transport can use this dependency today.
Neither tool parameters nor a JSON process identifier can connect it.

There are two process boundaries. The MCP worker serves the bounded task
protocol. Native capture preparation creates a separate supervisor through
`CapturePreparationHandle::begin`, verifies that supervisor's original process
handle before resuming it, and checks independence again in the child. The
supervisor must survive the originating Job's termination long enough to settle
and restore temporary window changes. Checking the MCP worker alone does not
prove that this second launch, authorization, restoration, or capture succeeds.

The execution product owns the missing launch capability. A local embedding
can use the current dependency only when it can legitimately create and
transfer the original local `Popen` object. A remote or Rust-owned broker needs
a separately reviewed channel/process ownership interface; it cannot return a
PID or synthesize `Popen` to fit this one. An experimental App Server
`process/spawn` string handle is not that capability. Running outside a sandbox
also does not prove absence of Windows Job membership.

## Ownership transfer

The embedding must supply a trusted launcher that returns the original local
`subprocess.Popen` child with exclusive stdin/stdout. Returning it transfers
exclusive process, channel, and exit responsibility to Core, including the
existing bounded terminate/wait/kill cleanup. This interface cannot attach to a
shared worker or to an external owner that retains process lifecycle control.
Before return, launch failure and cleanup belong to the launcher.

A PID, session string, remote process handle, reconstructed Popen, or shared
channel does not satisfy this contract. A product requiring remote ownership
needs a separate reviewed interface; it must not simulate this transfer.

Before MCP initialization, Core checks the transferred child's original Windows
handle for PID, creation time, live state, no Job membership, and actual image
path. The image file must match the pre-launch file identity and pinned SHA-256.
Birth and Job state are checked again after image verification. `Popen.args`
alone is insufficient proof. API failures, changed identity, an unexpected image,
or Job membership refuse without falling back to the ordinary launcher.

For a refused transferred child, Core attempts one bounded original-owner
terminate/wait. `ManagedLaunchRefusal` retains the original process, primary
exception, cleanup exception, and exit code. An unproven exit remains unknown;
the embedding must retain this exception/owner until settlement. No PID
reacquisition, process-tree kill, replacement worker, or retry is performed.

After transfer, reader startup, MCP initialization, and task startup failures
also preserve the primary and cleanup errors. If cleanup is unproven, the
exception retains the verified identity and the transport or task client as
`identity` and `owner`, keeping original process/task/reader state reachable.

## Authorization and remaining product work

The full pinned DCC-CUA MCP worker still owns `start_task`, the process-local
issuer/lease, exact-target checks, revocation, and `stop_task`. Core neither
mints nor serializes that authority. Existing native supervisor Job checks,
capture preparation, cleanup settlement, and refusal latches are unchanged.

The actual execution/permission runtime must still provide an authorized
launcher, exclusive channel, ownership transfer, permission-withdrawal policy,
and reliable EOF/shutdown settlement. Merely exposing a generic process-spawn
API or injecting a callback does not implement those capabilities. This change
does not install a service or alter Job, token, parent, or interpreter policy.

The product integration must satisfy these requirements before it is used for
capture. They describe remaining acceptance obligations, not new public tools:

| Boundary | Required behavior and evidence |
| --- | --- |
| Launch | Bind the current task authorization to the exact pinned executable, argv, environment, and one launch attempt. Refusal never selects another launcher. |
| Ownership | Transfer the original local process and exclusive channel once, or implement a separately reviewed broker contract. Keep a single cleanup owner before and after transfer. |
| Identity | Read PID, full creation FILETIME, live state, actual image path, and Job membership through the retained original process handle. Open that image to verify its file identity and pinned hash. Record failures without reopening the process by PID. |
| Output | Keep protocol stdout exclusive. Drain or explicitly discard stderr. Use only owner-selected capture journal and recording roots; tool arguments cannot redirect output. |
| State | Distinguish launch pending, verified process, initialized MCP, authorized task, cleanup pending, and proven terminal. An acknowledgement or timeout is not terminal proof. |
| Exit | Revoke the native task, settle restoration/recording, close the channel, and confirm the original process exit. Forced process exit alone does not prove restoration. |
| Cancellation | Permission withdrawal, expiry, interruption, EOF, and parent failure must stop new actions and retain the same cleanup owner until settlement. Never replay an uncertain mutation. |
| Unknown cleanup | Preserve primary/cleanup errors and the original process/task owner. Do not release a resource or start a replacement while terminal or restoration status is unknown. |
| Windows acceptance | Exercise the real native supervisor through the public task route, including refusal, restoration, cancellation, EOF, and parent termination. Verify the original handles and actual target/output, not serialized identity claims. |

## Validation limits

The mocked tests exercise client initialization, task start/stop, protocol identity
refusal, EOF, exclusive-child cleanup, native image/birth/Job checks, and
preservation of unknown cleanup. Process pipes and Win32 APIs are fixtures, not
proof of a real independent worker.

`tests/test_cua_mcp_managed_process_native.py` separately launches one ordinary
Python child with real Win32 APIs and its original `Popen` handle. It changes
no Job policy and performs no UI operation. In a Job, it verifies explicit
refusal and the same handle's signaled state and exit code. Outside a Job, it
checks full birth, actual image/hash, and normal EOF exit. Its JUnit properties
record the branch actually exercised; passing one branch does not certify the
other. It is not a DCC-CUA native supervisor or capture acceptance test.

Native product integration, permission
revocation, an actual HUD PNG, audible output, and continuous recording remain
separate acceptance gates. Do not install this preparatory interface as a claim
that those gates have passed.
