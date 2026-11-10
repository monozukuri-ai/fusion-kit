import gc
import hashlib
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

import fusion_kit as fusion
import pytest

from .manifest_fixtures import (
    DESIGN,
    DOC,
    OTHER,
    PNG,
    asset,
    document,
    manifest,
    text,
    u32,
)
from .zip_fixtures import make_zip


def test_lazy_document_keeps_zip_and_metadata_scopes_separate():
    archive = fusion.read(document())
    assert archive.to_dict()["manifest"] == "not_requested"
    info = archive.read_document()
    assert info.status == info.manifest_status == "complete"
    assert info.id == archive.root_document_id == DOC
    assert archive.documents[0].source_sha256 == archive.source_sha256
    assert info.format_version == "3-2-0-0"
    assert info.manifest.name == "モデル😀"
    assert info.manifest.description == ""
    assert info.to_dict() == json.loads(info.to_json())
    assert info.to_dict()["geometry"] == "not_requested"
    assert archive.verify().scope == "outer_archive_members"


@pytest.mark.parametrize(
    "prefix", ["Design", "FusionDesignSegmentType", "DifferentName"]
)
def test_segment_type_and_prefix_resolve_the_design_folder(prefix):
    archive = fusion.read(
        document(
            prefix=prefix, extra=[("Root[Active]/DesignWrong/MetaStream.dat", b"wrong")]
        )
    )
    info = archive.read_document()
    segment = info.assets[0].segments[0]
    assert segment.segment_type == "FusionDesignSegmentType"
    assert segment.role == "design"
    assert segment.instances[0].folder == f"Root[Active]/{prefix}1"
    assert segment.instances[0].meta_member.endswith("/MetaStream.dat")
    assert segment.instances[0].bulk_member.endswith("/BulkStream.dat")
    assert len(segment.instances) == 1


def test_active_animation_is_not_used_as_design_owner():
    archive = fusion.read(document(active_other=True))
    info = archive.read_document()
    assert info.active_asset_id == OTHER
    assert info.design_asset_id == DESIGN
    assert info.design_asset_folder == "Root[Active]"
    assert info.assets[0].role == "other"
    assert info.assets[0].is_active
    assert info.assets[0].segments is None
    assert info.assets[0].segment_status == "not_requested"
    assert archive.get_preview() == PNG


def test_opaque_regions_are_exact_and_bound_to_input_hash():
    archive = fusion.read(document())
    region = archive.read_document().manifest.generation_extension
    assert archive.read_span(region) == b"\x00\x01opaque"
    assert region.source_sha256 == archive.source_sha256
    assert region.sha256 == hashlib.sha256(b"\x00\x01opaque").hexdigest()
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(document(preview=b"different")).read_span(region)
    assert exc.value.code == "span.source_mismatch"


def test_omission_and_encoded_empty_remain_distinct_even_with_cache():
    omitted = fusion.read(document()).read_document()
    assert omitted.manifest.tail.terminal.marker is None
    archive = fusion.read(document(ending=b"\x01" + text("", True)))
    partial = archive.read_document(strict=False)
    assert partial.status == "partial"
    assert partial.manifest.tail.terminal.marker == ""
    assert partial.manifest.framing_complete
    assert archive.read_span(partial.manifest.tail.terminal.raw) == b"\x01" + u32(0)
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.read_document(strict=True)


@pytest.mark.parametrize(
    "ending,form",
    [
        (b"", "no-terminal"),
        (b"\x01", "flag-1-only"),
        (b"\x01" + text("NA_EXPORT", True), "flag-1-marker"),
        (b"\x01" + text("NA_OFFLINESAVE", True), "flag-1-marker"),
        (b"\x01" + text("urn:example:1", True), "flag-1-lineage"),
        (b"\x00" + text("モデル😀", True), "flag-0-name"),
        (
            b"\x00"
            + text("model", True)
            + text("urn:example:1", True)
            + b"H\0A\0A\0H\0A\0",
            "flag-0-name-lineage-suffix",
        ),
    ],
)
def test_terminal_variants(ending, form):
    archive = fusion.read(document(ending=ending))
    assert archive.read_document().manifest.tail.terminal.form == form


@pytest.mark.parametrize(
    "ending",
    [b"\x01\x00", b"\x01\x00\x00", b"\x01\x00\x00\x00", b"\x01" + u32(9) + b"N\x00"],
)
def test_truncated_terminal_is_invalid_in_partial_mode(ending):
    archive = fusion.read(document(ending=ending), strict=False)
    with pytest.raises(fusion.FormatError) as exc:
        archive.read_document()
    assert exc.value.code == "manifest.truncated"
    assert exc.value.member == "Manifest.dat"


def test_unknown_version_retains_raw_manifest_without_fabricated_assets():
    archive = fusion.read(document(version="9-0-0-0"), strict=False)
    info = archive.read_document()
    assert info.status == "unsupported"
    assert info.format_version == "9-0-0-0"
    assert info.id is None and info.assets is None and info.manifest is None
    assert archive.read_span(info.unknown_regions[0]) == archive.read_member(
        "Manifest.dat"
    )
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.read_document(strict=True)


def test_unknown_tail_is_preserved_without_a_chosen_asset_run():
    archive = fusion.read(document(ending=b"\x02unknown"), strict=False)
    info = archive.read_document()
    assert info.status == "unsupported"
    assert info.manifest.tail is None
    assert info.manifest.generation_extension is None
    assert info.assets is None
    assert archive.read_span(info.unknown_regions[0]).endswith(b"\x02unknown")


@pytest.mark.parametrize("case", ["revision", "suffix"])
def test_unknown_asset_layout_is_retained_without_segment_recovery(case):
    kwargs = {"revision": 99} if case == "revision" else {"asset_suffix": b"unknown"}
    archive = fusion.read(document(**kwargs), strict=False)
    info = archive.read_document()
    assert info.status == "partial"
    unknown = info.assets[0]
    assert unknown.segment_status == "unsupported"
    assert (
        unknown.segments is None if case != "suffix" else unknown.segments is not None
    )
    region = unknown.unknown_regions[0]
    assert len(archive.read_span(region)) == region.length
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.read_document(strict=True)


def test_reference_preamble_is_opaque_but_unique_registry_is_framed():
    archive = fusion.read(document(references=1))
    info = archive.read_document()
    assert info.status == "complete"
    asset_info = info.assets[0]
    assert asset_info.change_counter is None
    assert asset_info.segments[0].role == "design"
    region = asset_info.unknown_regions[0]
    assert region.purpose == "asset_reference_preamble"
    assert archive.read_span(region) == u32(1) + u32(7)


def test_legacy_design_type_and_unknown_subtype_mode():
    def read_asset(payload):
        return fusion.read(
            make_zip(
                [
                    ("Manifest.dat", manifest()),
                    ("Root/Manifest.dat", payload),
                ]
            )[0],
            strict=False,
        )

    legacy = read_asset(asset(segment_type="Design"))
    assert legacy.read_document().assets[0].segments[0].role == "design"
    unknown = read_asset(asset(subtype_mode=2))
    info = unknown.read_document()
    assert info.status == "partial" and info.design_asset_id is None
    assert info.assets[0].role == "unresolved"
    assert unknown.read_span(info.assets[0].unknown_regions[0]).startswith(b"\x02")
    with pytest.raises(fusion.UnsupportedFeatureError):
        unknown.read_document(strict=True)


def test_metadata_byte_limit_and_invalid_utf16():
    for payload, error_type, code in [
        (b"x" * (1024 * 1024 + 1), fusion.ResourceLimitError, "limit.manifest_bytes"),
        (
            text("3-2-0-0") + text("FusionDocType") + u32(1) + b"\x00\xd8",
            fusion.FormatError,
            "manifest.invalid_utf16",
        ),
    ]:
        archive = fusion.read(make_zip([("Manifest.dat", payload)])[0], strict=False)
        with pytest.raises(error_type) as exc:
            archive.read_document()
        assert exc.value.code == code


def test_preview_ignores_nonimages_and_matches_document_guid_case():
    archive = fusion.read(
        document(
            preview=None,
            extra=[
                ("Root[Active]/Previews/index.dat", b"metadata"),
            ],
        )
    )
    assert archive.get_preview() is None
    assert fusion.read(document()).get_preview(document_id=DOC.upper()) == PNG


def test_preview_without_default_does_not_choose_between_images():
    archive = fusion.read(
        document(
            preview=None,
            extra=[
                ("Root[Active]/Previews/nested/small.png", PNG),
                ("Root[Active]/Previews/large.png", PNG),
            ],
        )
    )
    with pytest.raises(fusion.FormatError) as exc:
        archive.get_preview()
    assert exc.value.code == "preview.ambiguous"


def test_missing_and_ambiguous_asset_folders_are_not_guessed():
    for entries in [
        [("Manifest.dat", manifest())],
        [
            ("Manifest.dat", manifest()),
            ("Root[Active]/Manifest.dat", asset()),
            ("Root/stray", b"x"),
        ],
    ]:
        with pytest.raises(fusion.FormatError):
            fusion.read(make_zip(entries)[0], strict=False).read_document()


def test_multiple_design_assets_and_duplicate_guids_are_errors():
    for second in [OTHER, DESIGN]:
        entries = [
            ("Manifest.dat", manifest(bases=("Root", "Second"))),
            ("Root/Manifest.dat", asset()),
            ("Second/Manifest.dat", asset("Second", guid=second)),
        ]
        with pytest.raises(fusion.FormatError):
            fusion.read(make_zip(entries)[0], strict=False).read_document()


@pytest.mark.parametrize("preview", [None, b"", PNG])
def test_preview_absence_empty_and_present_are_distinct(preview):
    archive = fusion.read(document(preview=preview))
    assert archive.get_preview() == preview
    with pytest.raises(KeyError):
        archive.get_preview(document_id=OTHER)


def test_preview_crc_and_combined_metadata_budget():
    data = document()
    archive = fusion.read(data)
    preview = next(m for m in archive.members if m.name.endswith("small.png"))
    damaged = bytearray(data)
    damaged[preview.data_offset] ^= 1
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(bytes(damaged)).get_preview()
    assert exc.value.code == "zip.crc_mismatch"
    sizes = [
        m.uncompressed_size for m in archive.members if m.name.endswith("Manifest.dat")
    ]
    limit = sum(sizes) + len(PNG) - 1
    limits = fusion.Limits(
        max_member_bytes=max(sizes), max_total_uncompressed_bytes=limit
    )
    small = fusion.read(data, limits=limits)
    small.read_document()
    with pytest.raises(fusion.ResourceLimitError):
        small.get_preview()


def test_document_views_survive_parent_and_parallel_access():
    archive = fusion.read(document())
    with ThreadPoolExecutor(max_workers=4) as pool:
        ids = list(pool.map(lambda _, item=archive: item.read_document().id, range(16)))
    assert ids == [DOC] * 16
    info = archive.documents[0]
    segment = info.assets[0].segments[0]
    del archive
    gc.collect()
    assert segment.role == "design" and info.id == DOC
    with pytest.raises(AttributeError):
        info.id = "changed"


def test_f3z_document_and_preview_do_not_guess_root():
    archive = fusion.read(
        make_zip([("Manifest.json", b"{}"), ("child.f3d", document())])[0]
    )
    for operation in [
        archive.read_document,
    ]:
        with pytest.raises(fusion.UnsupportedFeatureError) as exc:
            operation()
        assert exc.value.code == "document.package_not_decoded"


@pytest.mark.parametrize("version,code", [("3-2-0-0", 0), ("9-0-0-0", 2)])
def test_cli_document_layer(version, code, tmp_path):
    path = tmp_path / "model.f3d"
    path.write_bytes(document(version=version))
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            "-c",
            (
                "import sys; sys.stdout.reconfigure(encoding='ascii'); "
                "from fusion_kit.cli import main; raise SystemExit(main(sys.argv[1:]))"
            ),
            "inspect",
            str(path),
            "--document",
            "--json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == code, result.stderr
    payload = json.loads(result.stdout)
    assert payload["scope"] == "f3d_document_and_design_registry"
    if code == 0:
        assert payload["manifest"]["name"] == "モデル😀"
