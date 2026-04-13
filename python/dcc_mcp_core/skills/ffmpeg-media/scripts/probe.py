"""Probe media file metadata using ffprobe.

Extracts resolution, codec, duration, bitrate, and other metadata
from any media file supported by FFmpeg.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Extract media metadata via ffprobe."""
    parser = argparse.ArgumentParser(description="Probe media file metadata.")
    parser.add_argument("--input", required=True, dest="input_file")
    args = parser.parse_args()

    try:
        result = subprocess.run(
            [
                "ffprobe",
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                args.input_file,
            ],
            capture_output=True,
            timeout=30,
            encoding="utf-8",
        )

        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"ffprobe failed: {result.stderr.strip()}",
                    }
                )
            )
            sys.exit(1)

        probe_data = json.loads(result.stdout)

        streams = probe_data.get("streams", [])
        fmt = probe_data.get("format", {})
        video_stream = next((s for s in streams if s.get("codec_type") == "video"), None)
        audio_stream = next((s for s in streams if s.get("codec_type") == "audio"), None)

        info = {
            "filename": fmt.get("filename"),
            "duration": float(fmt.get("duration", 0)),
            "size_bytes": int(fmt.get("size", 0)),
            "format_name": fmt.get("format_name"),
            "bit_rate": int(fmt.get("bit_rate", 0)),
        }

        if video_stream:
            info["video"] = {
                "codec": video_stream.get("codec_name"),
                "width": video_stream.get("width"),
                "height": video_stream.get("height"),
                "fps": video_stream.get("r_frame_rate"),
            }

        if audio_stream:
            info["audio"] = {
                "codec": audio_stream.get("codec_name"),
                "sample_rate": audio_stream.get("sample_rate"),
                "channels": audio_stream.get("channels"),
            }

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Probed {args.input_file}: {info.get('duration', 0):.1f}s",
                    "context": info,
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "ffprobe not found. Install FFmpeg first.",
                    "context": {"possible_solutions": ["brew install ffmpeg", "apt install ffmpeg"]},
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
