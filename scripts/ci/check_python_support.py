"""Fail when packaging, CI, or documentation drifts from Python LTS policy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any
from typing import Mapping

try:
    from .python_support_contract import ContractError
    from .python_support_contract import load_contract
    from .python_support_contract import minimum_python_spec
    from .python_support_contract import native_ci_matrix
    from .python_support_contract import repository_root
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import ContractError
    from python_support_contract import load_contract
    from python_support_contract import minimum_python_spec
    from python_support_contract import native_ci_matrix
    from python_support_contract import repository_root


def expected_fragments(contract: Mapping[str, Any]) -> dict[str, list[str]]:
    """Return required projections of the canonical contract into repo files."""
    support = contract["support"]
    build = contract["build"]
    native = build["native_py37"]
    abi3 = build["abi3"]
    toolchain = contract["test_toolchain"]

    classifiers = [f'"Programming Language :: Python :: {version}"' for version in support["tested_versions"]]
    full_matrix_versions = [
        f'"{version}"' for version in support["tested_versions"] if version != support["minimum_python"]
    ]

    fragments = {
        "CONTRIBUTING.md": [
            "Python 3.7-3.14",
            "native cp37 wheels on Linux/Windows",
        ],
        "Cargo.toml": [
            f'pyo3 = {{ version = "{build["pyo3_series"]}"',
            f'{abi3["feature"]} = ["pyo3/{abi3["feature"]}"]',
        ],
        "Cargo.lock": [
            f'name = "pyo3"\nversion = "{build["pyo3_series"]}.',
        ],
        "justfile": [
            "WHEEL_FEATURES_PY37 :=",
            "build-py37 *EXTRA:",
            "check-python-support:",
        ],
        ".github/workflows/ci.yml": [
            "python37-contract:",
            "python37-native:",
            "python37-gate:",
            "fromJSON(needs.python37-contract.outputs.native-matrix)",
            "install_python37_test_toolchain.py",
            "vx uv pip compile pyproject.toml",
            "--extra semantic",
            "--python-version 3.7",
            "for platform in windows x86_64-manylinux_2_28",
            f'python-version: "{native["python"]}"',
        ],
        ".github/workflows/build-wheels.yml": [
            "linux-py37:",
            "windows-py37:",
            "--profile lite_py37",
            "--platform any",
        ],
        ".github/workflows/release.yml": [
            "uses: ./.github/workflows/build-wheels.yml",
            "needs.build-wheels.result == 'success'",
            "ref: ${{ github.workflow_sha }}",
            'profile="semantic_native_py37"',
            'profile="semantic_abi3"',
        ],
        ".github/workflows/python-matrix-full.yml": full_matrix_versions,
        ".github/actions/build-wheel/action.yml": [
            "check_python_wheel.py",
            "smoke_python37_runtime.py --profile native_py37",
        ],
    }
    for distribution in contract["distributions"].values():
        fragments[distribution["pyproject"]] = [
            f'requires-python = "{minimum_python_spec(dict(contract))}"',
            *classifiers,
        ]
    fragments["pyproject.toml"].extend(
        [
            f'python_version = "{support["minimum_python"]}"',
            f'target-version = "py{support["minimum_python"].replace(".", "")}"',
            f"pytest=={toolchain['pytest']},<8.0; python_version<'3.8'",
            f"pytest-xdist=={toolchain['pytest_xdist']}; python_version<'3.8'",
            f"typing-extensions=={toolchain['typing_extensions']}; python_version<'3.8'",
        ]
    )
    return fragments


_REQUIRES_PYTHON = re.compile(r"requires-python\s*=\s*['\"]([^'\"]+)['\"]", re.IGNORECASE)
_PYTHON_CLASSIFIER = re.compile(r"Programming Language :: Python :: (3[.]\d+)")
_PY37 = re.compile(r"(?:Python\s*3[.]7|py37)", re.IGNORECASE)
_ISO_DATE = re.compile(r"\b20\d{2}-\d{2}-\d{2}\b")
_EXPIRY_LANGUAGE = re.compile(
    r"(?:until|through|eol|expir(?:e|y)|sunset|deprecat(?:e|ed|ion)|"
    r"drop(?:ped)?|remov(?:e|ed|al)|end(?:s|ed)?\s+(?:on|at)|"
    r"支持至|支持到|截止|终止|弃用|移除)",
    re.IGNORECASE,
)


def _strip_unquoted_comment(line: str) -> str:
    """Remove a TOML/YAML comment while preserving hashes inside strings."""
    quote = ""
    escaped = False
    for index, character in enumerate(line):
        if escaped:
            escaped = False
            continue
        if character == "\\" and quote == '"':
            escaped = True
            continue
        if quote:
            if character == quote:
                quote = ""
            continue
        if character in ("'", '"'):
            quote = character
        elif character == "#":
            return line[:index].rstrip()
    return line.rstrip()


def _active_config_text(text: str) -> str:
    """Drop config comments so required fragments cannot live in comments."""
    return "\n".join(_strip_unquoted_comment(line) for line in text.splitlines())


def _toml_section(text: str, name: str) -> str:
    """Return active lines from one exact TOML section."""
    active = False
    lines: list[str] = []
    for raw_line in text.splitlines():
        line = _strip_unquoted_comment(raw_line)
        section = re.fullmatch(r"\s*\[([^]]+)]\s*", line)
        if section is not None:
            active = section.group(1).strip() == name
            continue
        if active and line.strip():
            lines.append(line)
    return "\n".join(lines)


def _project_requires_python(text: str) -> str | None:
    project = _toml_section(text, "project")
    matches = re.findall(
        r"(?m)^\s*requires-python\s*=\s*(?P<quote>['\"])(?P<value>[^'\"]+)(?P=quote)\s*$",
        project,
    )
    return matches[0][1] if len(matches) == 1 else None


def _project_python_classifiers(text: str) -> set[str]:
    project = _toml_section(text, "project")
    block = re.search(r"(?ms)^\s*classifiers\s*=\s*\[(.*?)]", project)
    return set(_PYTHON_CLASSIFIER.findall(block.group(1))) if block is not None else set()


def _projected_files(root: Path, patterns: list[str]) -> list[Path]:
    """Resolve projection globs once and return stable repository paths."""
    paths: set[Path] = set()
    for pattern in patterns:
        paths.update(path for path in root.glob(pattern) if path.is_file())
    return sorted(paths)


def collect_distribution_projection_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Validate every released distribution and compatibility-sensitive extra."""
    errors: list[str] = []
    expected_python = minimum_python_spec(dict(contract))
    expected_classifiers = set(contract["support"]["tested_versions"])
    distribution_text: dict[str, str] = {}
    for name, distribution in contract["distributions"].items():
        relative = distribution["pyproject"]
        path = root / relative
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            errors.append(f"{relative}: cannot read distribution metadata: {exc}")
            continue
        distribution_text[name] = text
        actual_python = _project_requires_python(text)
        if actual_python != expected_python:
            errors.append(f"{relative}: requires-python is {actual_python!r}, expected {expected_python!r}")
        actual_classifiers = _project_python_classifiers(text)
        if actual_classifiers != expected_classifiers:
            missing = sorted(expected_classifiers - actual_classifiers)
            unexpected = sorted(actual_classifiers - expected_classifiers)
            errors.append(f"{relative}: Python classifiers drifted; missing={missing!r}, unexpected={unexpected!r}")

    semantic_policy = contract["optional_dependencies"]["semantic"]
    owner = semantic_policy["owner_distribution"]
    owner_text = _toml_section(distribution_text.get(owner, ""), "project.optional-dependencies")
    semantic_block = re.search(
        r"(?ms)^semantic\s*=\s*\[(.*?)^\]",
        owner_text,
    )
    requirements = []
    if semantic_block is not None:
        entry = re.compile(r"(?m)^\s*(?P<quote>['\"])(?P<value>.*)(?P=quote)\s*,?\s*$")
        requirements = [match.group("value") for match in entry.finditer(semantic_block.group(1))]
    by_name: dict[str, str] = {}
    for requirement in requirements:
        match = re.match(r"([A-Za-z0-9_.-]+)", requirement)
        if match:
            by_name[match.group(1).lower().replace("_", "-")] = requirement

    native_name = semantic_policy["native_dependency"].lower().replace("_", "-")
    if native_name not in by_name:
        errors.append("pyproject.toml: semantic extra must include dcc-mcp-core-semantic")
    fallback_name = semantic_policy["python_fallback_dependency"].lower().replace("_", "-")
    fallback = by_name.get(fallback_name)
    minimum = semantic_policy["fallback_minimum_python"]
    if fallback is None:
        errors.append("pyproject.toml: semantic extra must include the fastembed fallback")
    else:
        _, separator, marker = fallback.partition(";")
        normalized_marker = re.sub(r"\s+", "", marker).replace('"', "'").lower()
        expected_marker = f"python_version>='{minimum}'"
        if not separator or normalized_marker != expected_marker:
            errors.append(
                "pyproject.toml: fastembed fallback must be inactive below "
                f"Python {minimum}; expected marker {expected_marker!r}"
            )
    return errors


def collect_document_projection_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Reject stale core requires-python snippets across agent and user docs."""
    errors: list[str] = []
    projections = contract["projections"]
    exemptions = {Path(value).as_posix() for value in projections["requires_python_exemptions"]}
    expected = minimum_python_spec(dict(contract))
    for path in _projected_files(root, projections["requires_python_globs"]):
        relative = path.relative_to(root).as_posix()
        if relative in exemptions:
            continue
        text = path.read_text(encoding="utf-8")
        for match in _REQUIRES_PYTHON.finditer(text):
            if match.group(1) != expected:
                line = text.count("\n", 0, match.start()) + 1
                errors.append(
                    f"{relative}:{line}: documented requires-python is {match.group(1)!r}, expected {expected!r}"
                )
    return errors


def calendar_expiry_lines(text: str) -> list[int]:
    """Return paragraph start lines that impose a dated Python 3.7 expiry."""
    lines: list[int] = []
    offset = 0
    for block in re.split(r"\n\s*\n", text):
        if _PY37.search(block) and _ISO_DATE.search(block) and _EXPIRY_LANGUAGE.search(block):
            lines.append(text.count("\n", 0, offset) + 1)
        offset += len(block)
        separator = re.match(r"\n\s*\n", text[offset:])
        if separator:
            offset += len(separator.group(0))
    return lines


def collect_calendar_policy_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Reject any automatic calendar expiry, regardless of the chosen date."""
    errors: list[str] = []
    patterns = contract["projections"]["calendar_policy_globs"]
    for path in _projected_files(root, patterns):
        text = path.read_text(encoding="utf-8")
        relative = path.relative_to(root).as_posix()
        for line in calendar_expiry_lines(text):
            errors.append(f"{relative}:{line}: calendar-based Python 3.7 expiry contradicts LTS policy")
    return errors


def _workflow_job_block(text: str, job_name: str) -> str:
    """Return one top-level GitHub Actions job block without parsing expressions."""
    match = re.search(rf"(?m)^  {re.escape(job_name)}:\s*$", text)
    if match is None:
        return ""
    next_job = re.search(r"(?m)^  [A-Za-z0-9_-]+:\s*$", text[match.end() :])
    end = match.end() + next_job.start() if next_job is not None else len(text)
    return text[match.start() : end]


def _workflow_steps(job_block: str) -> list[str]:
    """Split active job YAML into top-level step blocks."""
    starts = list(re.finditer(r"(?m)^      - (?=(?:name|uses|id):)", job_block))
    return [
        job_block[match.start() : starts[index + 1].start() if index + 1 < len(starts) else len(job_block)]
        for index, match in enumerate(starts)
    ]


def _workflow_tooling_checkout(job_block: str) -> str:
    """Return the unique checkout step bound to ``.workflow-tools``."""
    matches = [step for step in _workflow_steps(job_block) if re.search(r"(?m)^\s+path:\s*[.]workflow-tools\s*$", step)]
    return matches[0] if len(matches) == 1 else ""


def _tooling_checkout_is_current(step: str) -> bool:
    required = (
        r"(?m)^\s+uses:\s*actions/checkout@[^\s]+\s*$",
        r"(?m)^\s+ref:\s*\$\{\{ github[.]workflow_sha }}\s*$",
        r"(?m)^\s+path:\s*[.]workflow-tools\s*$",
        r"(?m)^\s+compatibility\s*$",
        r"(?m)^\s+scripts/ci\s*$",
        r"(?m)^\s+persist-credentials:\s*false\s*$",
    )
    return all(re.search(pattern, step) is not None for pattern in required)


def collect_full_matrix_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Ensure old interpreters consume a prebuilt ABI3 wheel, never rebuild it."""
    relative = ".github/workflows/python-matrix-full.yml"
    try:
        text = (root / relative).read_text(encoding="utf-8")
    except OSError as exc:
        return [f"{relative}: cannot read workflow: {exc}"]
    text = _active_config_text(text)

    errors: list[str] = []
    build = _workflow_job_block(text, "build-wheel")
    test = _workflow_job_block(text, "python-test-full")
    if not build:
        errors.append(f"{relative}: missing build-wheel job")
    else:
        for runner in ("ubuntu-latest", "windows-2022", "macos-latest"):
            if build.count(f"os: {runner}") != 1:
                errors.append(f"{relative}: build-wheel must include {runner} exactly once")
        if build.count("uses: ./.github/actions/build-wheel") != 1:
            errors.append(f"{relative}: build-wheel must invoke the reusable wheel action once")
        if "maturin-extra-args: ${{ matrix.maturin_args }}" not in build:
            errors.append(f"{relative}: build-wheel must forward per-platform maturin arguments")
        windows_row = re.search(
            r"(?ms)- os: windows-2022\s+artifact: python-matrix-wheel-windows\s+"
            r"(?:#[^\n]*\n\s*)*maturin_args: \"\"",
            build,
        )
        if windows_row is None:
            errors.append(f"{relative}: Windows full-matrix wheel must disable --find-interpreter")
        if 'generate-stubs: "false"' not in build:
            errors.append(f"{relative}: reusable full-matrix wheels must not regenerate stubs")
        for artifact in (
            "python-matrix-wheel-linux",
            "python-matrix-wheel-windows",
            "python-matrix-wheel-macos",
        ):
            if build.count(f"artifact: {artifact}") != 1:
                errors.append(f"{relative}: build-wheel must declare artifact {artifact!r} exactly once")

    if not test:
        errors.append(f"{relative}: missing python-test-full job")
    else:
        required = (
            "needs: build-wheel",
            "uses: actions/download-artifact@",
            "pip install dist/*.whl",
            "python -m pytest tests/ -q --tb=short --show-capture=no -n 4 --dist loadfile",
        )
        for fragment in required:
            if fragment not in test:
                errors.append(f"{relative}: python-test-full missing {fragment!r}")
        artifact_mappings = (
            "startsWith(matrix.os, 'windows') && 'python-matrix-wheel-windows'",
            "matrix.os == 'macos-latest' && 'python-matrix-wheel-macos'",
            "|| 'python-matrix-wheel-linux'",
        )
        for mapping in artifact_mappings:
            if mapping not in test:
                errors.append(f"{relative}: python-test-full artifact mapping missing {mapping!r}")
        forbidden = ("vx just", "vx just build", "just build", "stubgen")
        for fragment in forbidden:
            if fragment.lower() in test.lower():
                errors.append(f"{relative}: python-test-full must not run {fragment!r}")

    expected_versions = contract["support"]["tested_versions"][1:]
    version_matrix = re.search(r"(?m)^\s+python-version:\s*\[([^\]\n]+)\]\s*$", test)
    actual_versions = re.findall(r'"(3[.]\d+)"', version_matrix.group(1)) if version_matrix else []
    if actual_versions != expected_versions:
        errors.append(f"{relative}: python-test-full versions are {actual_versions!r}, expected {expected_versions!r}")
    return errors


def collect_wheel_action_errors(root: Path) -> list[str]:
    """Lock the reusable core wheel build to the legacy Linux ABI floor."""
    relative = ".github/actions/build-wheel/action.yml"
    try:
        text = (root / relative).read_text(encoding="utf-8")
    except OSError as exc:
        return [f"{relative}: cannot read action: {exc}"]
    text = _active_config_text(text)
    errors: list[str] = []
    manylinux = re.search(r"(?m)^\s*manylinux:\s*(.+?)\s*$", text)
    manylinux_value = manylinux.group(1) if manylinux is not None else ""
    pins_legacy_linux = bool(
        re.fullmatch(r"['\"]?2014['\"]?", manylinux_value)
        or re.search(
            r"runner[.]os\s*==\s*['\"]Linux['\"]\s*&&\s*['\"]2014['\"]",
            manylinux_value,
        )
    )
    if not pins_legacy_linux:
        errors.append(f"{relative}: core Linux wheels must target manylinux2014 / glibc 2.17")
    if re.search(r"(?m)^\s*manylinux:\s*auto\s*$", text):
        errors.append(f"{relative}: manylinux auto may raise the legacy DCC glibc baseline")
    for fragment in (
        "--platform",
        "linux-x86_64",
        "windows-x86_64",
        'runner.os }}" == "Windows"',
        'maturin_args=""',
    ):
        if fragment not in text:
            errors.append(f"{relative}: wheel validation missing {fragment!r}")
    return errors


def collect_backfill_tooling_errors(root: Path) -> list[str]:
    """Require every historical-source wheel job to use workflow-owned gates."""
    relative = ".github/workflows/build-wheels.yml"
    try:
        text = (root / relative).read_text(encoding="utf-8")
    except OSError as exc:
        return [f"{relative}: cannot read workflow: {exc}"]
    text = _active_config_text(text)
    errors: list[str] = []
    jobs = ("linux", "windows", "py37-lite", "linux-py37", "windows-py37", "macos")
    for job_name in jobs:
        block = _workflow_job_block(text, job_name)
        if not block:
            errors.append(f"{relative}: missing {job_name} job")
            continue
        tooling_step = _workflow_tooling_checkout(block)
        if not tooling_step or not _tooling_checkout_is_current(tooling_step):
            errors.append(
                f"{relative}: {job_name} must checkout compatibility and scripts/ci from github.workflow_sha "
                "with path: .workflow-tools"
            )
        if ".workflow-tools/scripts/ci/check_python_wheel.py" not in block:
            errors.append(f"{relative}: {job_name} missing workflow-owned wheel validator")
        build_at = block.find("uses: ./.github/actions/build-wheel")
        if job_name == "py37-lite":
            build_at = block.find("python scripts/build_py37_pure_wheel.py")
        tooling_at = block.find(tooling_step) if tooling_step else -1
        validation_at = block.find(".workflow-tools/scripts/ci/check_python_wheel.py")
        if min(build_at, tooling_at, validation_at) < 0 or not build_at < tooling_at < validation_at:
            errors.append(f"{relative}: {job_name} must build source before fetching and running workflow tooling")
    for job_name in ("py37-lite", "linux-py37", "windows-py37"):
        block = _workflow_job_block(text, job_name)
        if "if [[ -f compatibility/python.json ]]" not in block:
            errors.append(f"{relative}: {job_name} must retain the pre-contract backfill smoke path")
    return errors


def collect_semantic_release_errors(root: Path) -> list[str]:
    """Keep every semantic release wheel behind the current contract gate."""
    relative = ".github/workflows/release.yml"
    try:
        text = (root / relative).read_text(encoding="utf-8")
    except OSError as exc:
        return [f"{relative}: cannot read workflow: {exc}"]
    text = _active_config_text(text)
    block = _workflow_job_block(text, "build-semantic-wheels")
    if not block:
        return [f"{relative}: missing build-semantic-wheels job"]
    errors: list[str] = []
    tooling_step = _workflow_tooling_checkout(block)
    if not tooling_step or not _tooling_checkout_is_current(tooling_step):
        errors.append(
            f"{relative}: semantic release must checkout compatibility and scripts/ci from "
            "github.workflow_sha with path: .workflow-tools"
        )
    required = (
        ".workflow-tools/scripts/ci/check_python_wheel.py",
        'profile="semantic_abi3"',
        'profile="semantic_native_py37"',
        'platform="linux-x86_64"',
        'platform="windows-x86_64"',
        'platform="macos-native"',
    )
    for fragment in required:
        if fragment not in block:
            errors.append(f"{relative}: semantic release gate missing {fragment!r}")
    build_at = max(block.rfind("maturin build"), block.rfind("build_manylinux_semantic_wheel.sh"))
    tooling_at = block.find(tooling_step) if tooling_step else -1
    validation_at = block.find(".workflow-tools/scripts/ci/check_python_wheel.py")
    upload_at = block.find("name: Upload semantic wheel artefact")
    if min(build_at, tooling_at, validation_at, upload_at) < 0 or not build_at < tooling_at < validation_at < upload_at:
        errors.append(f"{relative}: semantic wheels must build, fetch current tooling, validate, then upload")
    return errors


def collect_expected_fragment_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Check required projections without accepting commented-out config."""
    errors: list[str] = []
    for relative, fragments in expected_fragments(contract).items():
        path = root / relative
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            errors.append(f"{relative}: cannot read file: {exc}")
            continue
        if path.suffix.lower() in {".toml", ".yml", ".yaml", ".lock"} or path.name == "justfile":
            text = _active_config_text(text)
        for fragment in fragments:
            if fragment not in text:
                errors.append(f"{relative}: missing contract projection {fragment!r}")
    return errors


def collect_projection_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Collect all contract drift errors without failing at the first one."""
    errors = collect_expected_fragment_errors(root, contract)

    errors.extend(collect_distribution_projection_errors(root, contract))
    errors.extend(collect_document_projection_errors(root, contract))
    errors.extend(collect_calendar_policy_errors(root, contract))
    errors.extend(collect_full_matrix_errors(root, contract))
    errors.extend(collect_wheel_action_errors(root))
    errors.extend(collect_backfill_tooling_errors(root))
    errors.extend(collect_semantic_release_errors(root))

    release_jobs = {
        "linux-x86_64": "linux-py37",
        "windows-x86_64": "windows-py37",
    }
    build_workflow = (root / ".github/workflows/build-wheels.yml").read_text(encoding="utf-8")
    for row in contract["build"]["native_py37"]["pr_matrix"]:
        job_name = release_jobs[row["platform"]]
        marker = f"  {job_name}:\n"
        start = build_workflow.find(marker)
        if start < 0:
            errors.append(f".github/workflows/build-wheels.yml: missing {job_name} job")
            continue
        next_job = build_workflow.find("\n  ", start + len(marker))
        while next_job >= 0:
            line = build_workflow[next_job + 1 :].split("\n", 1)[0]
            if line.startswith("  ") and not line.startswith("    ") and line.endswith(":"):
                break
            next_job = build_workflow.find("\n  ", next_job + 3)
        block = build_workflow[start : next_job if next_job >= 0 else len(build_workflow)]
        for fragment in (
            f"runs-on: {row['runner']}",
            f"target: {row['target']}",
            "python-version: '3.7'",
        ):
            if fragment not in block:
                errors.append(f".github/workflows/build-wheels.yml: {job_name} missing {fragment!r}")
    return errors


def main(argv: list[str] | None = None) -> int:
    """Validate the contract and every critical projection used by CI/release."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--github-output",
        type=Path,
        help="append the generated native matrix to this GitHub Actions output file",
    )
    args = parser.parse_args(argv)
    root = repository_root()
    try:
        contract = load_contract(root)
    except ContractError as exc:
        sys.stderr.write(f"python-support-contract: {exc}\n")
        return 1

    errors = collect_projection_errors(root, contract)
    if errors:
        for error in errors:
            sys.stderr.write(f"python-support-contract: {error}\n")
        return 1

    if args.github_output is not None:
        matrix = json.dumps(native_ci_matrix(contract), separators=(",", ":"), sort_keys=True)
        with args.github_output.open("a", encoding="utf-8") as stream:
            stream.write(f"native-matrix={matrix}\n")

    minimum = contract["support"]["minimum_python"]
    maximum = contract["support"]["maximum_tested_python"]
    sys.stdout.write(f"python-support-contract: OK (Python {minimum}-{maximum}, native py37 on Linux + Windows)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
