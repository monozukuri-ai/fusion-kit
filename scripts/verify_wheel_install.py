"""Install a wheel in a fresh interpreter and check the installed native module."""

import argparse
import hashlib
import os
import subprocess
import tempfile
import venv
import zipfile
from pathlib import Path

SMOKE = """
import hashlib
import importlib.metadata
import platform
import runpy
import sys
import sysconfig
from pathlib import Path

import fusion_kit
from fusion_kit import _native

assert platform.python_implementation() == "CPython"
assert not sysconfig.get_config_var("Py_GIL_DISABLED")
extension = Path(_native.__file__).resolve()
assert extension.is_relative_to(Path(sys.prefix).resolve())
assert hashlib.sha256(extension.read_bytes()).hexdigest() == sys.argv[1]
assert fusion_kit.__version__ == _native.__core_version__ == importlib.metadata.version("fusion-kit")
assert not importlib.metadata.requires("fusion-kit")
make_zip = runpy.run_path(sys.argv[2])["make_zip"]
data, _ = make_zip(method=93)
archive = fusion_kit.read(data)
assert archive.read_member("payload.bin") == b"hello\\x00world"
assert archive.verify().status == "complete"
print(platform.platform(), platform.machine(), sys.version.split()[0], extension.name, sys.argv[1])
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheel_directory", type=Path)
    parser.add_argument("source", type=Path, help="The inspected, extracted sdist")
    parser.add_argument(
        "--pytest-version",
        help="Also run the sdist test suite with this pytest version",
    )
    args = parser.parse_args()
    (wheel,) = args.wheel_directory.resolve().glob("*.whl")
    source = args.source.resolve()
    with zipfile.ZipFile(wheel) as archive:
        (extension,) = [
            name for name in archive.namelist() if name.endswith((".so", ".pyd"))
        ]
        digest = hashlib.sha256(archive.read(extension)).hexdigest()
    with tempfile.TemporaryDirectory(prefix="fusion-kit-install-") as directory:
        env = Path(directory) / "env"
        venv.EnvBuilder(with_pip=True).create(env)
        scripts = env / ("Scripts" if os.name == "nt" else "bin")
        python = scripts / ("python.exe" if os.name == "nt" else "python")
        cli = scripts / ("fusion-kit.exe" if os.name == "nt" else "fusion-kit")
        pip = [python, "-I", "-m", "pip", "--isolated", "install", "--no-cache-dir"]
        subprocess.run(
            [*pip, "--no-index", "--no-deps", wheel], check=True, cwd=directory
        )
        subprocess.run(
            [
                python,
                "-I",
                "-c",
                SMOKE,
                digest,
                source / "tests/python/zip_fixtures.py",
            ],
            check=True,
            cwd=directory,
        )
        subprocess.run([cli, "--version"], check=True, cwd=directory)
        if args.pytest_version:
            subprocess.run(
                [*pip, f"pytest=={args.pytest_version}"], check=True, cwd=directory
            )
            subprocess.run(
                [python, "-I", "-m", "pytest", "-q", "-p", "no:cacheprovider", "tests/python"],
                check=True,
                cwd=source,
            )


if __name__ == "__main__":
    main()
