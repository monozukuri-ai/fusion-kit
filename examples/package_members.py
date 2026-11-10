"""Inspect each F3Z member by its scoped ID, without writing preview images."""

import argparse
import hashlib
import json
from pathlib import Path
from typing import Literal, TypedDict

import fusion_kit
from fusion_kit.types import Status


class MemberSummary(TypedDict):
    id: str
    document_guid: str | None
    status: Status
    preview_status: Literal["present", "absent", "not_requested"]
    preview_sha256: str | None


def summarize(path: Path) -> list[MemberSummary]:
    archive = fusion_kit.readfile(path, strict=False)
    package = archive.read_package()
    rows: list[MemberSummary] = []
    for doc in package.documents:
        row: MemberSummary = {
            "id": doc.id,
            "document_guid": doc.document_guid,
            "status": doc.status,
            "preview_status": "not_requested",
            "preview_sha256": None,
        }
        if doc.metadata is not None and doc.status == "complete":
            # Each explicit child operation has its own budget.
            child = archive.open_document(doc.id)
            preview = child.get_preview()
            row["preview_status"] = "absent" if preview is None else "present"
            if preview is not None:
                row["preview_sha256"] = hashlib.sha256(preview).hexdigest()
        rows.append(row)
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.path), ensure_ascii=False))


if __name__ == "__main__":
    main()
