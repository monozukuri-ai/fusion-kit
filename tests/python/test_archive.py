import gc
import hashlib
import json
import subprocess
import sys
import zlib

import fusion_kit as fusion
import pytest

from .zip_fixtures import make_zip, patch_entry, zstd_raw


@pytest.mark.parametrize("method", [0, 8, 93])
@pytest.mark.parametrize("descriptor", [False, True])
def test_independent_compression_and_descriptor_fixtures(method, descriptor, tmp_path):
    data, _ = make_zip(method=method, descriptor=descriptor)
    path = tmp_path / "日本語.f3d"
    path.write_bytes(data)
    for archive in [fusion.read(data), fusion.readfile(path)]:
        assert archive.kind == "f3d"
        assert archive.source_sha256 == hashlib.sha256(data).hexdigest()
        assert archive.source_size == len(data)
        assert archive.read_member("payload.bin") == b"hello\x00world"
        assert archive.members[1].compression_method == method
        summary = archive.to_dict()
        assert summary == json.loads(archive.to_json())
        assert summary["verification"] == "not_run"
        assert summary["manifest"] == "not_requested"
        report = archive.verify()
        assert report.status == "complete"
        assert report.members_verified == report.members_total == 2
        assert report.bytes_decompressed == 19
        assert report.to_dict()["nested_archives"] == "not_requested"


def test_snapshot_survives_source_mutation_and_views_survive_parent(tmp_path):
    data, _ = make_zip()
    path = tmp_path / "source.f3d"
    path.write_bytes(data)
    archive = fusion.readfile(path)
    member = archive.members[1]
    path.write_bytes(b"changed")
    assert archive.read_member("payload.bin") == b"hello\x00world"
    assert archive.source_sha256 == hashlib.sha256(data).hexdigest()
    del archive
    gc.collect()
    assert member.name == "payload.bin"
    with pytest.raises(AttributeError):
        member.name = "other"


def test_utf8_member_name_round_trip():
    data, _ = make_zip([("Manifest.dat", b"m"), ("資料/日本語.bin", b"data")])
    archive = fusion.read(data)
    assert archive.members[1].name == "資料/日本語.bin"
    assert archive.read_member("資料/日本語.bin") == b"data"


def test_crc_failure_has_context_and_invalid_report():
    data, offsets = make_zip()
    changed = bytearray(data)
    changed[offsets["payload.bin"]] ^= 1
    archive = fusion.read(bytes(changed))
    with pytest.raises(fusion.FormatError) as exc:
        archive.read_member("payload.bin")
    assert exc.value.code == "zip.crc_mismatch"
    assert exc.value.member == "payload.bin"
    assert exc.value.offset == offsets["payload.bin"]
    assert exc.value.diagnostic["stage"] == "verification"
    assert archive.verify(strict=False).status == "invalid"
    with pytest.raises(fusion.FormatError) as exc:
        archive.verify()
    assert exc.value.report.status == "invalid"


@pytest.mark.parametrize("change", ["input", "member", "total", "count"])
@pytest.mark.parametrize("source", ["bytes", "file"])
def test_resource_limits_remain_fatal_with_partial_mode(change, source, tmp_path):
    data, _ = make_zip(method=8)
    kwargs = {
        "input": {"max_input_bytes": len(data) - 1},
        "member": {"max_member_bytes": 10},
        "total": {"max_member_bytes": 11, "max_total_uncompressed_bytes": 18},
        "count": {"max_entries": 1},
    }[change]
    limits = fusion.Limits(**kwargs)
    path = tmp_path / "limits.f3d"
    path.write_bytes(data)
    with pytest.raises(fusion.ResourceLimitError):
        archive = (
            fusion.read(data, limits=limits, strict=False)
            if source == "bytes"
            else fusion.readfile(path, limits=limits, strict=False)
        )
        archive.verify(strict=False)


@pytest.mark.parametrize(
    "name",
    [
        "../escape",
        "/absolute",
        "C:/drive",
        "a\\b",
        "a//b",
        "a/./b",
        "a/../b",
        "nul\x00name",
    ],
)
def test_unsafe_names_are_rejected(name):
    data, _ = make_zip([("Manifest.dat", b"m"), (name, b"x")])
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(data, strict=False)
    assert exc.value.code == "zip.unsafe_path"


def test_duplicate_names_are_not_silently_deduplicated():
    data, _ = make_zip([("Manifest.dat", b"a"), ("Manifest.dat", b"b")])
    with pytest.raises(fusion.FormatError) as exc:
        fusion.read(data)
    assert exc.value.code == "zip.duplicate_member"


@pytest.mark.parametrize("mode", ["method", "encrypted"])
def test_unsupported_members_remain_visible_in_partial_mode(mode):
    data, _ = make_zip()
    data = patch_entry(
        data,
        "payload.bin",
        **({"method": 1234} if mode == "method" else {"flags": 0x801}),
    )
    with pytest.raises(fusion.UnsupportedFeatureError):
        fusion.read(data)
    archive = fusion.read(data, strict=False)
    assert archive.status == "partial"
    assert len(archive.members) == 2
    assert archive.issues[0].member == "payload.bin"
    assert archive.verify(strict=False).status == "partial"


def test_declared_size_cannot_hide_expansion():
    data, _ = make_zip(method=8)
    data = patch_entry(data, "payload.bin", size=1)
    archive = fusion.read(data)
    with pytest.raises(fusion.FormatError) as exc:
        archive.read_member("payload.bin")
    assert exc.value.code == "zip.size_mismatch"


@pytest.mark.parametrize("extra", [b"", b"trailing"])
def test_deflate_end_marker_and_compressed_range_are_checked(extra):
    encoder = zlib.compressobj(wbits=-15)
    compressed = encoder.compress(b"x") + encoder.flush()
    altered = compressed[:-1] if not extra else compressed + extra
    data, _ = make_zip(
        [("Manifest.dat", b"m"), ("payload.bin", b"x")],
        method=8,
        compressed_override={"payload.bin": altered},
    )
    archive = fusion.read(data)
    with pytest.raises(fusion.FormatError):
        archive.read_member("payload.bin")


def test_zstd_window_budget_is_applied_before_large_allocation():
    data, _ = make_zip(
        method=93,
        compressed_override={"payload.bin": zstd_raw(b"hello\x00world", window=0x20)},
    )
    archive = fusion.read(data, limits=fusion.Limits(max_member_bytes=1024))
    with pytest.raises(fusion.ResourceLimitError) as exc:
        archive.verify(strict=False)
    assert exc.value.code == "limit.zstd_window"


@pytest.mark.parametrize("method", [8, 93])
def test_bad_members_still_consume_cumulative_expansion_budget(method):
    entries = [("Manifest.dat", b"m")] + [(f"bad{i}", b"xxxx") for i in range(10)]
    data, _ = make_zip(entries, method=method)
    for i in range(10):
        data = patch_entry(data, f"bad{i}", size=1)
    archive = fusion.read(
        data, limits=fusion.Limits(max_member_bytes=16, max_total_uncompressed_bytes=16)
    )
    with pytest.raises(fusion.ResourceLimitError) as exc:
        archive.verify(strict=False)
    assert exc.value.code == "limit.total_bytes"


def test_read_only_bytes_api_rejects_mutable_buffers():
    with pytest.raises(TypeError):
        fusion.read(bytearray(make_zip()[0]))


def test_zstd_concatenated_frames_are_all_verified():
    combined = zstd_raw(b"hello") + zstd_raw(b"\x00world")
    data, _ = make_zip(method=93, compressed_override={"payload.bin": combined})
    assert fusion.read(data).read_member("payload.bin") == b"hello\x00world"


def test_zstd_full_output_without_final_block_is_not_success():
    frame = bytearray(zstd_raw(b"hello\x00world"))
    frame[6] &= 0xFE  # Clear the last-block bit, leaving a missing next block.
    data, _ = make_zip(method=93, compressed_override={"payload.bin": bytes(frame)})
    with pytest.raises(fusion.FormatError):
        fusion.read(data).verify()


def test_truncated_zip_and_non_fusion_zip_fail():
    data, _ = make_zip()
    for invalid in [data[:-1], b"not a zip", make_zip([("other", b"x")])[0]]:
        with pytest.raises(fusion.FormatError):
            fusion.read(invalid)


def test_missing_file_and_member_keep_distinct_exception_types(tmp_path):
    with pytest.raises(FileNotFoundError):
        fusion.readfile(tmp_path / "missing.f3d")
    archive = fusion.read(make_zip()[0])
    with pytest.raises(KeyError) as exc:
        archive.read_member("absent")
    assert exc.value.code == "zip.member_not_found"


def test_f3z_outer_only_verification_remains_explicit():
    data, _ = make_zip([("Manifest.json", b"{}"), ("child.f3d", b"opaque child")])
    archive = fusion.read(data)
    assert archive.kind == "f3z"
    assert archive.verify(recursive=False).to_dict()["scope"] == "outer_archive_members"
    assert archive.to_dict()["relationships"] == "not_requested"


@pytest.mark.parametrize(
    "case,expected", [("valid", 0), ("corrupt", 1), ("unsupported", 2)]
)
def test_cli_json_and_exit_status(case, expected, tmp_path):
    data, offsets = make_zip()
    if case == "corrupt":
        changed = bytearray(data)
        changed[offsets["payload.bin"]] ^= 1
        data = bytes(changed)
    elif case == "unsupported":
        data = patch_entry(data, "payload.bin", method=1234)
    path = tmp_path / "case.f3d"
    path.write_bytes(data)
    done = subprocess.run(
        [sys.executable, "-I", "-m", "fusion_kit", "verify", str(path), "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode == expected, done.stderr
    assert (
        json.loads(done.stdout)["status"]
        == {0: "complete", 1: "invalid", 2: "partial"}[expected]
    )
