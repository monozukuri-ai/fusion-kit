use std::io::{Cursor, Write};

use fusion_kit_core::{Archive, ErrorKind, Limits, ReadOptions, Status};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fixture(method: CompressionMethod, payload: &[u8]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("Manifest.dat", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"m").unwrap();
    writer
        .start_file(
            "payload.bin",
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
    writer.write_all(payload).unwrap();
    writer.finish().unwrap().into_inner()
}

fn options(input: u64, member: u64, total: u64, entries: u64) -> ReadOptions {
    ReadOptions {
        limits: Limits::new(input, member, total, entries, 2).unwrap(),
        strict: true,
    }
}

#[test]
fn compression_methods_and_binary_payload_round_trip() {
    let payload: Vec<u8> = (0..100_000).map(|i| (i % 251) as u8).collect();
    for method in [
        CompressionMethod::Stored,
        CompressionMethod::Deflated,
        CompressionMethod::Zstd,
    ] {
        let data = fixture(method, &payload);
        let archive = Archive::from_bytes(&data, ReadOptions::default()).unwrap();
        assert_eq!(archive.read_member("payload.bin").unwrap(), payload);
        let report = archive.verify(true).unwrap();
        assert_eq!(report.status, Status::Complete);
        assert_eq!(report.members_verified, 2);
        assert_eq!(report.bytes_decompressed, 100_001);
        assert_eq!(report.nested_archives, "not_requested");
    }
}

#[test]
fn payload_crc_corruption_is_distinct_from_a_valid_index() {
    let mut data = fixture(CompressionMethod::Stored, b"original");
    let offset = Archive::from_bytes(&data, ReadOptions::default())
        .unwrap()
        .members()[1]
        .data_offset as usize;
    data[offset] ^= 1;
    let archive = Archive::from_bytes(&data, ReadOptions::default()).unwrap();
    let error = archive.read_member("payload.bin").unwrap_err();
    assert_eq!(error.diagnostic.code, "zip.crc_mismatch");
    assert_eq!(error.diagnostic.member.as_deref(), Some("payload.bin"));
    let report = archive.verify(false).unwrap();
    assert_eq!(report.status, Status::Invalid);
    assert_eq!(report.members_verified, 1);
    assert_eq!(
        archive.verify(true).unwrap_err().report.unwrap().status,
        Status::Invalid
    );
}

#[test]
fn declared_and_actual_resource_work_is_bounded() {
    let data = fixture(CompressionMethod::Deflated, &[42; 100]);
    assert_eq!(
        Archive::from_bytes(&data, options(1, 100, 100, 2))
            .unwrap_err()
            .kind,
        ErrorKind::ResourceLimit
    );
    assert_eq!(
        Archive::from_bytes(&data, options(10_000, 100, 100, 1))
            .unwrap_err()
            .diagnostic
            .code,
        "limit.entries"
    );
    let archive = Archive::from_bytes(&data, options(10_000, 50, 100, 2)).unwrap();
    assert_eq!(
        archive
            .read_member("payload.bin")
            .unwrap_err()
            .diagnostic
            .code,
        "limit.member_bytes"
    );
    let archive = Archive::from_bytes(&data, options(10_000, 100, 100, 2)).unwrap();
    assert!(archive.read_member("payload.bin").is_ok());
    assert_eq!(
        archive.verify(false).unwrap_err().diagnostic.code,
        "limit.total_bytes"
    );
}

#[test]
fn every_truncated_prefix_is_rejected_without_panicking() {
    let data = fixture(CompressionMethod::Deflated, b"payload");
    for end in 0..data.len() {
        assert!(
            Archive::from_bytes(&data[..end], ReadOptions::default()).is_err(),
            "prefix {end}"
        );
    }
}

#[test]
fn duplicate_and_traversal_names_are_rejected_before_lookup() {
    let mut traversal = fixture(CompressionMethod::Stored, b"payload");
    for at in 0..=traversal.len() - 11 {
        if &traversal[at..at + 11] == b"payload.bin" {
            traversal[at..at + 11].copy_from_slice(b"../evil.bin");
        }
    }
    assert_eq!(
        Archive::from_bytes(&traversal, ReadOptions::default())
            .unwrap_err()
            .diagnostic
            .code,
        "zip.unsafe_path"
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["Manifest.dat", "Manifest.dax"] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
    }
    let mut duplicate = writer.finish().unwrap().into_inner();
    for at in 0..=duplicate.len() - 12 {
        if &duplicate[at..at + 12] == b"Manifest.dax" {
            duplicate[at..at + 12].copy_from_slice(b"Manifest.dat");
        }
    }
    assert_eq!(
        Archive::from_bytes(&duplicate, ReadOptions::default())
            .unwrap_err()
            .diagnostic
            .code,
        "zip.duplicate_member"
    );
}

#[test]
fn read_options_default_to_strict() {
    assert!(ReadOptions::default().strict);
}

#[test]
fn public_spans_verify_hash_bounds_and_source() {
    let archive = Archive::from_bytes(
        &fixture(CompressionMethod::Stored, b"abc"),
        ReadOptions::default(),
    )
    .unwrap();
    let mut span = fusion_kit_core::RawSpan {
        source_sha256: archive.source_sha256().into(),
        member: "payload.bin".into(),
        offset: 0,
        length: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        purpose: "contract_test".into(),
    };
    assert_eq!(archive.read_span(&span).unwrap(), b"abc");
    span.sha256 = "wrong".into();
    let error = archive.read_span(&span).unwrap_err();
    assert_eq!(error.diagnostic.code, "span.hash_mismatch");
    assert_eq!(error.diagnostic.source.as_deref(), Some("<bytes>"));
    span.length = u64::MAX;
    assert_eq!(
        archive.read_span(&span).unwrap_err().diagnostic.code,
        "span.bounds"
    );
    span.source_sha256 = "other".into();
    assert_eq!(
        archive.read_span(&span).unwrap_err().diagnostic.code,
        "span.source_mismatch"
    );
}

#[test]
fn single_byte_mutations_do_not_panic_or_trust_unbounded_counts() {
    let data = fixture(CompressionMethod::Deflated, b"payload");
    for position in 0..data.len() {
        let mut changed = data.clone();
        changed[position] ^= 0xff;
        if let Ok(archive) = Archive::from_bytes(&changed, options(4096, 4096, 4096, 10)) {
            let _ = archive.verify(false);
        }
    }
}
