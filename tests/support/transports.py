"""Model only the external command shapes exercised by release and consumer tests."""

import json
import subprocess
from pathlib import Path

from sourcefield_tools.artifacts import digest_file
from sourcefield_tools.consumer import relative_path
from sourcefield_tools.release import TARGETS

REPOSITORY = "kdominic89/sourcefield"
RELEASE = "v1.2.3"
SOURCE_COMMIT = "a" * 40
ASSET_NAMES = {f"sourcefield-{target}.zip" for target in set(TARGETS.values()) | {"browser"}}


def verified_command(command: list[str], directory: Path) -> bool:
    """Recognize the exact synthetic release and attestation verification contract."""
    if command == ["gh", "release", "verify", RELEASE, "--repo", REPOSITORY]:
        return True

    for name in ASSET_NAMES:
        path = str(directory / name)
        if command == ["gh", "release", "verify-asset", RELEASE, path, "--repo", REPOSITORY]:
            return True

        if command == [
            "gh", "attestation", "verify", path, "--repo", REPOSITORY,
            "--signer-workflow", f"{REPOSITORY}/.github/workflows/release.yml",
            "--source-digest", SOURCE_COMMIT, "--signer-digest", SOURCE_COMMIT,
        ]:
            return True

    return False


def installation_transport(root: Path):
    """Simulate only verified downloads and the installed CLI's version probe."""
    def run(command, **kwargs):
        """Execute one modeled download or verification without invoking a real command."""
        if not kwargs.get("check"):
            raise AssertionError("unmodeled unchecked installation command")

        if command == ["gh", "release", "verify", RELEASE, "--repo", REPOSITORY]:
            return subprocess.CompletedProcess(command, 0, stdout="")

        if len(command) == 10 and command[:6] == ["gh", "release", "download", RELEASE, "--repo", REPOSITORY]:
            name, directory = command[7], Path(command[9])
            if (command[6] != "--pattern" or command[8] != "--dir" or name not in ASSET_NAMES
                    or directory.parent != root or not directory.name.startswith(".sourcefield-install-")):
                raise AssertionError("unmodeled installation download command")

            (directory / name).write_bytes((root / "assets" / name).read_bytes())

            return subprocess.CompletedProcess(command, 0, stdout="")

        directory = None
        if len(command) >= 5 and command[:3] == ["gh", "release", "verify-asset"]:
            directory = Path(command[4]).parent
        elif len(command) >= 4 and command[:3] == ["gh", "attestation", "verify"]:
            directory = Path(command[3]).parent

        if (directory is not None and directory.parent == root
                and directory.name.startswith(".sourcefield-install-") and verified_command(command, directory)):
            return subprocess.CompletedProcess(command, 0, stdout="")

        if len(command) == 2 and command[1] == "--version":
            executable = Path(command[0])
            directory = executable.parent.parent
            if (executable.name in {"sourcefield", "sourcefield.exe"} and executable.parent.name == "complete"
                    and directory.parent == root and directory.name.startswith(".sourcefield-install-")):
                return subprocess.CompletedProcess(command, 0, stdout="sourcefield 1.2.3\n")

        raise AssertionError("unmodeled installation command")

    return run


def publication_transport(root: Path, state: dict):
    """Model release state changes only after validating the complete command shape."""
    inventory = [
        "gh", "api", "--paginate", f"repos/{REPOSITORY}/releases?per_page=100", "--jq",
        '.[] | select(.tag_name == "v1.2.3") | '
        '{draft, target_commitish, assets: [.assets[] | {name, size, digest}]}',
    ]

    tag_lookup = [
        "gh", "api", f"repos/{REPOSITORY}/git/matching-refs/tags/{RELEASE}",
        "--jq", '.[] | select(.ref == "refs/tags/v1.2.3") | .ref',
    ]

    commit_lookup = ["gh", "api", f"repos/{REPOSITORY}/commits/tags/{RELEASE}", "--jq", ".sha"]
    create = [
        "gh", "release", "create", RELEASE, "--repo", REPOSITORY,
        "--target", SOURCE_COMMIT, "--draft", "--title", RELEASE, "--generate-notes",
    ]

    edit = ["gh", "release", "edit", RELEASE, "--repo", REPOSITORY, "--draft=false"]

    def run(command, **kwargs):
        """Return synthetic API state or apply one explicitly supported release mutation."""
        uploading = len(command) >= 8 and command[:4] == ["gh", "release", "upload", RELEASE]
        if uploading:
            files = sorted(path for path in (root / "assets").iterdir() if path.is_file())
            expected_upload = ["gh", "release", "upload", RELEASE, *map(str, files),
                               "--repo", REPOSITORY, "--clobber"]

            uploading = command == expected_upload

        if (not kwargs.get("check") or not (
                command in (inventory, tag_lookup, commit_lookup, create, edit)
                or uploading or verified_command(command, root / "assets"))):
            raise AssertionError("unmodeled publication command")

        if command[1:3] == state.get("fail_on"):
            raise subprocess.CalledProcessError(1, command)

        output = ""
        if command == inventory and state.get("draft") is not None:
            assets = [dict(asset) for asset in state.get("assets", {}).values()]
            fault = state.get("inventory_fault")
            if assets and fault == "digest":
                assets[0]["digest"] = "sha256:" + "0" * 64
            elif assets and fault == "null":
                assets[0]["digest"] = None
            elif assets and fault == "size":
                assets[0]["size"] += 1
            elif assets and fault == "missing":
                assets.pop()
            elif assets and fault == "duplicate":
                assets.append(dict(assets[0]))

            if state.get("extra_asset"):
                assets.append({"name": "unexpected.zip", "size": 1, "digest": "sha256:bad"})

            output = json.dumps({
                "draft": state["draft"], "target_commitish": state.get("target", SOURCE_COMMIT), "assets": assets,
            })

        elif command == tag_lookup:
            output = "refs/tags/v1.2.3" if state.get("tag") else ""
        elif command == commit_lookup:
            output = state.get("tag_commit", SOURCE_COMMIT)
        elif command == create:
            state["draft"] = True
        elif uploading:
            state["assets"] = {
                path.name: {"name": path.name, "size": path.stat().st_size,
                            "digest": f"sha256:{digest_file(path)}"}
                for path in (root / "assets").iterdir() if path.is_file()
            }

            if "target_after_upload" in state:
                state["target"] = state["target_after_upload"]

            if state.get("tag_after_upload"):
                state["tag"] = True

        elif command == edit:
            state["draft"] = False
            state["tag"] = True

        return subprocess.CompletedProcess(command, 0, stdout=output)

    return run


def consumer_git_transport(command, **kwargs):
    """Accept the exact synthetic main-branch checks and scoped staging operations."""
    if command == ["git", "rev-parse", "HEAD"] and kwargs.get("check"):
        return subprocess.CompletedProcess(command, 0, stdout=SOURCE_COMMIT)

    if command == ["git", "ls-remote", "origin", "refs/heads/main"] and kwargs.get("check"):
        return subprocess.CompletedProcess(command, 0, stdout=SOURCE_COMMIT + " refs/heads/main")

    if len(command) == 5 and command[:4] == ["git", "check-ignore", "-q", "--"] and kwargs.get("check") is False:
        relative_path(command[4])

        return subprocess.CompletedProcess(command, 1)

    if len(command) >= 5 and command[:4] == ["git", "add", "--all", "--"] and kwargs.get("check"):
        for name in command[4:]:
            relative_path(name)

        return subprocess.CompletedProcess(command, 0)

    raise AssertionError("unmodeled consumer Git command")
