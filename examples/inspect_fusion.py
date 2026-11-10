"""Read F3D metadata or F3Z dependencies and print schema-v1 JSON."""

import argparse
import json
from pathlib import Path

import fusion_kit
from fusion_kit.types import DocumentJSON, ExitCode, PackageJSON


def inspect(path: Path) -> tuple[DocumentJSON | PackageJSON, ExitCode]:
    archive = fusion_kit.readfile(path, strict=False)
    if archive.kind == "f3d":
        document = archive.read_document()
        return document.to_dict(), document.exit_code
    package = archive.read_package()
    return package.to_dict(), package.exit_code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    args = parser.parse_args()
    result, code = inspect(args.path)
    print(json.dumps(result, ensure_ascii=False))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
