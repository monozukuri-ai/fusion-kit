"""Synthetic manifest bytes built from the documented length-prefix grammar."""

import struct

from .zip_fixtures import make_zip

DOC = "11111111-2222-4333-8444-555555555555"
DESIGN = "11111111-2222-4333-8444-555555555556"
OTHER = "11111111-2222-4333-8444-555555555557"
PNG = b"\x89PNG\r\n\x1a\nfixture"


def u32(n):
    return struct.pack("<I", n)


def text(value, wide=False):
    data = value.encode("utf-16-le" if wide else "ascii")
    return u32(len(data) // (2 if wide else 1)) + data


def manifest(
    *,
    bases=("Root",),
    active=DESIGN,
    ending=b"\x01",
    version="3-2-0-0",
    current=True,
    opaque=b"\x00\x01opaque",
):
    prefix = b"".join(
        [
            text(version),
            text("FusionDocType"),
            text(".f3d", True),
            text("モデル😀", True),
            text("", True),
            text(DOC, True),
            text(DESIGN, True),
            (u32(1234) + u32(1) + u32(2) + u32(0)) if current else (u32(333) + u32(0)),
            u32(1),
            text("Fusion"),
            u32(20),
            opaque,
        ]
    )
    return (
        prefix
        + text(active, True)
        + u32(len(bases))
        + b"".join(text(b, True) for b in bases)
        + u32(0)
        + ending
    )


def asset(
    base="Root",
    *,
    guid=DESIGN,
    asset_type="FusionAssetType",
    revision=20,
    subtype="",
    prefix="Design",
    suffix=b"",
    references=0,
    segment_type="FusionDesignSegmentType",
    subtype_mode=0,
):
    header = text(base, True) + text(guid, True) + text(OTHER, True) + text(asset_type)
    if asset_type != "FusionAssetType":
        return header + u32(20) + b"opaque non-design payload"
    return b"".join(
        [
            header,
            u32(revision),
            u32(1),
            text("Application"),
            u32(139),
            text("Neutron3DAssetType"),
            bytes([subtype_mode]),
            text(subtype),
            u32(1),
            text("physicalChangeGuid"),
            text(OTHER, True),
            u32(references),
            u32(7),
            u32(1),
            u32(0),
            text(prefix),
            text(segment_type),
            suffix,
        ]
    )


def document(
    *,
    ending=b"\x01",
    version="3-2-0-0",
    revision=20,
    prefix="Design",
    preview=PNG,
    asset_suffix=b"",
    references=0,
    active_other=False,
    extra=None,
):
    bases = ("Other", "Root") if active_other else ("Root",)
    entries = [
        (
            "Manifest.dat",
            manifest(
                bases=bases,
                active=OTHER if active_other else DESIGN,
                ending=ending,
                version=version,
            ),
        ),
        (
            "Root[Active]/Manifest.dat",
            asset(
                revision=revision,
                prefix=prefix,
                suffix=asset_suffix,
                references=references,
            ),
        ),
        (f"Root[Active]/{prefix}1/MetaStream.dat", b"meta"),
        (f"Root[Active]/{prefix}1/BulkStream.dat", b"bulk"),
    ]
    if active_other:
        entries.append(
            (
                "Other[Active]/Manifest.dat",
                asset("Other", guid=OTHER, asset_type="AnimationAssetType"),
            )
        )
    if preview is not None:
        entries.append(("Root[Active]/Previews/small.png", preview))
    if extra:
        entries.extend(extra)
    return make_zip(entries)[0]
