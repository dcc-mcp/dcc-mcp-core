"""Build-wheel workflow release publication tests."""

from __future__ import annotations

from fnmatch import fnmatch

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

BUILD_WHEELS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "build-wheels.yml"
BUILD_JOB_IDS = {
    "linux",
    "windows",
    "py37-lite",
    "linux-py37",
    "windows-py37",
    "macos-py37",
    "macos",
}
# The macOS cp37 wheel stays a build-verification spike rather than a release
# artefact, so it trails both `publish-release.needs` and the published artifact
# list. It is withheld by scope, not because it is broken: the lane builds it,
# passes the native_py37 contract, and pip-installs it into python.org 3.7.9 on
# macOS 10.12+ (pip compares the wheel tag against the running OS version, not
# the interpreter's MACOSX_DEPLOYMENT_TARGET). Promoting it to a release
# artefact is a product decision, not a bug fix.
PUBLISH_JOB_IDS = BUILD_JOB_IDS - {"macos-py37"}
PUBLISH_ARTIFACT_NAMES = {
    "wheels-linux-x86_64",
    "wheels-windows-x64",
    "wheels-py37-lite",
    "wheels-linux-x86_64-py37",
    "wheels-windows-x64-py37",
    "wheels-macos-universal2",
}


def _jobs() -> dict:
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def test_manual_backfill_reuses_verified_github_release_assets() -> None:
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    reuse_input = workflow["on"]["workflow_call"]["inputs"]["reuse-release-assets"]
    assert reuse_input == {
        "description": "Reuse immutable dcc-mcp-core distributions from an existing GitHub Release",
        "required": False,
        "type": "boolean",
        "default": False,
    }

    jobs = workflow["jobs"]
    reuse = jobs["reuse-release-assets"]
    assert reuse["if"] == "needs.classify-release-assets.outputs.mode == 'reuse'"
    assert reuse["permissions"] == {"contents": "read"}
    commands = "\n".join(step.get("run", "") for step in reuse["steps"])
    assert "gh release download" in commands
    assert "dcc_mcp_core-${RELEASE_VERSION}-*" in commands
    assert "dcc_mcp_core-${RELEASE_VERSION}.tar.gz" in commands
    assert "check_release_distribution_set.py" in commands
    checkout = next(step for step in reuse["steps"] if step.get("uses") == "actions/checkout@v6")
    assert checkout["with"]["ref"] == "${{ github.workflow_sha }}"
    assert "compatibility" in checkout["with"]["sparse-checkout"]
    assert "scripts/ci" in checkout["with"]["sparse-checkout"]
    upload = next(step for step in reuse["steps"] if step.get("uses") == "actions/upload-artifact@v7")
    assert upload["with"] == {
        "name": "wheels-release-backfill",
        "path": "dist/",
        "retention-days": 30,
    }


def test_manual_backfill_rebuilds_from_tag_when_release_has_no_core_assets() -> None:
    jobs = _jobs()
    classifier = jobs["classify-release-assets"]
    assert classifier["if"] == "inputs.reuse-release-assets == true"
    assert classifier["permissions"] == {"contents": "read"}
    assert classifier["outputs"] == {"mode": "${{ steps.classify.outputs.mode }}"}
    classifier_commands = "\n".join(step.get("run", "") for step in classifier["steps"])
    assert "gh release view" in classifier_commands
    assert "--classify-assets" in classifier_commands
    assert 'if [ "$CHECKOUT_REF" != "$RELEASE_TAG" ]' in classifier_commands
    release_read = next(
        step for step in classifier["steps"] if step.get("name") == "Read dcc-mcp-core GitHub Release assets"
    )
    assert release_read["env"]["CHECKOUT_REF"] == "${{ inputs.checkout-ref }}"
    classifier_checkout = next(step for step in classifier["steps"] if step.get("uses") == "actions/checkout@v6")
    assert classifier_checkout["with"]["ref"] == "${{ github.workflow_sha }}"

    build_condition = (
        "always() && (inputs.reuse-release-assets != true || needs.classify-release-assets.outputs.mode == 'rebuild')"
    )
    for job_id in BUILD_JOB_IDS:
        assert jobs[job_id]["needs"] == ["classify-release-assets"]
        assert jobs[job_id]["if"] == build_condition

    reuse = jobs["reuse-release-assets"]
    assert reuse["needs"] == ["classify-release-assets"]
    assert reuse["if"] == "needs.classify-release-assets.outputs.mode == 'reuse'"


def test_release_wheels_are_uploaded_once_after_every_build() -> None:
    jobs = _jobs()

    for job_id in BUILD_JOB_IDS:
        steps = jobs[job_id]["steps"]
        assert all(step.get("uses") != "softprops/action-gh-release@v3" for step in steps)

    publish = jobs["publish-release"]
    assert set(publish["needs"]) == PUBLISH_JOB_IDS
    assert publish["if"] == "inputs.release-tag-name != ''"
    assert publish["permissions"] == {"actions": "read", "contents": "write"}

    # The macOS cp37 wheel is a build-verification spike, not a release
    # artefact, so `publish-release` downloads each published wheel by name
    # instead of globbing `wheels-*`. The spike is uploaded as
    # `spike-wheels-macos-x86_64-py37`, deliberately outside the `wheels-*`
    # namespace, so publication globs cannot sweep it into the distribution
    # set even if this allowlist is ever relaxed.
    downloads = [step for step in publish["steps"] if step.get("uses") == "actions/download-artifact@v8"]
    assert downloads
    downloaded_names = set()
    for step in downloads:
        with_ = step["with"]
        assert "pattern" not in with_
        assert with_["path"] == "dist"
        assert with_["merge-multiple"] is True
        downloaded_names.add(with_["name"])
    assert downloaded_names == PUBLISH_ARTIFACT_NAMES

    validate_index = next(
        index
        for index, step in enumerate(publish["steps"])
        if "check_release_distribution_set.py" in step.get("run", "")
    )
    upload_index = next(
        index for index, step in enumerate(publish["steps"]) if step.get("uses") == "softprops/action-gh-release@v3"
    )
    assert validate_index < upload_index
    validation = publish["steps"][validate_index]
    assert "--dist-dir dist" in validation["run"]
    assert "--version" in validation["run"]

    upload = publish["steps"][upload_index]
    assert upload["uses"] == "softprops/action-gh-release@v3"
    # The default GITHUB_TOKEN is refused on the release-update call that
    # precedes every asset upload, so release.yml passes the
    # release PAT through the reusable-workflow secret.
    assert upload["with"] == {
        "token": "${{ secrets.RELEASE_TOKEN || github.token }}",
        "tag_name": "${{ inputs.release-tag-name }}",
        "files": "dist/*",
        # Replacing a same-named asset is opt-in: the caller decides, because
        # only the asset backfill may overwrite an existing release.
        "overwrite_files": "${{ inputs.overwrite-release-assets }}",
        "fail_on_unmatched_files": True,
    }


def test_verification_spike_producer_stays_outside_publication_globs() -> None:
    jobs = _jobs()
    build = next(step for step in jobs["macos-py37"]["steps"] if step.get("uses") == "./.github/actions/build-wheel")
    artifact_name = build["with"]["artifact-name"]
    assert artifact_name not in PUBLISH_ARTIFACT_NAMES

    release = yaml_loads((REPO_ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8"))
    for job_id in ("publish-core-pypi", "publish-github-release-assets"):
        download = next(
            step
            for step in release["jobs"][job_id]["steps"]
            if step.get("name") == "Download dcc-mcp-core wheel artefacts"
        )
        assert not fnmatch(artifact_name, download["with"]["pattern"]), job_id


def test_callable_workflow_declares_the_release_token_secret() -> None:
    """Reusable workflows do not inherit secrets, so it must be declared."""
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    secrets = workflow["on"]["workflow_call"]["secrets"]
    assert secrets["RELEASE_TOKEN"]["required"] is False


def test_asset_overwrite_is_opt_in_on_every_trigger() -> None:
    """Both triggers default the overwrite input to false.

    `workflow_dispatch` matters as much as `workflow_call`: a manual run of the
    reusable workflow must not be able to clobber a published release by
    omission.
    """
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    for trigger in ("workflow_dispatch", "workflow_call"):
        overwrite = workflow["on"][trigger]["inputs"]["overwrite-release-assets"]
        assert overwrite["type"] == "boolean"
        assert overwrite["default"] is False
        assert overwrite["required"] is False


def test_python37_runtime_smokes_share_version_and_ref_bound_dependency_preparation() -> None:
    smoke_steps = {
        "py37-lite": "Test py37-lite wheel",
        "linux-py37": "Test native Python 3.7 wheel with workflow smoke",
        "windows-py37": "Test native Python 3.7 wheel with workflow smoke",
        "macos-py37": "Test native Python 3.7 wheel with workflow smoke",
    }

    for job_id, smoke_step_name in smoke_steps.items():
        steps = _jobs()[job_id]["steps"]
        smoke_index = next(index for index, step in enumerate(steps) if step.get("name") == smoke_step_name)
        smoke = steps[smoke_index]
        assert "python37_wheel_smoke.py" in smoke["run"]
        assert '--checkout-ref "$CHECKOUT_REF"' in smoke["run"]
        assert smoke["env"]["CHECKOUT_REF"] == "${{ inputs.checkout-ref || github.ref }}"
        assert "--profile " + ("lite_py37" if job_id == "py37-lite" else "native_py37") in smoke["run"]
        assert "if [[ -f" not in smoke["run"]
        assert all("test_issue_2388_zero_typing_extensions.py" not in step.get("run", "") for step in steps)


def test_py37_contract_check_keeps_macos_target_gate_off_linux_and_windows() -> None:
    """The macOS cp37 target gate must stay scoped to macOS.

    Linux and Windows pass their own `target` values (`x86_64`, `x64`), so
    reusing the Apple triple check for them rejects every non-macOS cp37 lane
    before the wheel contract ever runs.
    """
    action = yaml_loads((REPO_ROOT / ".github" / "actions" / "build-wheel" / "action.yml").read_text(encoding="utf-8"))
    contract = next(
        step for step in action["runs"]["steps"] if step.get("name") == "Check Python compatibility wheel contract"
    )
    run = contract["run"]

    # The Apple-triple guard is the only thing that may reference the triple.
    assert "x86_64-apple-darwin" in run
    # Linux and Windows fall through without touching `inputs.target`.
    linux_windows_branch = "Linux|Windows) ;;"
    assert linux_windows_branch in run
    # The macOS target check must sit inside the macOS arm of the *py37* case,
    # never in an unguarded or Windows-reachable position. Scope the search to
    # the py37 block: the abi3 block above also has a macOS arm.
    py37_block = run.split('profile="native_py37"')[1]
    macos_arm = py37_block.split("macOS)")[1].split(";;")[0]
    assert "x86_64-apple-darwin" in macos_arm
    # The py37 case must not assign a platform on Linux/Windows at all.
    assert 'platform="linux-x86_64"' not in py37_block
    assert 'platform="windows-x86_64"' not in py37_block


def test_py37_wheel_test_creates_venv_with_the_path_python() -> None:
    """The py37 venv must be created by the interpreter the setup step put on PATH.

    Each py37 lane installs its own interpreter (Windows via a python.org
    package, macOS via Rosetta 2). The Windows lane does expose `python3.7`, but
    only as a renamed copy of python.exe, and CPython 3.7 venv resolves the
    interpreter through sys._base_executable. A renamed copy makes that lookup
    fail with "The system cannot find the file specified" while the build and
    every direct `python3.7 <script>` call still succeed.
    """
    action = yaml_loads((REPO_ROOT / ".github" / "actions" / "build-wheel" / "action.yml").read_text(encoding="utf-8"))
    test_wheel = next(step for step in action["runs"]["steps"] if step.get("name") == "Test wheel")
    run = test_wheel["run"]
    assert "python -m venv test-env" in run
    # No lane may force a versioned interpreter name to build the venv.
    assert "python3.7 -m venv" not in run


def test_abi3_backfill_uses_workflow_smoke_without_changing_tag_source() -> None:
    for job_id in ("linux", "windows", "macos"):
        steps = _jobs()[job_id]["steps"]
        assert steps[0]["with"]["ref"] == "${{ inputs.checkout-ref || github.ref }}"
        build_index = next(i for i, step in enumerate(steps) if step.get("uses") == "./.github/actions/build-wheel")
        assert steps[build_index]["with"]["test-wheel"] == "false"
        tooling_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Checkout workflow compatibility tooling"
        )
        tooling = steps[tooling_index]["with"]
        assert tooling["ref"] == "${{ github.workflow_sha }}"
        assert "tests/test_imports.py" in tooling["sparse-checkout"]
        smoke_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Test wheel with workflow import smoke"
        )
        assert build_index < tooling_index < smoke_index
        smoke = steps[smoke_index]["run"]
        assert "python -m venv test-env" in smoke
        assert "python -m pip install --force-reinstall dist/*.whl" in smoke
        assert "python -m pip check" in smoke
        assert "python .workflow-tools/tests/test_imports.py" in smoke
        assert "test-env/Scripts/activate" in smoke
        assert "test-env/bin/activate" in smoke


def test_native_py37_backfill_uses_workflow_owned_clean_venv() -> None:
    """An old tag must not supply the venv implementation for its own backfill."""
    for job_id, interpreter in (
        ("linux-py37", "test-env/bin/python"),
        ("windows-py37", "test-env/Scripts/python.exe"),
    ):
        steps = _jobs()[job_id]["steps"]
        assert steps[0]["with"]["ref"] == "${{ inputs.checkout-ref || github.ref }}"
        build_index = next(i for i, step in enumerate(steps) if step.get("uses") == "./.github/actions/build-wheel")
        assert steps[build_index]["with"]["test-wheel"] == "false"
        tooling_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Checkout workflow compatibility tooling"
        )
        tooling = steps[tooling_index]["with"]
        assert tooling["ref"] == "${{ github.workflow_sha }}"
        assert tooling["path"] == ".workflow-tools"
        assert tooling["persist-credentials"] is False
        assert "compatibility" in tooling["sparse-checkout"]
        assert "scripts/ci" in tooling["sparse-checkout"]
        smoke_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Test native Python 3.7 wheel with workflow smoke"
        )
        assert build_index < tooling_index < smoke_index
        smoke = steps[smoke_index]
        assert "if" not in smoke
        run = smoke["run"]
        assert "python -m venv test-env" in run
        assert "--system-site-packages" not in run
        assert "python3.7 -m venv" not in run
        assert "source " not in run
        assert "sys.version_info[:2] == (3, 7)" in run
        assert "sys.prefix != sys.base_prefix" in run
        assert interpreter + " -m pip --version" in run
        assert interpreter + " .workflow-tools/scripts/ci/python37_wheel_smoke.py" in run
        assert interpreter + " -m pip check" in run
        assert run.index("python -m venv") < run.index("python37_wheel_smoke.py") < run.index(" -m pip check")


def test_windows_py37_ci_exercises_the_real_action_venv() -> None:
    """Keep the Windows venv/ensurepip regression on the native wheel CI path."""
    ci = yaml_loads((REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8"))
    job = ci["jobs"]["python37-native"]
    build = next(step for step in job["steps"] if step.get("uses") == "./.github/actions/build-wheel")
    assert build["with"]["python-version"] == "3.7"
    assert build["with"]["test-wheel"] == "${{ matrix.platform == 'windows-x86_64' && 'true' || 'false' }}"
    action = yaml_loads((REPO_ROOT / ".github" / "actions" / "build-wheel" / "action.yml").read_text(encoding="utf-8"))
    test_wheel = next(step for step in action["runs"]["steps"] if step.get("name") == "Test wheel")
    assert test_wheel["if"] == "inputs.test-wheel == 'true'"
    assert "python -m venv test-env" in test_wheel["run"]
    assert "pip install --force-reinstall dist/*.whl" in test_wheel["run"]
    verify = next(step for step in job["steps"] if step.get("name") == "Verify Windows Python 3.7 clean venv")
    install = next(step for step in job["steps"] if step.get("name") == "Install native wheel")
    assert job["steps"].index(build) < job["steps"].index(verify) < job["steps"].index(install)
    assert verify["if"] == "runner.os == 'Windows'"
    run = verify["run"]
    assert "test-env/Scripts/python.exe -c" in run
    assert "sys.prefix != sys.base_prefix" in run
    assert "sys.version_info[:2] == (3, 7)" in run
    assert "test-env/Scripts/python.exe -m pip check" in run
    assert "test-env/Scripts/python.exe -I scripts/ci/smoke_python37_runtime.py --profile native_py37" in run


def test_native_py37_backfill_resolves_declared_dependencies_after_isolated_smoke() -> None:
    """A no-deps runtime smoke cannot satisfy a declared-dependency check."""
    for job_id, interpreter in (
        ("linux-py37", "test-env/bin/python"),
        ("windows-py37", "test-env/Scripts/python.exe"),
    ):
        smoke = next(
            step
            for step in _jobs()[job_id]["steps"]
            if step.get("name") == "Test native Python 3.7 wheel with workflow smoke"
        )
        run = smoke["run"]
        install = interpreter + " -m pip install --force-reinstall dist/*.whl"
        check = interpreter + " -m pip check"
        assert install in run.splitlines()
        assert check in run.splitlines()
        assert run.index("python37_wheel_smoke.py") < run.index(install) < run.index(check)
        assert "--no-deps" not in run
        assert "--ignore-requires-python" not in run


def test_shared_build_wheel_venv_upgrades_pip_before_installing() -> None:
    """A fresh py37 venv seeds pip 20.1.1, which rejects the macOS cp37 tag.

    `python -m venv` does not inherit the upgraded pip the py37 setup actions
    install; ensurepip seeds CPython 3.7.9's pip 20.1.1. Its vendored
    `packaging` cannot classify the cross-compiled `macosx_10_12_x86_64` tag,
    so `pip install dist/*.whl` fails with "not a supported wheel on this
    platform" even though the wheel is the right artefact for the host.
    """
    action = yaml_loads((REPO_ROOT / ".github" / "actions" / "build-wheel" / "action.yml").read_text(encoding="utf-8"))
    test_wheel = next(step for step in action["runs"]["steps"] if step.get("name") == "Test wheel")
    run = test_wheel["run"]
    # The ceiling keeps Python 3.7 installable: pip 24.1+ drops py37.
    assert 'python -m pip install --upgrade "pip<24.1" wheel' in run
    # `pip install --upgrade pip` replaces the running pip.exe and dies with
    # WinError 5 on Windows; upgrading through `python -m pip` avoids that.
    assert "pip install --upgrade pip" not in run
    install = "pip install --force-reinstall dist/*.whl"
    assert run.index("--upgrade") < run.index(install)


def test_macos_py37_smoke_upgrades_pip_before_installing() -> None:
    """The macOS lane's smoke installs through the same stale-pip interpreter."""
    smoke = next(
        step
        for step in _jobs()["macos-py37"]["steps"]
        if step.get("name") == "Test native Python 3.7 wheel with workflow smoke"
    )
    run = smoke["run"]
    assert 'python -m pip install --upgrade "pip<24.1" wheel' in run
    assert run.index("--upgrade") < run.index("python37_wheel_smoke.py")
