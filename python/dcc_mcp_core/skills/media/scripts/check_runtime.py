"""Read-only, bounded media runtime preflight with reproducibility evidence."""

from __future__ import annotations

from pathlib import Path
import re
import time

from _media_audio import BINARY_LIMIT
from _media_audio import _hash_file
from _media_audio import _resolve_binaries
from _media_audio_process import run_audio_command
from _media_common import MediaToolError
from _media_common import emit
from _media_common import int_value
from _media_common import read_params
from _media_common import run_tool
from _media_common import skill_success
from _media_runtime import encoder_names


def check_runtime(required_encoders=None, timeout_secs=30):
    """Identify installed tools and required capabilities without provisioning."""
    required = ["pcm_s16le", "libvorbis", "pcm_f32le"] if required_encoders is None else required_encoders
    if (
        not isinstance(required, list)
        or len(required) > 32
        or any(
            not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]{0,79}", name) for name in required
        )
    ):
        raise MediaToolError("required_encoders must be a list of at most 32 encoder names.", "invalid_input")
    if isinstance(timeout_secs, bool):
        raise MediaToolError("Timeout must be an integer.", "invalid_input")
    timeout = int_value("timeout_secs", timeout_secs, 30, minimum=1, maximum=60)
    deadline = time.monotonic() + timeout
    commands = []

    def execute(argv):
        commands.append(argv)
        return run_audio_command(argv, deadline)

    binaries = _resolve_binaries(execute, deadline)
    available = encoder_names(execute([binaries["ffmpeg"]["path"], "-hide_banner", "-encoders"])["stdout"])
    for binary in binaries.values():
        if _hash_file(Path(binary["path"]), deadline, BINARY_LIMIT) != binary["sha256"]:
            raise MediaToolError("Resolved executable changed during preflight.", "executable_changed")
    missing = sorted(set(required) - available)
    context = dict(
        executables=binaries, available_encoders=sorted(available), required_encoders=required, commands=commands
    )
    if missing:
        raise MediaToolError(
            "Required encoders are unavailable: " + ", ".join(missing),
            "encoder_unavailable",
            prompt="Preinstall a build with the required codecs and point the runtime override at that build.",
            context=context,
        )
    return skill_success("Installed media runtime is ready for the requested encoders.", **context)


def main(**params):
    """Run the typed runtime preflight."""
    return run_tool(check_runtime, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
