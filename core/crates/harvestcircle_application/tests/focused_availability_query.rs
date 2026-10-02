use std::error::Error;
use std::fmt::Write;

use harvestcircle_application::{
    AvailabilityLocalQueryScope, ScopedAvailabilityQuery, SessionGeneration,
};
use harvestcircle_domain::availability::query::{EventTimestamp, FoodAvailabilityStatus};
use harvestcircle_domain::{
    AVAILABILITY_CURSOR_MAX_BYTES, AVAILABILITY_PAGE_DEFAULT_ROWS, AVAILABILITY_PAGE_MAX_ROWS,
    AVAILABILITY_QUERY_TEXT_MAX_BYTES, AvailabilityEventVersion, AvailabilityOrderKey,
    AvailabilityPage, AvailabilityPageContinuation, AvailabilityPageCursor, AvailabilityPageLimit,
    AvailabilityQueryContext, AvailabilityQueryError, AvailabilityQueryFilters,
    AvailabilityQueryFingerprint, AvailabilitySearchText, PublicKey, PublicPublisher,
};

const AUTHOR_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const OTHER_AUTHOR_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
// Independent pre-production SHA-256 of the specified v1 frame, not an encoder result.
const GOLDEN_FINGERPRINT_HEX: &str =
    "3db9ea644546b310f06abf524cfa0248fb5b1757e8a34620a4c820ba16a20276";
const GOLDEN_FINGERPRINT_BYTES: [u8; 32] = [
    0x3d, 0xb9, 0xea, 0x64, 0x45, 0x46, 0xb3, 0x10, 0xf0, 0x6a, 0xbf, 0x52, 0x4c, 0xfa, 0x02, 0x48,
    0xfb, 0x5b, 0x17, 0x57, 0xe8, 0xa3, 0x46, 0x20, 0xa4, 0xc8, 0x20, 0xba, 0x16, 0xa2, 0x02, 0x76,
];

fn author() -> PublicKey {
    PublicKey::from_hex(AUTHOR_HEX).expect("fixed valid public curve author")
}

fn other_author() -> PublicKey {
    PublicKey::from_hex(OTHER_AUTHOR_HEX).expect("second fixed valid public curve author")
}

fn publisher() -> PublicPublisher {
    PublicPublisher::from_public_key(author())
}

fn other_publisher() -> PublicPublisher {
    PublicPublisher::from_public_key(other_author())
}

fn context(
    context_id: [u8; 32],
    store_generation: [u8; 32],
    source_revision: u64,
    projection_generation: u64,
) -> AvailabilityQueryContext {
    AvailabilityQueryContext::new(
        context_id,
        store_generation,
        source_revision,
        projection_generation,
    )
    .expect("nonzero structural query identities")
}

fn base_context() -> AvailabilityQueryContext {
    context([1; 32], [2; 32], 5, 7)
}

fn filters(
    search: Option<&str>,
    publisher: Option<PublicPublisher>,
    status: Option<FoodAvailabilityStatus>,
) -> AvailabilityQueryFilters {
    AvailabilityQueryFilters::new(
        search.map(|text| AvailabilitySearchText::new(text).expect("bounded cached search text")),
        publisher,
        status,
    )
}

fn base_filters() -> AvailabilityQueryFilters {
    filters(Some("a"), None, Some(FoodAvailabilityStatus::Active))
}

fn scope(
    owner: PublicKey,
    context: AvailabilityQueryContext,
    session_generation: u64,
) -> AvailabilityLocalQueryScope {
    AvailabilityLocalQueryScope::new(
        owner,
        context,
        SessionGeneration::from_value(session_generation),
    )
}

fn base_scope() -> AvailabilityLocalQueryScope {
    scope(author(), base_context(), 3)
}

fn base_fingerprint() -> AvailabilityQueryFingerprint {
    AvailabilityQueryFingerprint::new(
        author(),
        &base_context(),
        3,
        &base_filters(),
        AvailabilityPageLimit::default(),
    )
}

fn base_query() -> ScopedAvailabilityQuery {
    ScopedAvailabilityQuery::new(
        base_scope(),
        base_filters(),
        AvailabilityPageLimit::default(),
        None,
    )
    .expect("pure local structural query")
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("writing hexadecimal into a string");
    }
    text
}

fn version(bytes: [u8; 32]) -> AvailabilityEventVersion {
    AvailabilityEventVersion::from_hex(&hex(&bytes)).expect("exact public event ID")
}

fn order(timestamp: u64, event_id: [u8; 32]) -> AvailabilityOrderKey {
    AvailabilityOrderKey::new(EventTimestamp::new(timestamp), version(event_id))
}

fn canonical_cursor(timestamp_hex: &str, event_id: [u8; 32]) -> String {
    format!(
        "hcq1:{GOLDEN_FINGERPRINT_HEX}:{timestamp_hex}:{}",
        hex(&event_id)
    )
}

fn text_at_byte_count(unit: &str, count: usize) -> String {
    unit.repeat(count / unit.len()) + &"x".repeat(count % unit.len())
}

fn assert_distinct(fingerprints: &[AvailabilityQueryFingerprint]) {
    for (index, fingerprint) in fingerprints.iter().enumerate() {
        for other in &fingerprints[index + 1..] {
            assert_ne!(
                fingerprint, other,
                "different framed requests must remain distinct"
            );
        }
    }
}

#[test]
fn page_limits_admit_default_and_exact_maximum() {
    assert_eq!(AVAILABILITY_PAGE_DEFAULT_ROWS, 50_u16);
    assert_eq!(AVAILABILITY_PAGE_MAX_ROWS, 100_u16);
    assert_eq!(AvailabilityPageLimit::default().rows(), 50);

    for rows in 1..=100 {
        assert_eq!(
            AvailabilityPageLimit::new(rows)
                .expect("admitted row limit")
                .rows(),
            rows
        );
    }
    for rows in [0, 101, u16::MAX] {
        assert_eq!(
            AvailabilityPageLimit::new(rows).expect_err("outside the exact row bounds"),
            AvailabilityQueryError::InvalidInput,
        );
    }
}

#[test]
fn search_text_preserves_exact_utf8_with_byte_boundaries() {
    assert_eq!(AVAILABILITY_QUERY_TEXT_MAX_BYTES, 512_usize);
    for text in [
        "",
        " ",
        "  farm:%_\\\t\n\0🥕  ",
        "é",
        "e\u{301}",
        "a\u{200d}b",
    ] {
        let search = AvailabilitySearchText::new(text).expect("opaque cached search text");
        assert_eq!(search.as_str().as_bytes(), text.as_bytes());
    }
    assert_ne!(
        AvailabilitySearchText::new("é")
            .expect("precomposed")
            .as_str(),
        AvailabilitySearchText::new("e\u{301}")
            .expect("combining")
            .as_str(),
    );

    for unit in ["x", "é", "🥕", "e\u{301}"] {
        for count in [511, 512] {
            let text = text_at_byte_count(unit, count);
            assert_eq!(text.len(), count);
            assert_eq!(
                AvailabilitySearchText::new(&text)
                    .expect("exact UTF-8 byte boundary")
                    .as_str(),
                text,
            );
        }
        let text = text_at_byte_count(unit, 513);
        assert_eq!(text.len(), 513);
        assert_eq!(
            AvailabilitySearchText::new(&text).expect_err("one byte over the text cap"),
            AvailabilityQueryError::InputTooLarge,
        );
    }
}

#[test]
fn filters_keep_optional_search_publisher_and_shared_status() {
    for search in [None, Some(""), Some(" \té\0%_\n")] {
        for publisher in [None, Some(publisher()), Some(other_publisher())] {
            for status in [
                None,
                Some(FoodAvailabilityStatus::Active),
                Some(FoodAvailabilityStatus::Sold),
            ] {
                let value = filters(search, publisher, status);
                assert_eq!(value.search().map(AvailabilitySearchText::as_str), search);
                assert_eq!(value.publisher(), publisher);
                assert_eq!(value.status(), status);
            }
        }
    }
    assert_eq!(FoodAvailabilityStatus::Active.as_str(), "active");
    assert_eq!(FoodAvailabilityStatus::Sold.as_str(), "sold");
    assert!(filters(None, None, None).search().is_none());
    assert_eq!(
        filters(Some(""), None, None)
            .search()
            .expect("present empty search")
            .as_str(),
        ""
    );
}

#[test]
fn context_binds_nonzero_identity_and_full_width_generations() {
    for (source_revision, projection_generation) in [
        (0, 0),
        (5, 7),
        (1_u64 << 63, (1_u64 << 63) + 1),
        (u64::MAX, u64::MAX),
    ] {
        let value = context([1; 32], [2; 32], source_revision, projection_generation);
        assert_eq!(value.context_id(), &[1; 32]);
        assert_eq!(value.store_generation(), &[2; 32]);
        assert_eq!(value.source_revision(), source_revision);
        assert_eq!(value.projection_generation(), projection_generation);
    }
    for index in [0, 16, 31] {
        let mut nonzero = [0; 32];
        nonzero[index] = 1;
        let value = context(nonzero, nonzero, 0, 0);
        assert_eq!(value.context_id(), &nonzero);
        assert_eq!(value.store_generation(), &nonzero);
    }
    for (context_id, store_generation) in
        [([0; 32], [2; 32]), ([1; 32], [0; 32]), ([0; 32], [0; 32])]
    {
        assert_eq!(
            AvailabilityQueryContext::new(context_id, store_generation, 0, u64::MAX)
                .expect_err("zero opaque identity"),
            AvailabilityQueryError::InvalidInput,
        );
    }
}

#[test]
fn ordering_uses_descending_signed_time_then_lowest_event_id() {
    let mut middle_id = [0; 32];
    middle_id[16] = 1;
    let mut last_id = [0; 32];
    last_id[31] = 1;
    let expected = vec![
        order(u64::MAX, [0; 32]),
        order(u64::MAX, [u8::MAX; 32]),
        order(u64::MAX - 1, [0; 32]),
        order(1_u64 << 63, [0; 32]),
        order((1_u64 << 63) - 1, [0; 32]),
        order(10, [0; 32]),
        order(10, last_id),
        order(10, middle_id),
        order(10, [1; 32]),
        order(0, [u8::MAX; 32]),
    ];
    let mut reversed = expected.clone();
    reversed.reverse();
    reversed.sort();
    assert_eq!(reversed, expected);
    let permutation = [7, 3, 9, 1, 5, 0, 8, 4, 2, 6];
    let mut permuted: Vec<_> = permutation
        .into_iter()
        .map(|index| expected[index])
        .collect();
    permuted.sort();
    assert_eq!(permuted, expected);
    for rotation in 1..expected.len() {
        let mut input = expected.clone();
        input.rotate_left(rotation);
        input.sort();
        assert_eq!(input, expected);
    }
    assert_eq!(expected[0].created_at().as_u64(), u64::MAX);
    assert_eq!(expected[3].created_at().as_u64(), 1_u64 << 63);
    assert_eq!(expected[6].version().event_id().as_bytes(), &last_id);
    assert_eq!(
        order(10, middle_id).cmp(&order(10, middle_id)),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        order(0, [0; 32]).version(),
        order(u64::MAX, [0; 32]).version()
    );
    assert!(order(u64::MAX, [0; 32]) < order(0, [0; 32]));
}

#[test]
fn continuation_position_is_strict_for_time_and_id_ties() {
    let mut last_id = [0; 32];
    last_id[31] = 1;
    let mut middle_id = [0; 32];
    middle_id[16] = 1;
    let positions = [
        order(u64::MAX, [0; 32]),
        order(1_u64 << 63, [0; 32]),
        order((1_u64 << 63) - 1, [0; 32]),
        order(10, [0; 32]),
        order(10, last_id),
        order(10, middle_id),
        order(10, [u8::MAX; 32]),
        order(0, [0; 32]),
    ];
    for (previous_index, previous) in positions.into_iter().enumerate() {
        assert!(
            !previous.is_after(previous),
            "equal keys are never continuation positions"
        );
        for (candidate_index, candidate) in positions.into_iter().enumerate() {
            assert_eq!(
                candidate.is_after(previous),
                candidate_index > previous_index
            );
        }
    }
    assert!(order(9, [0; 32]).is_after(order(10, [u8::MAX; 32])));
    assert!(!order(11, [u8::MAX; 32]).is_after(order(10, [0; 32])));
}

#[test]
fn cursor_roundtrip_preserves_canonical_version_and_full_width_key() {
    for (timestamp, timestamp_hex) in [
        (0, "0000000000000000"),
        (1, "0000000000000001"),
        ((1_u64 << 63) - 1, "7fffffffffffffff"),
        (1_u64 << 63, "8000000000000000"),
        (u64::MAX, "ffffffffffffffff"),
    ] {
        for event_id in [[0; 32], [0xab; 32], [u8::MAX; 32]] {
            let key = order(timestamp, event_id);
            let expected = canonical_cursor(timestamp_hex, event_id);
            let encoded = AvailabilityPageCursor::encode(base_fingerprint(), key);
            assert_eq!(encoded.as_str().as_bytes(), expected.as_bytes());
            assert_eq!(encoded.as_str().len(), 151);
            assert!(encoded.as_str().is_ascii());
            assert_eq!(encoded.after(), key);
            let parsed = AvailabilityPageCursor::parse(&expected, base_fingerprint())
                .expect("independently expected canonical cursor");
            assert_eq!(parsed.as_str(), expected);
            assert_eq!(parsed.after().created_at().as_u64(), timestamp);
            assert_eq!(parsed.after().version().event_id().as_bytes(), &event_id);
        }
    }
}

#[test]
fn cursor_rejects_malformed_noncanonical_and_unknown_versions() {
    let valid = canonical_cursor("ffffffffffffffff", [0xab; 32]);
    let mut malformed = vec![
        String::new(),
        valid[..150].to_owned(),
        format!("{valid}x"),
        valid.replacen("hcq1:", "hcq0:", 1),
        valid.replacen("hcq1:", "hcq2:", 1),
        valid.replacen("hcq1:", "HCQ1:", 1),
        valid.replacen("hcq1:", "hcq01:", 1),
        format!(" {valid}"),
        format!("{valid}\n"),
        valid.replacen(":ffffffffffffffff:", ":fffffffffffffff:", 1),
        valid.replacen(":ffffffffffffffff:", ":0xffffffffffffffff:", 1),
    ];
    for index in [4, 69, 86] {
        let mut wrong_separator = valid.clone();
        wrong_separator.replace_range(index..index + 1, "-");
        malformed.push(wrong_separator);
    }
    for index in [5, 70, 87] {
        for invalid in ["A", "g", " ", "\0"] {
            let mut changed = valid.clone();
            changed.replace_range(index..index + 1, invalid);
            assert_eq!(changed.len(), 151);
            malformed.push(changed);
        }
    }
    for (end, replacement) in [(7, "é"), (9, "🥕")] {
        let mut non_ascii = valid.clone();
        non_ascii.replace_range(5..end, replacement);
        assert_eq!(non_ascii.len(), 151);
        malformed.push(non_ascii);
    }
    for text in malformed {
        assert_eq!(
            AvailabilityPageCursor::parse(&text, base_fingerprint()).expect_err("malformed cursor"),
            AvailabilityQueryError::InvalidInput,
        );
    }
    let mut other_fingerprint = valid;
    other_fingerprint.replace_range(5..6, "0");
    assert_eq!(
        AvailabilityPageCursor::parse(&other_fingerprint, base_fingerprint())
            .expect_err("canonical different fingerprint"),
        AvailabilityQueryError::StaleQuery,
    );
}

#[test]
fn cursor_rejects_overlong_input_before_retention() {
    assert_eq!(AVAILABILITY_CURSOR_MAX_BYTES, 512_usize);
    for count in [511, 512] {
        for unit in ["x", "\0", "é", "🥕"] {
            let text = text_at_byte_count(unit, count);
            assert_eq!(text.len(), count);
            assert_eq!(
                AvailabilityPageCursor::parse(&text, base_fingerprint())
                    .expect_err("bounded malformed cursor"),
                AvailabilityQueryError::InvalidInput,
            );
        }
    }
    let valid = canonical_cursor("ffffffffffffffff", [0xab; 32]);
    let oversized_valid_prefix = format!("{valid}{}", "x".repeat(513 - valid.len()));
    let mut wrong_fingerprint = valid.clone();
    wrong_fingerprint.replace_range(5..6, "0");
    let oversized_wrong_fingerprint = format!(
        "{wrong_fingerprint}{}",
        "x".repeat(513 - wrong_fingerprint.len())
    );
    let oversized_unknown_version = format!("hcq9:{}", "g".repeat(508));
    for text in [
        "x".repeat(513),
        "\0".repeat(513),
        text_at_byte_count("é", 513),
        text_at_byte_count("🥕", 513),
        oversized_valid_prefix,
        oversized_wrong_fingerprint,
        oversized_unknown_version,
        "g".repeat(4_096),
    ] {
        assert!(text.len() > 512);
        assert_eq!(
            AvailabilityPageCursor::parse(&text, base_fingerprint())
                .expect_err("byte cap takes precedence over shape and fingerprint"),
            AvailabilityQueryError::InputTooLarge,
        );
    }
}

#[test]
fn fingerprint_binds_owner_context_and_store_identity() {
    let baseline = base_fingerprint();
    assert_eq!(baseline, base_fingerprint());
    let mut fingerprints = vec![baseline];
    fingerprints.push(AvailabilityQueryFingerprint::new(
        other_author(),
        &base_context(),
        3,
        &base_filters(),
        AvailabilityPageLimit::default(),
    ));
    for index in [0, 16, 31] {
        let mut changed_context = [1; 32];
        changed_context[index] = 3;
        let mut changed_store = [2; 32];
        changed_store[index] = 3;
        for changed in [
            context(changed_context, [2; 32], 5, 7),
            context([1; 32], changed_store, 5, 7),
        ] {
            fingerprints.push(AvailabilityQueryFingerprint::new(
                author(),
                &changed,
                3,
                &base_filters(),
                AvailabilityPageLimit::default(),
            ));
        }
    }
    assert_distinct(&fingerprints);
}

#[test]
fn fingerprint_binds_source_projection_and_session_generations() {
    let mut fingerprints = vec![base_fingerprint()];
    for value in [0, 1, 1_u64 << 63, u64::MAX] {
        for (changed_context, session) in [
            (context([1; 32], [2; 32], value, 7), 3),
            (context([1; 32], [2; 32], 5, value), 3),
            (base_context(), value),
        ] {
            fingerprints.push(AvailabilityQueryFingerprint::new(
                author(),
                &changed_context,
                session,
                &base_filters(),
                AvailabilityPageLimit::default(),
            ));
        }
    }
    for (source, projection, session) in [(1, 2, 3), (2, 1, 3), (1, 3, 2), (3, 2, 1)] {
        fingerprints.push(AvailabilityQueryFingerprint::new(
            author(),
            &context([1; 32], [2; 32], source, projection),
            session,
            &base_filters(),
            AvailabilityPageLimit::default(),
        ));
    }
    assert_distinct(&fingerprints);
}

#[test]
fn fingerprint_binds_exact_filters_and_row_limit() {
    assert_eq!(base_fingerprint().bytes(), &GOLDEN_FINGERPRINT_BYTES);
    let mut fingerprints = vec![base_fingerprint()];
    let exact_ascii = "a".repeat(512);
    let exact_unicode = "é".repeat(256);
    for search in [
        None,
        Some(""),
        Some("A"),
        Some("a "),
        Some(" a"),
        Some("é"),
        Some("e\u{301}"),
        Some("a\0"),
        Some(exact_ascii.as_str()),
        Some(exact_unicode.as_str()),
    ] {
        fingerprints.push(AvailabilityQueryFingerprint::new(
            author(),
            &base_context(),
            3,
            &filters(search, None, Some(FoodAvailabilityStatus::Active)),
            AvailabilityPageLimit::default(),
        ));
    }
    for publisher in [publisher(), other_publisher()] {
        fingerprints.push(AvailabilityQueryFingerprint::new(
            author(),
            &base_context(),
            3,
            &filters(
                Some("a"),
                Some(publisher),
                Some(FoodAvailabilityStatus::Active),
            ),
            AvailabilityPageLimit::default(),
        ));
    }
    for status in [None, Some(FoodAvailabilityStatus::Sold)] {
        fingerprints.push(AvailabilityQueryFingerprint::new(
            author(),
            &base_context(),
            3,
            &filters(Some("a"), None, status),
            AvailabilityPageLimit::default(),
        ));
    }
    for rows in 1..=100 {
        if rows != 50 {
            fingerprints.push(AvailabilityQueryFingerprint::new(
                author(),
                &base_context(),
                3,
                &base_filters(),
                AvailabilityPageLimit::new(rows).expect("valid distinct row limit"),
            ));
        }
    }
    assert_distinct(&fingerprints);
}

#[test]
fn fingerprint_option_and_length_framing_prevent_ambiguous_queries() {
    assert_eq!(base_fingerprint().bytes(), &GOLDEN_FINGERPRINT_BYTES);
    let mut fingerprints = Vec::new();
    for search in [
        None,
        Some(""),
        Some("a"),
        Some("a\0"),
        Some("a\0\0"),
        Some("active"),
        Some("sold"),
        Some("None"),
        Some("0:1:32"),
        Some(AUTHOR_HEX),
    ] {
        for publisher in [None, Some(publisher())] {
            for status in [
                None,
                Some(FoodAvailabilityStatus::Active),
                Some(FoodAvailabilityStatus::Sold),
            ] {
                fingerprints.push(AvailabilityQueryFingerprint::new(
                    author(),
                    &base_context(),
                    3,
                    &filters(search, publisher, status),
                    AvailabilityPageLimit::default(),
                ));
            }
        }
    }
    assert_distinct(&fingerprints);
    let with_search = AvailabilityQueryFingerprint::new(
        author(),
        &base_context(),
        3,
        &filters(Some(AUTHOR_HEX), None, None),
        AvailabilityPageLimit::default(),
    );
    let with_publisher = AvailabilityQueryFingerprint::new(
        author(),
        &base_context(),
        3,
        &filters(Some(""), Some(publisher()), None),
        AvailabilityPageLimit::default(),
    );
    assert_ne!(
        with_search, with_publisher,
        "search bytes cannot become a publisher field"
    );
}

#[test]
fn scoped_query_uses_local_owner_without_signing_capability() {
    for session in [0, 3, u64::MAX] {
        let local = scope(author(), base_context(), session);
        assert_eq!(local.owner(), author());
        assert_eq!(local.context().context_id(), &[1; 32]);
        assert_eq!(local.context().store_generation(), &[2; 32]);
        assert_eq!(local.context().source_revision(), 5);
        assert_eq!(local.context().projection_generation(), 7);
        assert_eq!(
            local.session_generation(),
            SessionGeneration::from_value(session)
        );
        let query = ScopedAvailabilityQuery::new(
            local,
            base_filters(),
            AvailabilityPageLimit::default(),
            None,
        )
        .expect("public owner and structural generations require no signing capability");
        assert_eq!(query.scope().owner(), author());
        assert_eq!(query.scope().session_generation().value(), session);
        assert_eq!(
            query.filters().search().expect("exact search").as_str(),
            "a"
        );
        assert_eq!(query.filters().publisher(), None);
        assert_eq!(
            query.filters().status(),
            Some(FoodAvailabilityStatus::Active)
        );
        assert_eq!(query.limit().rows(), 50);
        assert!(query.cursor().is_none());
        assert_eq!(
            query.validate_scope(&scope(author(), base_context(), session)),
            Ok(())
        );
    }
    assert_eq!(
        base_query().fingerprint().bytes(),
        &GOLDEN_FINGERPRINT_BYTES
    );
}

#[test]
fn scoped_query_refuses_owner_and_context_changes() {
    let query = base_query();
    assert_eq!(query.validate_scope(&base_scope()), Ok(()));
    assert_eq!(
        query.validate_scope(&scope(other_author(), base_context(), 3)),
        Err(AvailabilityQueryError::ScopeMismatch),
    );
    for index in [0, 16, 31] {
        let mut changed_context = [1; 32];
        changed_context[index] = 3;
        let changed = scope(author(), context(changed_context, [2; 32], 5, 7), 3);
        assert_eq!(
            query.validate_scope(&changed),
            Err(AvailabilityQueryError::ScopeMismatch)
        );
    }
    assert_eq!(query.validate_scope(&base_scope()), Ok(()));
    assert_eq!(query.scope().owner(), author());
    assert_eq!(query.scope().context().context_id(), &[1; 32]);
}

#[test]
fn scoped_query_refuses_stale_session_store_source_and_projection() {
    let query = base_query();
    for index in [0, 16, 31] {
        let mut changed_store = [2; 32];
        changed_store[index] = 3;
        let changed = scope(author(), context([1; 32], changed_store, 5, 7), 3);
        assert_eq!(
            query.validate_scope(&changed),
            Err(AvailabilityQueryError::StaleQuery)
        );
    }
    for value in [0, 1, 1_u64 << 63, u64::MAX] {
        for changed in [
            scope(author(), context([1; 32], [2; 32], value, 7), 3),
            scope(author(), context([1; 32], [2; 32], 5, value), 3),
            scope(author(), base_context(), value),
        ] {
            assert_eq!(
                query.validate_scope(&changed),
                Err(AvailabilityQueryError::StaleQuery)
            );
            assert_eq!(query.validate_scope(&base_scope()), Ok(()));
        }
    }
    let full_width = ScopedAvailabilityQuery::new(
        scope(
            author(),
            context([1; 32], [2; 32], u64::MAX, u64::MAX),
            u64::MAX,
        ),
        base_filters(),
        AvailabilityPageLimit::default(),
        None,
    )
    .expect("full-width structural generations");
    assert_eq!(
        full_width.validate_scope(&scope(
            author(),
            context([1; 32], [2; 32], u64::MAX, u64::MAX),
            u64::MAX
        )),
        Ok(()),
    );
    assert_eq!(
        full_width.validate_scope(&scope(
            author(),
            context([1; 32], [2; 32], u64::MAX, u64::MAX),
            (1_u64 << 63) - 1
        )),
        Err(AvailabilityQueryError::StaleQuery),
    );
    assert_eq!(query.scope().context().store_generation(), &[2; 32]);
    assert_eq!(query.scope().context().source_revision(), 5);
    assert_eq!(query.scope().context().projection_generation(), 7);
    assert_eq!(query.scope().session_generation().value(), 3);
}

#[test]
fn scoped_query_validates_cursor_against_complete_request() {
    let cursor_text = canonical_cursor("ffffffffffffffff", [0xab; 32]);
    let query = {
        let borrowed_input = cursor_text.clone();
        ScopedAvailabilityQuery::new(
            base_scope(),
            base_filters(),
            AvailabilityPageLimit::default(),
            Some(&borrowed_input),
        )
        .expect("valid borrowed cursor retained as a bounded owned value")
    };
    let retained = query.cursor().expect("explicit continuation");
    assert_eq!(retained.as_str(), cursor_text);
    assert_eq!(retained.after().created_at().as_u64(), u64::MAX);
    assert_eq!(
        retained.after().version().event_id().as_bytes(),
        &[0xab; 32]
    );
    assert_eq!(query.fingerprint().bytes(), &GOLDEN_FINGERPRINT_BYTES);
    assert_eq!(query.validate_scope(&base_scope()), Ok(()));

    let mut changed_requests = vec![(
        scope(other_author(), base_context(), 3),
        base_filters(),
        AvailabilityPageLimit::default(),
    )];
    for index in [0, 16, 31] {
        let mut changed_context = [1; 32];
        changed_context[index] = 3;
        let mut changed_store = [2; 32];
        changed_store[index] = 3;
        for changed_scope in [
            scope(author(), context(changed_context, [2; 32], 5, 7), 3),
            scope(author(), context([1; 32], changed_store, 5, 7), 3),
        ] {
            changed_requests.push((
                changed_scope,
                base_filters(),
                AvailabilityPageLimit::default(),
            ));
        }
    }
    for value in [0, 1_u64 << 63, u64::MAX] {
        for changed_scope in [
            scope(author(), context([1; 32], [2; 32], value, 7), 3),
            scope(author(), context([1; 32], [2; 32], 5, value), 3),
            scope(author(), base_context(), value),
        ] {
            changed_requests.push((
                changed_scope,
                base_filters(),
                AvailabilityPageLimit::default(),
            ));
        }
    }
    for search in [
        None,
        Some(""),
        Some("A"),
        Some("a "),
        Some("é"),
        Some("e\u{301}"),
        Some("a\0"),
    ] {
        changed_requests.push((
            base_scope(),
            filters(search, None, Some(FoodAvailabilityStatus::Active)),
            AvailabilityPageLimit::default(),
        ));
    }
    for changed_publisher in [publisher(), other_publisher()] {
        changed_requests.push((
            base_scope(),
            filters(
                Some("a"),
                Some(changed_publisher),
                Some(FoodAvailabilityStatus::Active),
            ),
            AvailabilityPageLimit::default(),
        ));
    }
    for status in [None, Some(FoodAvailabilityStatus::Sold)] {
        changed_requests.push((
            base_scope(),
            filters(Some("a"), None, status),
            AvailabilityPageLimit::default(),
        ));
    }
    for rows in [1, 49, 51, 100] {
        changed_requests.push((
            base_scope(),
            base_filters(),
            AvailabilityPageLimit::new(rows).expect("changed bounded row limit"),
        ));
    }
    assert!(
        changed_requests.len() >= 20,
        "complete independent scope/filter/limit mutations"
    );
    for (changed_scope, changed_filters, changed_limit) in changed_requests {
        assert_eq!(
            ScopedAvailabilityQuery::new(
                changed_scope,
                changed_filters,
                changed_limit,
                Some(&cursor_text)
            )
            .expect_err("cursor belongs to a different complete request"),
            AvailabilityQueryError::StaleQuery,
        );
    }
    for invalid in ["", "hcq9:invalid", "HCAV_UNTRUSTED_CURSOR_MARKER"] {
        assert_eq!(
            ScopedAvailabilityQuery::new(
                base_scope(),
                base_filters(),
                AvailabilityPageLimit::default(),
                Some(invalid)
            )
            .expect_err("malformed borrowed cursor"),
            AvailabilityQueryError::InvalidInput,
        );
    }
    let oversized = "\0".repeat(513);
    assert_eq!(
        ScopedAvailabilityQuery::new(
            base_scope(),
            base_filters(),
            AvailabilityPageLimit::default(),
            Some(&oversized)
        )
        .expect_err("oversized borrowed cursor"),
        AvailabilityQueryError::InputTooLarge,
    );
    assert_eq!(base_query().fingerprint(), query.fingerprint());
    assert!(base_query().cursor().is_none());
}

#[test]
fn page_contract_enforces_rows_and_preserves_projection() {
    // This type deliberately has neither Clone nor Debug: owned pages need neither.
    struct Row(u16);

    for (rows, limit) in [(0, 50), (50, 50), (100, 100), (1, 1)] {
        let items: Vec<_> = (0..rows).map(Row).collect();
        let pointer = items.as_ptr();
        let capacity = items.capacity();
        let page = AvailabilityPage::new(
            AvailabilityPageLimit::new(limit).expect("bounded row limit"),
            items,
            AvailabilityPageContinuation::End,
            u64::MAX,
        )
        .expect("owned row count within limit");
        assert_eq!(page.items().len(), usize::from(rows));
        assert_eq!(page.items().as_ptr(), pointer);
        assert_eq!(page.projection_generation(), u64::MAX);
        assert!(matches!(
            page.continuation(),
            AvailabilityPageContinuation::End
        ));
        assert_eq!(
            page.items().iter().map(|row| row.0).collect::<Vec<_>>(),
            (0..rows).collect::<Vec<_>>()
        );
        let returned = page.into_items();
        assert_eq!(returned.as_ptr(), pointer);
        assert_eq!(returned.capacity(), capacity);
        assert_eq!(returned.len(), usize::from(rows));
    }
    for limit in [1, 7, 50, 100] {
        let items: Vec<_> = (0..=limit).map(Row).collect();
        assert_eq!(
            AvailabilityPage::new(
                AvailabilityPageLimit::new(limit).expect("bounded row limit"),
                items,
                AvailabilityPageContinuation::End,
                7,
            )
            .expect_err("configured limit plus one, including 101 rows"),
            AvailabilityQueryError::Capacity,
        );
    }
    let query = base_query();
    assert_eq!(
        query
            .page(vec![0_u8; 50], None)
            .expect("default full page")
            .projection_generation(),
        7
    );
    assert_eq!(
        query
            .page(vec![0_u8; 51], None)
            .expect_err("default row overflow"),
        AvailabilityQueryError::Capacity
    );
    for generation in [0, u64::MAX] {
        let query = ScopedAvailabilityQuery::new(
            scope(author(), context([1; 32], [2; 32], 5, generation), 3),
            base_filters(),
            AvailabilityPageLimit::new(100).expect("maximum limit"),
            None,
        )
        .expect("exact projection query");
        assert_eq!(
            query
                .page(vec![0_u8; 100], None)
                .expect("maximum full page")
                .projection_generation(),
            generation
        );
        assert_eq!(
            query
                .page(vec![0_u8; 101], None)
                .expect_err("maximum overflow"),
            AvailabilityQueryError::Capacity
        );
    }
}

#[test]
fn local_page_end_and_continuation_remain_distinct() {
    let query = base_query();
    let next = order(u64::MAX, [0xab; 32]);
    let ended = query
        .page(Vec::<u8>::new(), None)
        .expect("explicit local end");
    assert!(ended.items().is_empty());
    assert!(matches!(
        ended.continuation(),
        AvailabilityPageContinuation::End
    ));
    assert_eq!(ended.projection_generation(), 7);
    let continued = query
        .page(vec![42_u8], Some(next))
        .expect("explicit local continuation");
    assert_eq!(continued.items(), &[42]);
    assert_eq!(continued.projection_generation(), 7);
    let AvailabilityPageContinuation::More(cursor) = continued.continuation() else {
        panic!("explicit next position must retain a continuation");
    };
    assert_eq!(
        cursor.as_str(),
        canonical_cursor("ffffffffffffffff", [0xab; 32])
    );
    assert_eq!(cursor.after(), next);
    assert_eq!(
        AvailabilityPageCursor::parse(cursor.as_str(), query.fingerprint())
            .expect("query-bound continuation")
            .after(),
        next
    );
    let empty_continued = query
        .page(Vec::<u8>::new(), Some(next))
        .expect("caller explicitly supplies continuation");
    assert!(empty_continued.items().is_empty());
    assert!(matches!(
        empty_continued.continuation(),
        AvailabilityPageContinuation::More(_)
    ));
    assert_eq!(
        empty_continued.projection_generation(),
        ended.projection_generation()
    );
    assert!(format!("{:?}", ended.continuation()).contains("End"));
    assert!(format!("{:?}", continued.continuation()).contains("More"));
}

#[test]
fn query_errors_and_debug_never_echo_untrusted_payloads() {
    const SEARCH_MARKER: &str = "HCAV_UNTRUSTED_SEARCH_PAYLOAD_é\0%_";
    const ITEM_MARKER: &str = "HCAV_UNTRUSTED_ITEM_PAYLOAD";
    const CURSOR_MARKER: &str = "HCAV_UNTRUSTED_CURSOR_PAYLOAD";
    let mut event_id = [0xde; 32];
    event_id[..4].copy_from_slice(&[0xde, 0xad, 0xc0, 0xde]);
    let search = AvailabilitySearchText::new(SEARCH_MARKER).expect("bounded marker search");
    let marked_filters = filters(
        Some(SEARCH_MARKER),
        Some(publisher()),
        Some(FoodAvailabilityStatus::Sold),
    );
    let marked_fingerprint = AvailabilityQueryFingerprint::new(
        author(),
        &base_context(),
        3,
        &marked_filters,
        AvailabilityPageLimit::default(),
    );
    let cursor = AvailabilityPageCursor::encode(marked_fingerprint, order(u64::MAX, event_id));
    let cursor_text = cursor.as_str().to_owned();
    let scope_debug = format!("{:?}", base_scope());
    let query = ScopedAvailabilityQuery::new(
        base_scope(),
        marked_filters,
        AvailabilityPageLimit::default(),
        Some(&cursor_text),
    )
    .expect("valid query carrying untrusted bounded search and public continuation");
    let search_debug = format!("{search:?}");
    assert!(search_debug.contains(&SEARCH_MARKER.len().to_string()));
    let cursor_debug = format!("{cursor:?}");
    assert!(cursor_debug.contains("151"));
    let mut debug_values = vec![
        search_debug,
        format!("{:?}", query.filters()),
        cursor_debug,
        scope_debug,
        format!("{query:?}"),
        format!("{:?}", AvailabilityPageContinuation::End),
    ];
    for next in [None, Some(order(u64::MAX, event_id))] {
        let page = query
            .page(vec![ITEM_MARKER.to_owned()], next)
            .expect("bounded marker page");
        debug_values.push(format!("{page:?}"));
        debug_values.push(format!("{:?}", page.continuation()));
    }
    let event_id_hex = hex(&event_id);
    let event_id_debug = format!("{event_id:?}");
    let fingerprint_hex = hex(marked_fingerprint.bytes());
    let fingerprint_debug = format!("{:?}", marked_fingerprint.bytes());
    let context_hex = hex(&[1; 32]);
    let store_hex = hex(&[2; 32]);
    let context_debug = format!("{:?}", [1_u8; 32]);
    let store_debug = format!("{:?}", [2_u8; 32]);
    for text in debug_values {
        for forbidden in [
            "HCAV_UNTRUSTED_SEARCH_PAYLOAD_",
            SEARCH_MARKER,
            ITEM_MARKER,
            CURSOR_MARKER,
            cursor_text.as_str(),
            event_id_hex.as_str(),
            event_id_debug.as_str(),
            fingerprint_hex.as_str(),
            fingerprint_debug.as_str(),
            context_hex.as_str(),
            store_hex.as_str(),
            context_debug.as_str(),
            store_debug.as_str(),
        ] {
            assert!(
                !text.contains(forbidden),
                "diagnostics must omit untrusted payload and opaque identities"
            );
        }
    }
    let invalid_cursor = ScopedAvailabilityQuery::new(
        base_scope(),
        base_filters(),
        AvailabilityPageLimit::default(),
        Some(CURSOR_MARKER),
    )
    .expect_err("untrusted malformed cursor");
    let invalid_search = AvailabilitySearchText::new(&SEARCH_MARKER.repeat(32))
        .expect_err("oversized untrusted search");
    assert_eq!(invalid_cursor, AvailabilityQueryError::InvalidInput);
    assert_eq!(invalid_search, AvailabilityQueryError::InputTooLarge);
    for (error, name) in [
        (AvailabilityQueryError::InvalidInput, "InvalidInput"),
        (AvailabilityQueryError::InputTooLarge, "InputTooLarge"),
        (AvailabilityQueryError::ScopeMismatch, "ScopeMismatch"),
        (AvailabilityQueryError::StaleQuery, "StaleQuery"),
        (AvailabilityQueryError::Capacity, "Capacity"),
    ] {
        let copied = error;
        assert_eq!(copied, error);
        let standard_error: &dyn Error = &error;
        assert!(standard_error.source().is_none());
        assert_eq!(format!("{error:?}"), name);
        let display = error.to_string();
        assert!(!display.is_empty());
        assert_eq!(display, copied.to_string());
        for forbidden in [
            "HCAV_UNTRUSTED_SEARCH_PAYLOAD_",
            SEARCH_MARKER,
            ITEM_MARKER,
            CURSOR_MARKER,
            cursor_text.as_str(),
        ] {
            assert!(!format!("{error:?} {error}").contains(forbidden));
        }
    }
    for error in [invalid_cursor, invalid_search] {
        assert!(!format!("{error:?} {error}").contains("HCAV_UNTRUSTED_SEARCH_PAYLOAD_"));
        assert!(!format!("{error:?} {error}").contains(SEARCH_MARKER));
        assert!(!format!("{error:?} {error}").contains(CURSOR_MARKER));
    }
}
