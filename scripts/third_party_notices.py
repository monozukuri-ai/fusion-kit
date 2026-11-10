"""Collect upstream license texts for the locked builds on the four release targets."""

import argparse
import json
import subprocess
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)


def cargo_packages():
    packages = {}
    for target in TARGETS:
        metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--format-version",
                    "1",
                    "--filter-platform",
                    target,
                ],
                cwd=ROOT,
            )
        )
        resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
        for package in metadata["packages"]:
            if package["source"] and package["id"] in resolved:
                packages[package["id"]] = package
    return sorted(
        packages.values(), key=lambda package: (package["name"], package["version"])
    )


def render():
    lines = [
        "fusion-kit third-party notices",
        "",
        "Format references",
        "cadmpeg project, F3D format notes (CC BY 4.0), revision",
        "5af533221f18d2ed2e30c888381ab22324f8564f:",
        "https://github.com/cadmpeg/cadmpeg/blob/5af533221f18d2ed2e30c888381ab22324f8564f/docs/formats/f3d.md",
        "https://creativecommons.org/licenses/by/4.0/",
        "ezf3d contributors, format notes (MIT), revision",
        "97fe90ab8c39c3978fa6354f4a21c5ebdbc38c6e:",
        "https://github.com/AlexSabaka/ezf3d/tree/97fe90ab8c39c3978fa6354f4a21c5ebdbc38c6e/docs/format",
        "https://github.com/AlexSabaka/ezf3d/blob/97fe90ab8c39c3978fa6354f4a21c5ebdbc38c6e/LICENSE",
        "These notes were consulted for the independent parser implementation.",
        "",
        "Rust Standard Library",
        "RUST_STDLIB_COPYRIGHT.html is the notice shipped with the pinned Rust toolchain.",
        "",
        "Cargo dependencies",
        "This inventory covers normal and build dependencies for:",
        *TARGETS,
        "The linked subset depends on the target. Upstream terms govern each component.",
        "",
    ]
    texts = defaultdict(list)
    for package in cargo_packages():
        name, version = package["name"], package["version"]
        label = f"{name} {version}"
        root = Path(package["manifest_path"]).parent
        files = {
            file
            for file in root.iterdir()
            if file.is_file()
            and file.name.upper().startswith(
                ("LICENSE", "NOTICE", "COPYING", "COPYRIGHT", "UNLICENSE")
            )
        }
        if package["license_file"]:
            files.add(root / package["license_file"])
        if name == "zstd-sys":
            files.update(
                root / "zstd" / filename for filename in ("LICENSE", "COPYING")
            )
        if not files or not package["license"]:
            raise SystemExit(f"Review missing upstream license material for {label}")
        lines.extend(
            [
                f"{label}  {package['license']}",
                f"  https://crates.io/crates/{name}/{version}",
            ]
        )
        for file in sorted(files):
            content = file.read_text(encoding="utf-8").rstrip()
            texts[content].append(f"{label} / {file.relative_to(root).as_posix()}")
    lines.extend(["", "Upstream texts"])
    for content, labels in texts.items():
        lines.extend(["", *labels, "", content])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = ROOT / "THIRD_PARTY_NOTICES.txt"
    expected = render()
    if args.check:
        if path.read_text(encoding="utf-8") != expected:
            raise SystemExit(
                "Refresh THIRD_PARTY_NOTICES.txt after the Cargo dependency change"
            )
    else:
        path.write_text(expected, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
