# Owned MCP worker launch dependency

`PixelsMcpHostClient` and `OwnedCuaMcpTransport` accept an optional,
constructor-only `managed_launcher(command, environment)` dependency. The
default process launch and public task protocol are unchanged. No environment
variable, tool argument, endpoint discovery, shared Host attachment, or lease
import enables this dependency.

This is a preparatory embedding interface. Core does not supply the launcher.
It does not fix `SupervisorNotIndependent` in an existing deployment or establish
that an execution runtime can create an independent worker.

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

## Validation limits

The tests exercise client initialization, task start/stop, protocol identity
refusal, EOF, exclusive-child cleanup, native image/birth/Job checks, and
preservation of unknown cleanup. Process pipes and Win32 APIs are fixtures, not
proof of a real independent worker. Native product integration, permission
revocation, an actual HUD PNG, audible output, and continuous recording remain
separate acceptance gates. Do not install this preparatory interface as a claim
that those gates have passed.
