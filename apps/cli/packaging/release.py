#!/usr/bin/env python3
"""Release steps for tc that the release workflow runs (ADR 0017).

    release.py check-version VERSION        the tag matches apps/cli/Cargo.toml
    release.py archive TARGET BINARY DEPENDENCIES OUT
                                            one archive: tc, license, notices
    release.py sums DIR                     SHA256SUMS for the archives in DIR
    release.py formula VERSION SUMS         the Homebrew formula on standard output
    release.py winget VERSION SUMS OUT      the three winget manifest files in OUT

Archive names are tc-<target>.tar.gz, and tc-<target>.zip for Windows; the
GitHub Action downloads them by these names.
"""

from __future__ import annotations

import hashlib
import io
import re
import sys
import tarfile
import zipfile
from pathlib import Path

CLI = Path(__file__).resolve().parent.parent
REPOSITORY = "https://github.com/tokencontroller/tokencontroller"
HOMEPAGE = "https://docs.tokencontroller.com/collect/cli"
DESCRIPTION = "Cost accounting for AI agents: reads session files, submits token sheets"
SUMMARY = (
    "Token Controller's command line: reads AI agents' session files, places "
    "token entries with your rules and submits token sheets."
)

# Homebrew: (operating system block, CPU block) for each target it installs.
HOMEBREW = {
    "aarch64-apple-darwin": ("on_macos", "on_arm"),
    "x86_64-apple-darwin": ("on_macos", "on_intel"),
    "aarch64-unknown-linux-gnu": ("on_linux", "on_arm"),
    "x86_64-unknown-linux-gnu": ("on_linux", "on_intel"),
}
WINGET = {
    "x86_64-pc-windows-msvc": "x64",
    "aarch64-pc-windows-msvc": "arm64",
}


def archive_name(target: str) -> str:
    return f"tc-{target}.zip" if "windows" in target else f"tc-{target}.tar.gz"


def release_url(version: str, file: str) -> str:
    return f"{REPOSITORY}/releases/download/v{version}/{file}"


def cargo_version() -> str:
    """The version in apps/cli/Cargo.toml (tomllib needs Python 3.11)."""
    text = (CLI / "Cargo.toml").read_text()
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if match is None:
        sys.exit("apps/cli/Cargo.toml has no version")
    return match.group(1)


def check_version(version: str) -> None:
    expected = cargo_version()
    if version != expected:
        sys.exit(f"the tag says {version}, apps/cli/Cargo.toml says {expected}")


def archive(target: str, binary: str, dependencies: str, out: str) -> Path:
    """Packs the binary with its license and the notices."""
    windows = "windows" in target
    files = [
        ("tc.exe" if windows else "tc", Path(binary), 0o755),
        ("LICENSE", CLI / "LICENSE", 0o644),
        ("THIRD_PARTY_NOTICES.md", CLI / "THIRD_PARTY_NOTICES.md", 0o644),
        ("DEPENDENCIES.md", Path(dependencies), 0o644),
    ]
    folder = Path(out)
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / archive_name(target)
    if windows:
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as packed:
            for name, source, mode in files:
                info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                packed.writestr(info, source.read_bytes())
    else:
        with tarfile.open(path, "w:gz") as packed:
            for name, source, mode in files:
                data = source.read_bytes()
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = mode
                info.mtime = 1767225600
                packed.addfile(info, io.BytesIO(data))
    return path


def sums(folder: str) -> Path:
    """SHA256SUMS in the format `sha256sum` writes."""
    root = Path(folder)
    lines = []
    for path in sorted(root.iterdir()):
        if path.name.startswith("tc-") and path.name.endswith((".tar.gz", ".zip")):
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            lines.append(f"{digest}  {path.name}\n")
    if not lines:
        sys.exit(f"no tc archives in {folder}")
    out = root / "SHA256SUMS"
    out.write_text("".join(lines))
    return out


def read_sums(path: str) -> dict[str, str]:
    found = {}
    for line in Path(path).read_text().splitlines():
        parts = line.split()
        if len(parts) == 2:
            found[parts[1].lstrip("*")] = parts[0].lower()
    return found


def checksum(found: dict[str, str], target: str) -> str:
    name = archive_name(target)
    if name not in found:
        sys.exit(f"SHA256SUMS does not list {name}")
    return found[name]


def formula(version: str, sums_path: str) -> str:
    found = read_sums(sums_path)
    blocks = []
    for system in ("on_macos", "on_linux"):
        inner = []
        for target, (os_block, cpu_block) in HOMEBREW.items():
            if os_block != system:
                continue
            inner.append(
                f"    {cpu_block} do\n"
                f'      url "{release_url(version, archive_name(target))}"\n'
                f'      sha256 "{checksum(found, target)}"\n'
                f"    end\n"
            )
        blocks.append(f"  {system} do\n{''.join(inner)}  end\n")
    return f"""# Written by .github/workflows/release.yml from apps/cli/packaging/release.py.
# brew tap tokencontroller/tap {REPOSITORY}
# brew install tc
class Tc < Formula
  desc "{DESCRIPTION}"
  homepage "{HOMEPAGE}"
  version "{version}"
  license "MIT"

{"".join(blocks)}
  head do
    url "{REPOSITORY}.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "apps/cli")
    else
      bin.install "tc"
    end
  end

  test do
    assert_match "tc ", shell_output("#{{bin}}/tc --version")
  end
end
"""


def yaml_string(text: str) -> str:
    """Quote a YAML value: the summary holds a colon, which bare YAML reads as a key."""
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def winget(version: str, sums_path: str, out: str) -> None:
    found = read_sums(sums_path)
    identifier = "TokenController.tc"
    folder = Path(out)
    folder.mkdir(parents=True, exist_ok=True)
    head = f"PackageIdentifier: {identifier}\nPackageVersion: {version}\n"
    tail = "ManifestVersion: 1.6.0\n"
    (folder / f"{identifier}.yaml").write_text(
        f"{head}DefaultLocale: en-US\nManifestType: version\n{tail}"
    )
    installers = "".join(
        f"  - Architecture: {architecture}\n"
        f"    InstallerUrl: {release_url(version, archive_name(target))}\n"
        f"    InstallerSha256: {checksum(found, target).upper()}\n"
        for target, architecture in WINGET.items()
    )
    (folder / f"{identifier}.installer.yaml").write_text(
        f"{head}InstallerType: zip\nNestedInstallerType: portable\n"
        f"NestedInstallerFiles:\n  - RelativeFilePath: tc.exe\n    PortableCommandAlias: tc\n"
        f"Installers:\n{installers}ManifestType: installer\n{tail}"
    )
    (folder / f"{identifier}.locale.en-US.yaml").write_text(
        f"{head}PackageLocale: en-US\nPublisher: Token Controller\n"
        f"PublisherUrl: https://tokencontroller.com\nPackageName: tc\n"
        f"PackageUrl: {HOMEPAGE}\nLicense: MIT\n"
        f"LicenseUrl: {REPOSITORY}/blob/main/apps/cli/LICENSE\n"
        f"ShortDescription: {yaml_string(SUMMARY)}\nMoniker: tc\n"
        f"Tags:\n  - ai-agents\n  - cost-accounting\n  - tokens\n"
        f"ManifestType: defaultLocale\n{tail}"
    )


def main(args: list[str]) -> None:
    match args:
        case ["check-version", version]:
            check_version(version)
        case ["archive", target, binary, dependencies, out]:
            print(archive(target, binary, dependencies, out))
        case ["sums", folder]:
            print(sums(folder).read_text(), end="")
        case ["formula", version, sums_path]:
            sys.stdout.write(formula(version, sums_path))
        case ["winget", version, sums_path, out]:
            winget(version, sums_path, out)
        case _:
            sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
