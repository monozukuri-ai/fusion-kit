"""Static API contracts; ignored errors must remain errors (warn_unused_ignores)."""

from pathlib import Path
from typing import Literal, assert_type

import fusion_kit as fusion
from fusion_kit.types import (
    ArchiveJSON,
    DocumentJSON,
    ErrorJSON,
    ExitCode,
    JSONValue,
    PackageJSON,
    Status,
)


def supported(path: Path, data: bytes) -> None:
    archive = fusion.readfile(path, strict=False)
    assert_type(fusion.read(data), fusion.Archive)
    assert_type(archive.to_dict(), ArchiveJSON)
    doc = archive.read_document(strict=False)
    assert_type(doc.to_dict(), DocumentJSON)
    assert_type(doc.status, Status)
    assert_type(doc.exit_code, ExitCode)
    if doc.manifest is not None:
        assert_type(doc.manifest.name, str)
    if doc.assets is not None:
        for asset in doc.assets:
            assert_type(asset.issues, list[fusion.Diagnostic])
    package = archive.read_package()
    assert_type(package.to_dict(), PackageJSON)
    for child in package.documents:
        assert_type(child.id, str)
        if child.metadata is not None:
            assert_type(child.metadata, fusion.DocumentInfo)
        assert_type(archive.open_document(child.id), fusion.Archive)
    for sidecar in package.raw_sidecars:
        assert_type(sidecar.value, JSONValue)
    try:
        archive.verify(strict=True)
    except fusion.FusionError as error:
        assert_type(error.result, ErrorJSON)
        assert_type(error.issues, list[fusion.Diagnostic])
        assert_type(error.exit_code, ExitCode)
        if error.report is not None:
            assert_type(
                error.report.nested_archives,
                Literal["complete", "partial", "not_requested"],
            )


def forbidden(doc: fusion.DocumentInfo, archive: fusion.Archive) -> None:
    doc.id = "changed"  # type: ignore[misc]
    fusion.read(bytearray())  # type: ignore[arg-type]
    fusion.DocumentInfo()  # type: ignore[call-arg]
    archive.read_document(False)  # type: ignore[misc]
    _ = doc.manifest.name  # type: ignore[union-attr]
    _ = doc.to_dict()["bodies"]  # type: ignore[typeddict-item]
    payload = doc.to_dict()
    payload["status"] = "success"  # type: ignore[typeddict-item]
    archive.get_preview(document_id=None).decode()  # type: ignore[union-attr]


class CannotSubclass(fusion.DocumentInfo):  # type: ignore[misc]
    pass
