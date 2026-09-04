#!/usr/bin/env python3
"""Emit the source-bound RSHR-201 gate result for HarvestCircle Steps 289/290."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import signal
import stat
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
AUTHORITY_PATH = ROOT / "contracts/rshr-201-step-gates.v1.json"
ORIGIN = "ssh://git@github.com/radrootslabs/harvestcircle.git"
BRANCH = "rshr/rcld-201"
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MAX_STREAM_BYTES = 64 * 1024 * 1024
TIMEOUT_SECONDS = 3600
GATE_DEFINITIONS = {
    289: "focused Kotlin and source-lock mutations",
    290: "focused Gradle and unsigned-package negatives",
}


class GateError(RuntimeError):
    """A fail-closed gate error."""


def canonical(value: object) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")


def sha256_bytes(contents: bytes) -> str:
    return hashlib.sha256(contents).hexdigest()


def read_regular(path: Path, maximum: int = MAX_SOURCE_BYTES) -> bytes:
    relative = path.relative_to(ROOT)
    if path.is_symlink():
        raise GateError(f"source path is a symbolic link: {relative}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > maximum:
            raise GateError(f"source path is not a bounded regular file: {relative}")
        chunks: list[bytes] = []
        remaining = maximum + 1
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        contents = b"".join(chunks)
        if len(contents) > maximum:
            raise GateError(f"source path exceeds its byte bound: {relative}")
        after = os.fstat(descriptor)
        if (metadata.st_dev, metadata.st_ino, metadata.st_size) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
        ):
            raise GateError(f"source path changed during read: {relative}")
        return contents
    finally:
        os.close(descriptor)


def run(arguments: list[str], environment: dict[str, str] | None = None) -> bytes:
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        process = subprocess.Popen(
            arguments,
            cwd=ROOT,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=stdout,
            stderr=stderr,
            start_new_session=True,
        )
        try:
            return_code = process.wait(timeout=TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as error:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=1)
            raise GateError(f"command exceeded {TIMEOUT_SECONDS}s: {arguments!r}") from error
        stdout.seek(0, os.SEEK_END)
        stderr.seek(0, os.SEEK_END)
        stdout_size = stdout.tell()
        stderr_size = stderr.tell()
        if stdout_size > MAX_STREAM_BYTES or stderr_size > MAX_STREAM_BYTES:
            raise GateError(f"command output exceeded its bound: {arguments!r}")
        stdout.seek(0)
        stderr.seek(0)
        captured_stdout = stdout.read()
        captured_stderr = stderr.read()
    if return_code:
        detail = (captured_stdout + captured_stderr)[-8192:].decode("utf-8", "replace")
        raise GateError(f"command failed ({return_code}): {arguments!r}\n{detail}")
    return captured_stdout


def git(*arguments: str) -> str:
    return run(["git", *arguments]).decode("utf-8", "strict").strip()


def require_source_state(source_revision: str, source_tree: str) -> None:
    if (
        git("rev-parse", "HEAD") != source_revision
        or git("rev-parse", "HEAD^{tree}") != source_tree
        or git("symbolic-ref", "--short", "HEAD") != BRANCH
        or git("remote", "get-url", "origin") != ORIGIN
        or git("rev-parse", f"origin/{BRANCH}") != source_revision
        or run(
            [
                "git",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
            ]
        )
    ):
        raise GateError("HarvestCircle source is not clean and tracking-exact")
    forbidden = git("ls-files", ".github", ".github/**")
    if forbidden or (ROOT / ".github").exists():
        raise GateError("forbidden .github surface is present")


def source_lock_revision() -> str:
    source_lock = read_regular(ROOT / "radroots.lib.source-lock.v1.toml", 16 * 1024)
    try:
        text = source_lock.decode("utf-8", "strict")
    except UnicodeError as error:
        raise GateError("Lib source lock is not UTF-8") from error
    values = dict(
        re.findall(r'^([a-z0-9_]+) = "([^"\r\n]+)"$', text, flags=re.MULTILINE)
    )
    if values.get("schema") != "radroots.lib.source-lock.v1":
        raise GateError("Lib source-lock schema differs")
    if values.get("lockfile") != "core/Cargo.lock":
        raise GateError("Lib source-lock path differs")
    lockfile = read_regular(ROOT / values["lockfile"])
    if values.get("lockfile_sha256") != sha256_bytes(lockfile):
        raise GateError("Lib source-lock digest differs from actual bounded bytes")
    revision = values.get("revision", "")
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise GateError("Lib source-lock revision is not canonical")
    return revision


def gate_environment(source_revision: str) -> dict[str, str]:
    allowed = {
        "CARGO_HOME",
        "EXT_BUILD_CONFIG",
        "EXT_BUILD_MACHINE_CONFIG",
        "EXT_BUILD_ROOT",
        "GRADLE_USER_HOME",
        "HOME",
        "JAVA_HOME",
        "LANG",
        "LC_ALL",
        "PATH",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "SDKROOT",
        "TMPDIR",
        "XCODE_DEVELOPER_DIR",
        "XCODE_DERIVED_DATA",
        "XCODE_PACKAGE_CACHE",
        "XCODE_SOURCE_PACKAGES",
    }
    environment = {name: value for name, value in os.environ.items() if name in allowed}
    environment.update(
        {
            "HARVESTCIRCLE_BUILD_SOURCE_COMMIT": source_revision,
            "HARVESTCIRCLE_BUILD_SOURCE_DIRTY": "false",
            "HARVESTCIRCLE_BUILD_RADROOTS_REVISION": source_lock_revision(),
            "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN": "1.97.1",
            "SOURCE_DATE_EPOCH": git("show", "-s", "--format=%ct", source_revision),
        }
    )
    return environment


def run_step(step: int, source_revision: str) -> None:
    environment = gate_environment(source_revision)
    if step == 289:
        run(
            [
                "./gradlew",
                "--offline",
                "--no-daemon",
                "-p",
                "build-logic",
                ":contracts:test",
                "--tests",
                "org.harvestcircle.buildlogic.contracts.BuildContractsTest",
            ],
            environment,
        )
        run(
            [
                "cargo",
                "+1.97.1",
                "test",
                "--offline",
                "--manifest-path",
                "tools/xtask/Cargo.toml",
                "--locked",
                "source_lock",
            ],
            environment,
        )
    elif step == 290:
        run(["tools/test-build-modes.sh"], environment)
        run(
            [
                "./gradlew",
                "--offline",
                "--no-daemon",
                "-p",
                "build-logic",
                ":contracts:test",
                ":plugins:test",
                ":plugins:functionalTest",
            ],
            environment,
        )
        run(
            [
                "./gradlew",
                "--offline",
                "--no-daemon",
                "--no-parallel",
                "--no-configuration-cache",
                ":app:desktop:unsignedReleaseReadiness",
            ],
            environment,
        )
    else:
        raise GateError(f"unsupported step: {step}")
    if run(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"]
    ):
        raise GateError("verification changed the tracked or untracked source state")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--step", type=int, required=True)
    parser.add_argument("--check-id", required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--source-tree", required=True)
    parser.add_argument("--candidate-digest", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--execution-request-sha256", required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    if arguments.step not in GATE_DEFINITIONS:
        raise GateError("step is outside the HarvestCircle gate authority")
    for value, label in (
        (arguments.check_id.removeprefix("gate-01-"), "check digest"),
        (arguments.source_revision, "source revision"),
        (arguments.source_tree, "source tree"),
        (arguments.execution_request_sha256, "execution request"),
    ):
        expected_length = 40 if label in {"source revision", "source tree"} else 64
        if re.fullmatch(rf"[0-9a-f]{{{expected_length}}}", value) is None:
            raise GateError(f"{label} is not canonical")
    if arguments.candidate_digest != "none" or arguments.platform != "macos_aarch64":
        raise GateError("candidate or platform scope differs")
    authority_bytes = read_regular(AUTHORITY_PATH, 256 * 1024)
    authority = json.loads(authority_bytes)
    if canonical(authority) + b"\n" != authority_bytes:
        raise GateError("gate authority is not canonical JSON")
    selected = [
        row
        for row in authority.get("gate_command_contract", [])
        if row.get("step") == arguments.step
    ]
    if len(selected) != 1:
        raise GateError("gate command authority is absent or duplicated")
    contract = selected[0]
    gate_digest = sha256_bytes(GATE_DEFINITIONS[arguments.step].encode("utf-8"))
    verifier_digest = sha256_bytes(read_regular(Path(__file__).resolve()))
    assertion_id = f"step_{arguments.step:03d}_gate_01_{gate_digest}"
    if (
        contract.get("check_id") != arguments.check_id
        or contract.get("gate_definition_sha256") != gate_digest
        or contract.get("verifier_sha256") != verifier_digest
        or contract.get("assertion_id") != [assertion_id]
    ):
        raise GateError("gate command authority differs from source bytes")
    require_source_state(arguments.source_revision, arguments.source_tree)
    run_step(arguments.step, arguments.source_revision)
    assertions = [{"id": assertion_id, "result": "pass"}]
    result = {
        "schema": "radroots.services-hardening.rshr-200-step-check-result.v1",
        "step": arguments.step,
        "check_id": arguments.check_id,
        "gate_definition_sha256": gate_digest,
        "source_revision": arguments.source_revision,
        "source_tree": arguments.source_tree,
        "candidate_generation": 0,
        "candidate_digest": "none",
        "command_contract_sha256": sha256_bytes(canonical(contract)),
        "verifier_sha256": verifier_digest,
        "execution_request": [
            {
                "platform": arguments.platform,
                "sha256": arguments.execution_request_sha256,
            }
        ],
        "assertion_inventory_sha256": sha256_bytes(canonical(assertions)),
        "assertion": assertions,
        "result": "pass",
    }
    sys.stdout.buffer.write(canonical(result) + b"\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (GateError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"HarvestCircle RSHR-201 gate failed: {error}", file=sys.stderr)
        raise SystemExit(1)
