"""Package a CI-built irl with the inputs needed to identify and rebuild it."""

import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    binary, name, runner = sys.argv[1:]
    if Path(name).name != name:
        raise ValueError("asset name must be a filename")
    dist = Path("desktop-dist")
    dist.mkdir(exist_ok=True)
    asset = dist / name
    shutil.copy2(binary, asset)
    if platform.system() != "Windows":
        asset.chmod(0o755)

    manifest = {
        "repository": os.environ["GITHUB_REPOSITORY"],
        "commit": command("git", "rev-parse", "HEAD"),
        "run_url": (
            f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/actions/runs/"
            f"{os.environ['GITHUB_RUN_ID']}"
        ),
        "runner": runner,
        "runner_image": os.environ.get("ImageOS"),
        "runner_image_version": os.environ.get("ImageVersion"),
        "os": platform.platform(),
        "architecture": platform.machine(),
        "rustc": command("rustc", "-Vv"),
        "cargo": command("cargo", "--version"),
        "profile": "release-dist",
        "features": ["aec"],
        "default_features": True,
        "cargo_lock_sha256": digest(Path("Cargo.lock")),
        "binary": name,
        "binary_sha256": digest(asset),
    }
    if platform.system() == "Darwin":
        manifest["macos_sdk"] = command("xcrun", "--sdk", "macosx", "--show-sdk-version")
        manifest["macos_deployment_target"] = os.environ["MACOSX_DEPLOYMENT_TARGET"]
    info = dist / "build-info.json"
    info.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    checksums = dist / "SHA256SUMS"
    checksums.write_text(
        "".join(f"{digest(path)}  {path.name}\n" for path in (asset, info)),
        encoding="utf-8",
    )

    # upload-artifact does not preserve Unix executable permissions. A tarball
    # does; normalize archive headers so packaging adds no current timestamps.
    if platform.system() == "Darwin":
        epoch = int(command("git", "log", "-1", "--format=%ct"))
        archive = dist / f"{name}.tar.gz"
        with archive.open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as tar:
                    for path in (asset, info, checksums):
                        header = tar.gettarinfo(str(path), arcname=path.name)
                        header.uid = header.gid = 0
                        header.uname = header.gname = ""
                        header.mtime = epoch
                        with path.open("rb") as contents:
                            tar.addfile(header, contents)
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
