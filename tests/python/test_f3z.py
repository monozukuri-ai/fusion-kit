import copy
import gc
import hashlib
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

import fusion_kit as fusion
import pytest

from .manifest_fixtures import DOC, PNG, document
from .package_fixtures import package, values
from .zip_fixtures import make_zip, patch_entry


@pytest.mark.parametrize("method", [0, 8, 93])
@pytest.mark.parametrize("kind", ["XREF", "DERIVED"])
def test_package_layers_and_scoped_ids(method, kind):
    archive = fusion.read(package(values(kind), method=method))
    info = archive.read_package()
    assert info.status == "complete"
    assert archive.root_document_id == info.root_document_id == "root.f3d"
    assert {d.id for d in archive.documents} == {"root.f3d", "part.f3d"}
    # Identical Fusion GUIDs do not collapse distinct package members.
    assert [d.document_guid for d in info.documents] == [DOC, DOC]
    assert all(d.package_sha256 == archive.source_sha256 for d in info.documents)
    assert info.references[0].kind == archive.references[0].kind == kind
    assert info.references[0].neutron_data is None
    inner = next(
        r for r in info.redirections if r.document_id == "root.f3d"
    ).references[0]
    assert inner.neutron_data == "independent-data" and inner.neutron_role == "role"
    assert all(c.matches for c in info.comparisons)
    assert info.graphs[0].reachable_ids == [1, 2]
    assert info.to_dict() == json.loads(info.to_json())
    assert (
        info.to_dict()["occurrences"] == info.to_dict()["geometry"] == "not_requested"
    )
    assert archive.get_preview() == archive.get_preview(document_id="part.f3d") == PNG
    child = archive.open_document("root.f3d")
    assert child.read_document().id == DOC
    for sidecar in info.raw_sidecars:
        owner = (
            archive
            if sidecar.document_id is None
            else archive.open_document(sidecar.document_id)
        )
        raw = owner.read_span(sidecar.raw)
        assert hashlib.sha256(raw).hexdigest() == sidecar.raw.sha256
        assert json.loads(raw) == sidecar.value
    assert archive.to_dict()["relationships"] == "not_requested"


def mutation(case):
    manifest, desc, tables = data = values()
    nodes = desc["designDescription"]["designGraphs"][0]["designObjects"]
    group = nodes[0]["references"][0]
    row = tables["root.f3d"]["references"][0]
    if case == "root_missing":
        manifest.clear()
    elif case == "root_member_missing":
        manifest["root"] = "absent.f3d"
    elif case == "duplicate_object_id":
        nodes.append(copy.deepcopy(nodes[0]))
    elif case == "dangling_reference_id":
        group["ids"] = [999]
    elif case == "object_content_type_mismatch":
        nodes[1]["contentType"] = "f2d"
    elif case == "reference_role_mismatch":
        row["properties"][0]["neutronRole"]["value"] = "other"
    elif case == "redirections_references":
        tables["part.f3d"]["references"] = {"unexpected": 1}
    elif case == "redirections_schema":
        tables["root.f3d"]["schema-version"] = False
    elif case == "relationship_target":
        group["relationships"][0]["id"] = True
    elif case == "reference_property":
        row["properties"].append(copy.deepcopy(row["properties"][0]))
    elif case == "root_object_ambiguous":
        node = copy.deepcopy(nodes[0])
        node["id"] = 3
        nodes.append(node)
    elif case == "reference_cycle":
        nodes[1]["references"] = [
            {
                "type": "XREF",
                "ids": [1],
                "relationships": [{"id": 1, "metadata": {"neutronRole": "back"}}],
            }
        ]
    elif case == "root_declaration_mismatch":
        desc["designDescription"]["designGraphs"][0]["rootIds"] = [2]
    elif case == "description_schema":
        desc["version"] = "future"
    elif case == "reference_source_mismatch":
        row["from"] = "part.f3d"
    elif case == "reference_design_missing":
        tables["root.f3d"]["designs"] = [{"targetFileName": "root.f3d"}]
    elif case == "redirections_unavailable":
        tables["part.f3d"] = None
    elif case == "reference_layers_differ":
        row["type"] = "DERIVED"
    else:
        raise AssertionError(case)
    return data


@pytest.mark.parametrize(
    "case",
    [
        "root_missing",
        "root_member_missing",
        "duplicate_object_id",
        "dangling_reference_id",
        "object_content_type_mismatch",
        "reference_role_mismatch",
        "redirections_references",
        "redirections_schema",
        "relationship_target",
        "reference_property",
        "root_object_ambiguous",
        "reference_cycle",
        "root_declaration_mismatch",
        "description_schema",
        "reference_source_mismatch",
        "reference_design_missing",
        "redirections_unavailable",
        "reference_layers_differ",
    ],
)
def test_graph_findings_and_strict_cache_policy(case):
    archive = fusion.read(package(mutation(case)))
    result = archive.read_package(strict=False)
    assert case in {d.code for d in result.issues}
    assert result.status != "complete"
    expected = (
        fusion.UnsupportedFeatureError
        if result.status == "unsupported"
        else fusion.ReferenceResolutionError
    )
    with pytest.raises(expected):
        archive.read_package(strict=True)
    # strict failure must not erase the cached inspectable result.
    assert archive.read_package(strict=False).to_dict() == result.to_dict()


@pytest.mark.parametrize("kind", ["FUTURE", {"future": "kind"}])
def test_unknown_kind_is_preserved_in_original_sidecar(kind):
    archive = fusion.read(package(values(kind)), strict=False)
    result = archive.read_package()
    assert result.status == "unsupported"
    assert result.references[0].kind == (kind if isinstance(kind, str) else None)
    raw = next(
        s.value for s in result.raw_sidecars if s.raw.member == "DesignDescription.json"
    )
    assert (
        raw["designDescription"]["designGraphs"][0]["designObjects"][0]["references"][
            0
        ]["type"]
        == kind
    )


@pytest.mark.parametrize(
    "payload",
    [
        b'{"root":"root.f3d","root":"part.f3d"}',
        b'{"root":"root.f3d","unknown":{"x":1,"x":2}}',
        b'{"root":"root.f3d"} trailing',
        b"\xff",
        b'{"root":NaN}',
    ],
)
def test_invalid_json_is_fatal_in_partial_mode(payload):
    data = values()
    data = payload, data[1], data[2]
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(package(data), strict=False).read_package()
    assert exc.value.code == "json.invalid"


def test_json_preallocation_and_depth_limits():
    data = values()
    for payload in [
        b'{"root":"root.f3d","extra":[' + b"0," * 100000 + b"0]}",
        b"[" * 150 + b"]" * 150,
    ]:
        bad = payload, data[1], data[2]
        with pytest.raises((fusion.ResourceLimitError, fusion.FormatError)):
            fusion.read(package(bad), strict=False).read_package()


def test_json_size_limit_precedes_decompression():
    raw = patch_entry(
        package(method=8), "DesignDescription.json", size=16 * 1024 * 1024 + 1
    )
    with pytest.raises(fusion.ResourceLimitError) as exc:
        fusion.read(raw, strict=False).read_package()
    assert exc.value.code == "limit.json_bytes"


def test_child_metadata_unsupported_and_missing_sidecar_are_distinct():
    archive = fusion.read(
        package(overrides={"part.f3d": document(version="99")}), strict=False
    )
    info = archive.read_package()
    assert info.status == "unsupported"
    assert next(d for d in info.documents if d.id == "part.f3d").document_guid is None
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.read_package(strict=True)
    broken = make_zip(
        [("Manifest.json", b'{"root":"root.f3d"}'), ("root.f3d", document())]
    )[0]
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(broken, strict=False).read_package()
    assert exc.value.code == "package.sidecar_missing"


def test_same_object_ids_in_other_graph_remain_scoped_and_unreachable_warns():
    manifest, desc, tables = values()
    desc["designDescription"]["designGraphs"].append(
        {
            "rootIds": [],
            "designObjects": [
                {
                    "id": 1,
                    "relativePath": "other.f3d",
                    "contentType": "f3d",
                    "references": [],
                }
            ],
        }
    )
    tables["other.f3d"] = {
        "name": "RedirectionsStream",
        "schema-version": 0,
        "designs": [{"targetFileName": "other.f3d"}],
        "references": {},
    }
    result = fusion.read(package((manifest, desc, tables)), strict=False).read_package()
    assert "duplicate_object_id" not in {i.code for i in result.issues}
    assert result.graphs[1].unreachable_ids == [1]
    assert result.status == "partial"


def test_drawing_root_has_no_f3d_fallback():
    manifest, desc, tables = values()
    manifest["root"] = "drawing.f2d"
    desc["designDescription"]["designGraphs"][0]["designObjects"][0].update(
        relativePath="drawing.f2d", contentType="f2d"
    )
    archive = fusion.read(
        package((manifest, desc, tables), extra=[("drawing.f2d", b"opaque drawing")]),
        strict=False,
    )
    info = archive.read_package()
    assert info.status == "unsupported"
    assert archive.root_document_id is None
    assert next(d for d in info.documents if d.id == "drawing.f2d").metadata is None
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.open_document("drawing.f2d")
    with pytest.raises(fusion.ReferenceResolutionError):
        archive.get_preview()


def test_undocumented_child_is_retained_and_verified():
    archive = fusion.read(package(extra=[("orphan.f3d", document())]), strict=False)
    info = archive.read_package()
    assert info.undocumented_members == ["orphan.f3d"]
    assert "orphan.f3d" in [d.id for d in archive.documents]
    assert (
        archive.verify().status == "complete"
    )  # CRC scope is independent of graph status.


def test_recursive_bytes_and_entry_counts_include_nested_container_bytes():
    archive = fusion.read(package(method=8))
    children = [archive.open_document(d.id) for d in archive.documents]
    expected_bytes = sum(
        m.uncompressed_size for a in [archive, *children] for m in a.members
    )
    expected_entries = sum(len(a.members) for a in [archive, *children])
    report = archive.verify()
    assert report.scope == "package_archive_members"
    assert report.bytes_decompressed == expected_bytes
    assert report.members_verified == report.members_total == expected_entries
    assert report.to_dict()["nested_archives"] == "complete"
    assert archive.verify(recursive=False).members_total == len(archive.members)


@pytest.mark.parametrize("operation", ["package", "verify"])
@pytest.mark.parametrize("limit", ["depth", "entries", "bytes"])
def test_nested_limits_are_shared_and_fatal(operation, limit):
    raw = package()
    reference = fusion.read(raw)
    info = reference.read_package()
    kwargs = {
        "depth": {"max_nesting_depth": 1},
        "entries": {"max_entries": info.members_indexed - 1},
        "bytes": {
            "max_member_bytes": max(m.uncompressed_size for m in reference.members),
            "max_total_uncompressed_bytes": info.bytes_decompressed - 1,
        },
    }[limit]
    archive = fusion.read(raw, limits=fusion.Limits(**kwargs), strict=False)
    with pytest.raises(fusion.ResourceLimitError):
        archive.read_package() if operation == "package" else archive.verify(
            strict=False
        )


def test_package_preview_uses_package_budget_even_when_metadata_is_cached():
    raw = package()
    baseline = fusion.read(raw)
    info = baseline.read_package()
    limit = info.bytes_decompressed + len(PNG) - 1
    archive = fusion.read(
        raw,
        limits=fusion.Limits(
            max_member_bytes=max(m.uncompressed_size for m in baseline.members),
            max_total_uncompressed_bytes=limit,
        ),
    )
    archive.read_package()
    with pytest.raises(fusion.ResourceLimitError):
        archive.get_preview()


def test_corruption_inside_crc_valid_child_is_detected():
    child = fusion.read(package()).read_member("part.f3d")
    parsed = fusion.read(child)
    offset = next(
        m.data_offset for m in parsed.members if m.name.endswith("BulkStream.dat")
    )
    bad = bytearray(child)
    bad[offset] ^= 1
    archive = fusion.read(package(overrides={"part.f3d": bytes(bad)}))
    assert (
        archive.read_package().status == "complete"
    )  # BulkStream not decoded by metadata.
    report = archive.verify(strict=False)
    assert report.status == "invalid"
    assert any(
        d.source.endswith("!part.f3d") and d.code == "zip.crc_mismatch"
        for d in report.issues
    )
    with pytest.raises(fusion.FormatError) as exc:
        archive.verify()
    assert exc.value.report.scope == "package_archive_members"


def test_nested_f3z_is_walked_iteratively_and_depth_is_enforced():
    raw = make_zip([("Manifest.json", b"{}"), ("nested.f3z", package())])[0]
    with pytest.raises(fusion.ResourceLimitError):
        fusion.read(raw).verify(strict=False)
    assert (
        fusion.read(raw, limits=fusion.Limits(max_nesting_depth=3)).verify().status
        == "complete"
    )


def test_package_views_parallel_cache_and_child_lifetime():
    archive = fusion.read(package())
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(
            pool.map(
                lambda _, owner=archive: owner.read_package().root_document_id,
                range(12),
            )
        )
    assert results == ["root.f3d"] * 12
    info = archive.read_package()
    ref = archive.references[0]
    child = archive.open_document("part.f3d")
    del archive
    gc.collect()
    assert child.get_preview() == PNG and info.documents[0].metadata.id == DOC
    assert ref.kind == "XREF"
    with pytest.raises(AttributeError):
        ref.kind = "changed"
    with pytest.raises(KeyError):
        fusion.read(package()).open_document(DOC)


@pytest.mark.parametrize(
    "case,expected", [("valid", 0), ("invalid", 1), ("unsupported", 2)]
)
def test_cli_package_scope_and_exit_code(case, expected, tmp_path):
    data = (
        values("FUTURE")
        if case == "unsupported"
        else mutation("root_missing")
        if case == "invalid"
        else values()
    )
    path = tmp_path / "assembly.f3z"
    path.write_bytes(package(data))
    done = subprocess.run(
        [
            sys.executable,
            "-I",
            "-m",
            "fusion_kit",
            "inspect",
            str(path),
            "--package",
            "--json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode == expected, done.stderr
    assert json.loads(done.stdout)["scope"] == "f3z_document_dependencies"
