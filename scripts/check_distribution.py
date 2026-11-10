"""Inspect fusion-kit wheels and sdist and assemble a checksummed CI bundle."""

import argparse
import base64
import csv
import hashlib
import io
import os
import re
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile
from email.parser import BytesParser
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
PROJECT = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))[
    "project"
]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"][
    "package"
]["version"]
STEM = f"fusion_kit-{VERSION}"
LICENSE_FILES = PROJECT["license-files"]
PLATFORMS = {
    "linux-x86_64": "manylinux_2_28_x86_64",
    "windows-x86_64": "win_amd64",
    "macos-arm64": "macosx_11_0_arm64",
    "macos-x86_64": "macosx_11_0_x86_64",
}
PYTHON_FILES = {
    file.relative_to(ROOT / "python").as_posix()
    for file in (ROOT / "python/fusion_kit").iterdir()
    if file.suffix in {".py", ".pyi"} or file.name == "py.typed"
}


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def one(directory, pattern):
    paths = list(directory.glob(pattern))
    require(len(paths) == 1, f"Expected one {pattern} in {directory}, found {paths}")
    return paths[0]


def check_paths(names):
    require(
        len(names) == len({name.casefold() for name in names}),
        "Duplicate archive paths",
    )
    for name in names:
        path = PurePosixPath(name)
        require(
            path.parts
            and not path.is_absolute()
            and ".." not in path.parts
            and name == path.as_posix()
            and "\\" not in name
            and ":" not in name,
            f"Unsafe archive path: {name}",
        )


def headers(data):
    return BytesParser().parsebytes(data, headersonly=True)


def check_metadata(data):
    metadata = headers(data)
    for field, expected in (
        ("Name", "fusion-kit"),
        ("Version", VERSION),
        ("Requires-Python", PROJECT["requires-python"]),
        ("License-Expression", PROJECT.get("license")),
    ):
        require(metadata[field] == expected, f"Incorrect {field}: {metadata[field]!r}")
    require(
        not metadata.get_all("Requires-Dist"), "Unexpected Python runtime dependencies"
    )
    require(
        set(metadata.get_all("License-File", [])) == set(LICENSE_FILES),
        "Incorrect License-File metadata",
    )


def equal_text(data, file):
    return data.decode("utf-8").replace("\r\n", "\n") == file.read_text(
        encoding="utf-8"
    )


def check_sdist(path, extract=None):
    require(path.name == f"{STEM}.tar.gz", f"Unexpected sdist name: {path.name}")
    expected = {
        "PKG-INFO",
        "Cargo.toml",
        "Cargo.lock",
        "pyproject.toml",
        "README.md",
        "rust-toolchain.toml",
        *LICENSE_FILES,
        *(f"python/{name}" for name in PYTHON_FILES),
    }
    for directory, pattern in (
        ("crates", "**/*.rs"),
        ("crates", "*/Cargo.toml"),
        ("docs", "*.md"),
        ("tests/python", "*.py"),
        ("tests/typing", "*.py"),
        ("examples", "*.py"),
    ):
        expected.update(
            file.relative_to(ROOT).as_posix()
            for file in (ROOT / directory).glob(pattern)
        )
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        check_paths(names)
        require(
            all(member.isfile() for member in members),
            "Expected regular files in sdist",
        )
        require(
            set(names) == {f"{STEM}/{name}" for name in expected},
            f"Unexpected sdist inventory: {set(names) ^ {f'{STEM}/{name}' for name in expected}}",
        )
        check_metadata(archive.extractfile(f"{STEM}/PKG-INFO").read())
        for name in expected - {"PKG-INFO"}:
            data = archive.extractfile(f"{STEM}/{name}").read()
            if name == "Cargo.toml":
                # Maturin removes workspace.default-members in the sdist.
                workspace = tomllib.loads((ROOT / name).read_text(encoding="utf-8"))
                workspace["workspace"].pop("default-members", None)
                require(
                    tomllib.loads(data.decode("utf-8")) == workspace,
                    "sdist workspace differs",
                )
            else:
                require(equal_text(data, ROOT / name), f"sdist source differs: {name}")
        if extract is not None:
            extract.mkdir(parents=True, exist_ok=False)
            for member in members:
                destination = extract / PurePosixPath(member.name).relative_to(STEM)
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(archive.extractfile(member).read())
    print(f"{path.name}: source inventory and metadata OK")


def audit_binary(path, platform):
    if platform == "linux-x86_64":
        output = subprocess.check_output(["readelf", "-d", path], text=True)
        dependencies = set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", output))
        allowed = {
            "libc.so.6",
            "libgcc_s.so.1",
            "libm.so.6",
            "libpthread.so.0",
            "libdl.so.2",
            "librt.so.1",
            "ld-linux-x86-64.so.2",
        }
        require(
            dependencies and dependencies <= allowed,
            f"Unexpected ELF dependencies: {dependencies}",
        )
    elif platform.startswith("macos-"):
        output = subprocess.check_output(["otool", "-L", path], text=True)
        dependencies = {
            line.strip().split(" (", 1)[0] for line in output.splitlines()[1:]
        }
        # otool -L includes the library's own LC_ID_DYLIB entry.
        install_name = subprocess.check_output(["otool", "-D", path], text=True)
        dependencies.difference_update(
            line.strip() for line in install_name.splitlines()[1:]
        )
        require(
            dependencies
            and all(
                name.startswith(("/usr/lib/", "/System/Library/"))
                for name in dependencies
            ),
            f"Unexpected Mach-O dependencies: {dependencies}",
        )
    else:
        vswhere = (
            Path(os.environ["ProgramFiles(x86)"])
            / "Microsoft Visual Studio/Installer/vswhere.exe"
        )
        candidates = subprocess.check_output(
            [
                vswhere,
                "-latest",
                "-products",
                "*",
                "-find",
                r"VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe",
            ],
            text=True,
            encoding="utf-8",
        ).splitlines()
        require(candidates, "MSVC dumpbin was not found")
        output = subprocess.check_output(
            [candidates[0], "/DEPENDENTS", path],
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        dependencies = set(
            re.findall(r"^\s+([\w.-]+\.dll)\s*$", output.lower(), re.MULTILINE)
        )
        allowed = {
            "python3.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "advapi32.dll",
            "bcrypt.dll",
            "bcryptprimitives.dll",
            "ntdll.dll",
            "userenv.dll",
            "ws2_32.dll",
            "ucrtbase.dll",
            "msvcrt.dll",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
        }
        require(
            "python3.dll" in dependencies
            and all(
                name in allowed or name.startswith("api-ms-win-")
                for name in dependencies
            ),
            f"Unexpected PE dependencies: {dependencies}",
        )
    print("Dynamic dependencies:", ", ".join(sorted(dependencies)))


def check_wheel(path, platform, audit=False):
    tag = f"cp311-abi3-{PLATFORMS[platform]}"
    require(path.name == f"{STEM}-{tag}.whl", f"Unexpected wheel tag: {path.name}")
    info = f"{STEM}.dist-info"
    extension = (
        "fusion_kit/_native.pyd"
        if platform == "windows-x86_64"
        else "fusion_kit/_native.abi3.so"
    )
    expected = (
        PYTHON_FILES
        | {extension}
        | {
            f"{info}/{name}"
            for name in ("METADATA", "WHEEL", "RECORD", "entry_points.txt")
        }
        | {f"{info}/licenses/{name}" for name in LICENSE_FILES}
    )
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        check_paths(names)
        require(
            set(names) == expected,
            f"Unexpected wheel inventory: {set(names) ^ expected}",
        )
        check_metadata(archive.read(f"{info}/METADATA"))
        wheel = headers(archive.read(f"{info}/WHEEL"))
        require(
            wheel["Root-Is-Purelib"] == "false" and wheel.get_all("Tag") == [tag],
            "Incorrect WHEEL tags",
        )
        for name in LICENSE_FILES:
            require(
                equal_text(archive.read(f"{info}/licenses/{name}"), ROOT / name),
                f"Wheel notice differs: {name}",
            )
        for name in PYTHON_FILES:
            require(
                equal_text(archive.read(name), ROOT / "python" / name),
                f"Wheel Python source differs: {name}",
            )
        record = f"{info}/RECORD"
        rows = list(csv.reader(io.StringIO(archive.read(record).decode("utf-8"))))
        require(
            len(rows) == len(names) and {row[0] for row in rows} == expected,
            "Incorrect RECORD inventory",
        )
        for name, checksum, length in rows:
            if name == record:
                require(not checksum and not length, "RECORD must have no self hash")
                continue
            data = archive.read(name)
            sha = (
                base64.urlsafe_b64encode(hashlib.sha256(data).digest())
                .decode()
                .rstrip("=")
            )
            require(
                checksum == f"sha256={sha}" and length == str(len(data)),
                f"Incorrect RECORD hash: {name}",
            )
        if audit:
            with tempfile.TemporaryDirectory() as temp:
                binary = Path(temp) / PurePosixPath(extension).name
                binary.write_bytes(archive.read(extension))
                audit_binary(binary, platform)
    print(f"{path.name}: ABI3 tag, inventory, metadata and RECORD OK")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("sdist", "wheel", "bundle"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("--platform", choices=PLATFORMS)
    parser.add_argument("--extract", type=Path)
    parser.add_argument("--audit", action="store_true")
    args = parser.parse_args()
    if args.kind == "sdist":
        check_sdist(one(args.directory, "*.tar.gz"), args.extract)
    elif args.kind == "wheel":
        require(args.platform, "--platform is required for a wheel")
        check_wheel(one(args.directory, "*.whl"), args.platform, args.audit)
    else:
        source = one(args.directory, "*.tar.gz")
        check_sdist(source)
        artifacts = [source]
        for platform, tag in PLATFORMS.items():
            wheel = one(args.directory, f"*-{tag}.whl")
            check_wheel(wheel, platform)
            artifacts.append(wheel)
        checksum = args.directory / "SHA256SUMS"
        require(
            set(args.directory.iterdir()) - {checksum} == set(artifacts),
            "Unexpected files in bundle",
        )
        expected_checksums = "".join(
            f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n"
            for file in sorted(artifacts)
        )
        if checksum.exists():
            require(
                checksum.read_text(encoding="ascii") == expected_checksums,
                "SHA256SUMS mismatch",
            )
        else:
            checksum.write_text(expected_checksums, encoding="ascii", newline="\n")


if __name__ == "__main__":
    main()
