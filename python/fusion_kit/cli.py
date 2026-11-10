"""CLI over the native schema-v1 views; --json also covers argument errors."""

import argparse
import json
import sys
from typing import Never, cast

from . import (
    Archive,
    DocumentInfo,
    FusionError,
    Limits,
    PackageInfo,
    ValidationReport,
    __version__,
    readfile,
)
from .types import (
    ArchiveJSON,
    DocumentJSON,
    ErrorJSON,
    ExitCode,
    PackageJSON,
    ValidationJSON,
)


class _UsageError(ValueError):
    pass


class _Parser(argparse.ArgumentParser):
    def error(self, message: str) -> Never:
        raise _UsageError(message)


def _failure(error: BaseException, source: str | None) -> tuple[ErrorJSON, ExitCode]:
    native = getattr(error, "result", None)
    if native is not None:
        return cast(ErrorJSON, native), cast(ExitCode, getattr(error, "exit_code", 1))
    config = not isinstance(error, OSError)
    result: ErrorJSON = {
        "schema_version": 1,
        "status": "invalid",
        "report": None,
        "issues": [
            {
                "severity": "error",
                "code": "configuration.invalid" if config else "source.io",
                "stage": "configuration" if config else "source",
                "source": source,
                "member": None,
                "offset": None,
                "offset_basis": None,
                "context": None,
                "message": str(error),
            }
        ],
    }
    return result, 1


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    parser = _Parser(prog="fusion-kit")
    parser.add_argument("--version", action="version", version=__version__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("inspect", "verify"):
        sub = commands.add_parser(command)
        sub.add_argument("path")
        sub.add_argument("--json", action="store_true", dest="json_output")
        if command == "inspect":
            layer = sub.add_mutually_exclusive_group()
            layer.add_argument(
                "--document",
                action="store_true",
                help="parse a standalone F3D document and Design registry",
            )
            layer.add_argument(
                "--package",
                action="store_true",
                help="parse F3Z root and document dependencies",
            )
        else:
            sub.add_argument(
                "--outer-only",
                action="store_true",
                help="verify the outer ZIP without opening child archives",
            )
        for name in (
            "max-input-bytes",
            "max-member-bytes",
            "max-total-uncompressed-bytes",
            "max-entries",
            "max-nesting-depth",
        ):
            sub.add_argument("--" + name, type=int)
    json_output = "--json" in argv
    source: str | None = None
    operation = "configuration"
    lines: list[str] = []
    result: ArchiveJSON | DocumentJSON | PackageJSON | ValidationJSON | ErrorJSON
    view: Archive | DocumentInfo | PackageInfo | ValidationReport
    try:
        args = parser.parse_args(argv)
        source, operation = args.path, args.command
        limits = Limits(
            **{
                name: getattr(args, name)
                for name in (
                    "max_input_bytes",
                    "max_member_bytes",
                    "max_total_uncompressed_bytes",
                    "max_entries",
                    "max_nesting_depth",
                )
                if getattr(args, name) is not None
            }
        )
        archive = readfile(args.path, limits=limits, strict=False)
        if args.command == "verify":
            view = archive.verify(strict=False, recursive=not args.outer_only)
        elif args.package:
            view = archive.read_package(strict=False)
        elif args.document:
            view = archive.read_document(strict=False)
        else:
            view = archive
        result, code = view.to_dict(), view.exit_code
        if not json_output:
            if isinstance(view, Archive):
                lines = [
                    f"  {m.name} ({m.uncompressed_size} bytes)" for m in view.members
                ]
            elif isinstance(view, ValidationReport):
                lines = [
                    f"  verified {view.members_verified}/{view.members_total} members"
                ]
            elif isinstance(view, PackageInfo):
                lines = [
                    f"  root: {view.root_document_id}",
                    f"  {len(view.documents)} documents, {len(view.references)} references",
                ]
            else:
                lines = [f"  document: {view.id}"]
    except (FusionError, OSError, KeyError, ValueError, OverflowError) as error:
        result, code = _failure(error, source)
    if json_output:
        # JSON escapes preserve Unicode on consoles and pipes using legacy encodings.
        print(json.dumps(result))
    else:
        print(f"{operation}: {result['status']}")
        for line in lines:
            print(line)
        for issue in result["issues"]:
            print(f"  {issue['code']}: {issue['message']}", file=sys.stderr)
    return code
