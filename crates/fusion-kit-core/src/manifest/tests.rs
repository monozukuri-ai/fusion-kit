use super::*;

const GUID: &str = "11111111-2222-4333-8444-555555555555";

fn word(n: u32) -> Vec<u8> {
    n.to_le_bytes().to_vec()
}
fn text(s: &str, wide: bool) -> Vec<u8> {
    let bytes = if wide {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        s.as_bytes().to_vec()
    };
    [word((bytes.len() / if wide { 2 } else { 1 }) as u32), bytes].concat()
}
fn tail(ending: &[u8], bases: &[&str]) -> Vec<u8> {
    let mut out = text(GUID, true);
    out.extend(word(bases.len() as u32));
    for base in bases {
        out.extend(text(base, true));
    }
    out.extend(word(0));
    out.extend(ending);
    out
}
fn prefix(current: bool) -> Vec<u8> {
    [
        text("3-2-0-0", false),
        text("FusionDocType", false),
        text(".f3d", true),
        text("Fusion Document", true),
        text("", true),
        text(GUID, true),
        text(GUID, true),
        if current {
            [word(1234), word(1), word(2), word(0)].concat()
        } else {
            [word(333), word(0)].concat()
        },
        word(1),
        text("Fusion", false),
        word(20),
    ]
    .concat()
}
fn parse_test(data: &[u8]) -> Result<DocumentManifest, Error> {
    parse(data, "Manifest.dat", "source-hash")
}

#[test]
fn flag_only_and_encoded_empty_are_distinct() {
    let omitted = parse_test(&[prefix(true), tail(&[1], &["Root"])].concat()).unwrap();
    assert_eq!(omitted.tail.unwrap().terminal.marker, None);
    let empty = parse_test(
        &[
            prefix(true),
            tail(&[vec![1], text("", true)].concat(), &["Root"]),
        ]
        .concat(),
    )
    .unwrap();
    assert_eq!(empty.tail.unwrap().terminal.marker.as_deref(), Some(""));
    assert_eq!(empty.issues[0].code, "manifest.unknown_marker");
}

#[test]
fn truncated_present_string_is_not_absence() {
    for ending in [
        vec![1, 0],
        vec![1, 0, 0],
        vec![1, 0, 0, 0],
        [vec![1], word(9), vec![b'N', 0]].concat(),
    ] {
        let error = parse_test(&[prefix(true), tail(&ending, &["Root"])].concat()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Format);
    }
}

#[test]
fn terminal_variants_and_utf16_code_units() {
    let variants = [
        (Vec::new(), "no-terminal"),
        ([vec![1], text("NA_EXPORT", true)].concat(), "flag-1-marker"),
        (
            [vec![1], text("NA_OFFLINESAVE", true)].concat(),
            "flag-1-marker",
        ),
        (
            [vec![1], text("urn:example:1", true)].concat(),
            "flag-1-lineage",
        ),
        ([vec![0], text("モデル😀", true)].concat(), "flag-0-name"),
        (
            [
                vec![0],
                text("model", true),
                text("urn:example:1", true),
                SUFFIX.to_vec(),
            ]
            .concat(),
            "flag-0-name-lineage-suffix",
        ),
    ];
    for (ending, form) in variants {
        let manifest = parse_test(&[prefix(true), tail(&ending, &["Root"])].concat()).unwrap();
        assert_eq!(manifest.tail.unwrap().terminal.form, form);
    }
}

#[test]
fn unknown_terminal_and_trailing_junk_are_not_repaired() {
    let unknown = parse_test(&[prefix(true), tail(&[2], &["Root"])].concat()).unwrap();
    assert!(!unknown.framing_complete);
    assert_eq!(unknown.issues[0].code, "manifest.unknown_tail");
    let junk = [vec![1], text("NA_EXPORT", true), b"junk".to_vec()].concat();
    assert!(parse_test(&[prefix(true), tail(&junk, &["Root"])].concat()).is_err());
}

#[test]
fn duplicate_unsafe_or_empty_asset_runs_fail() {
    for bases in [vec!["a", "a"], vec!["../a"], vec!["a\\b"], vec![]] {
        assert_eq!(
            parse_test(&[prefix(true), tail(&[1], &bases)].concat())
                .unwrap_err()
                .kind,
            ErrorKind::Format
        );
    }
}

#[test]
fn generation_forms_and_opaque_region_match_exact_offsets() {
    for current in [false, true] {
        let start = prefix(current).len();
        let opaque = b"\0\x01opaque";
        let data = [prefix(current), opaque.to_vec(), tail(&[1], &["Root"])].concat();
        let parsed = parse_test(&data).unwrap();
        let region = parsed.generation_extension.unwrap();
        assert_eq!(region.offset, start as u64);
        assert_eq!(region.length, opaque.len() as u64);
        assert_eq!(parsed.tail.unwrap().offset, (start + opaque.len()) as u64);
    }
}

#[test]
fn unknown_version_is_unsupported() {
    assert_eq!(
        parse_test(&text("9-0-0-0", false)).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn huge_schema_count_fails_before_entries_are_read() {
    let mut data = prefix(true);
    data.truncate(data.len() - (4 + 4 + 6 + 4));
    data.extend(word(u32::MAX));
    assert_eq!(
        parse_test(&data).unwrap_err().diagnostic.code,
        "limit.manifest_items"
    );
}

#[test]
fn two_complete_tail_framings_are_ambiguous() {
    let nested = tail(&[], &["Root"]);
    let ending = [vec![0], word((nested.len() / 2) as u32), nested].concat();
    assert_eq!(
        parse_test(&[prefix(true), tail(&ending, &["Root"])].concat())
            .unwrap_err()
            .diagnostic
            .code,
        "manifest.ambiguous_tail"
    );
}

#[test]
fn lineage_non_ascii_never_panics() {
    let ending = [vec![0], text("model", true), text("ああ", true)].concat();
    assert_eq!(
        parse_test(&[prefix(true), tail(&ending, &["Root"])].concat())
            .unwrap_err()
            .kind,
        ErrorKind::Format
    );
}

#[test]
fn candidate_scan_has_a_work_bound() {
    let fake = [text(GUID, true), word(0), word(0)].concat().repeat(257);
    let data = [prefix(true), fake, tail(&[1], &["Root"])].concat();
    assert_eq!(
        parse_test(&data).unwrap_err().diagnostic.code,
        "limit.manifest_candidates"
    );
}
