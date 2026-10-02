use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::food::availability::{
    FoodAvailabilityDetails, FoodAvailabilityError, FoodAvailabilityStatus, FoodContent,
    FoodIdentifier, FoodText, RADROOTS_FOOD_CONTENT_MAX_BYTES, RADROOTS_FOOD_IDENTIFIER_MAX_BYTES,
    RADROOTS_FOOD_TEXT_MAX_BYTES,
};
use radroots_event::wire::{
    DEFAULT_RAW_JSON_MAX_BYTES, EventWireError, Nip01EventWire, Nip01EventWireParts,
};
use radroots_event_codec::decode::food_availability::{
    RadrootsFoodAvailabilityProjectionOutcome, RadrootsInboundFoodAvailabilityProjection,
    project_verified_food_availability_event,
};
use radroots_event_codec::encode::food_availability::{
    RadrootsFoodAvailabilityEncodeError, authored_food_availability_build_tags,
    authored_food_availability_to_wire_parts,
};
use radroots_event_codec::verify::verify_nip01_event;

#[path = "fixtures/focused_availability_v1.rs"]
mod corpus;

const MAX_CORPUS_SOURCE_BYTES: usize = 16 * 1024;
const MAX_CORPUS_EVENT_BYTES: usize = 4 * 1024;
const CORPUS_SOURCE: &str = include_str!("fixtures/focused_availability_v1.rs");

struct SignedPublicEvent {
    json: String,
    author: String,
}

/// Only public JSON and the independently captured public author leave this scope.
fn sign_public_parts(parts: Nip01EventWireParts) -> SignedPublicEvent {
    let keys = Keys::generate();
    let author = keys.public_key().to_hex();
    let tags = parts
        .tags
        .into_iter()
        .map(|tag| Tag::parse(tag).expect("shared authored tag"))
        .collect::<Vec<_>>();
    let kind = u16::try_from(parts.kind).expect("shared kind fits NIP-01");
    let event = EventBuilder::new(Kind::from(kind), parts.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(corpus::CREATED_AT))
        .sign_with_keys(&keys)
        .expect("ephemeral signing succeeds");
    drop(keys);
    SignedPublicEvent {
        json: event.as_json(),
        author,
    }
}

fn project_public_event(signed: &SignedPublicEvent) -> RadrootsInboundFoodAvailabilityProjection {
    let wire = Nip01EventWire::parse_json(&signed.json)
        .expect("bounded canonical ID-verified public wire");
    assert!(wire.extra.is_empty(), "only standard public NIP-01 fields");
    assert_eq!(wire.kind, 30402);
    assert_eq!(wire.created_at, 1_800_000_100);
    assert_eq!(wire.pubkey, signed.author);
    let event_id = wire.id.clone();
    let verified = verify_nip01_event(wire.into_envelope().expect("ID-verified envelope"))
        .expect("shared ID and Schnorr signature verification");
    assert_eq!(verified.event().author().to_hex(), signed.author);
    assert_eq!(verified.event().id_hex(), event_id);
    match project_verified_food_availability_event(&verified).expect("shared focused projection") {
        RadrootsFoodAvailabilityProjectionOutcome::Focused(projection) => *projection,
        _ => panic!("public focused fixture must be admitted"),
    }
}

fn expected_tags(expected: &corpus::Expected, status: &str) -> Vec<Vec<String>> {
    let mut tags = vec![
        vec!["d".into(), expected.identifier.into()],
        vec!["title".into(), "Focused availability conformance".into()],
        vec!["summary".into(), "Public protocol fixture".into()],
        vec!["published_at".into(), "1800000000".into()],
        vec!["location".into(), "Protocol test location".into()],
        vec!["price".into(), expected.price.into(), "CAD".into()],
        vec!["radroots:price_unit".into(), expected.unit.into()],
    ];
    if let Some(quantity) = expected.quantity {
        tags.push(vec![
            "radroots:quantity".into(),
            quantity.into(),
            expected.unit.into(),
        ]);
    }
    tags.push(vec!["status".into(), status.into()]);
    tags
}

#[test]
fn public_corpus_strict_encoder_and_verified_projection_preserve_exact_semantics() {
    assert_eq!(corpus::INPUTS.len(), 10);
    assert_eq!(corpus::EXPECTED.len(), 10);
    let mut signed_cases = 0;
    for (input, expected) in corpus::INPUTS.iter().zip(&corpus::EXPECTED) {
        for (input_status, expected_status) in [
            (FoodAvailabilityStatus::Active, "active"),
            (FoodAvailabilityStatus::Sold, "sold"),
        ] {
            let details = FoodAvailabilityDetails::new(corpus::details_parts(input, input_status))
                .expect("strict public input");
            let parts = authored_food_availability_to_wire_parts(&details, corpus::CREATED_AT)
                .expect("shared strict authored codec");
            assert_eq!(parts.kind, 30402);
            assert_eq!(parts.tags, expected_tags(expected, expected_status));
            assert_eq!(
                parts.content,
                "Public conformance: café, \"exact\" decimals.\nText only."
            );
            let signed = sign_public_parts(parts);
            assert!(signed.json.len() <= MAX_CORPUS_EVENT_BYTES);
            let projection = project_public_event(&signed);

            assert_eq!(projection.identifier().as_str(), expected.identifier);
            assert_eq!(
                projection.title().as_str(),
                "Focused availability conformance"
            );
            assert_eq!(projection.summary().as_str(), "Public protocol fixture");
            assert_eq!(projection.published_at().as_u64(), 1_800_000_000);
            assert_eq!(projection.location().as_str(), "Protocol test location");
            assert_eq!(
                projection.content().as_str(),
                "Public conformance: café, \"exact\" decimals.\nText only."
            );
            assert_eq!(projection.price().amount(), expected.price);
            assert_eq!(projection.price().currency().as_str(), "CAD");
            assert_eq!(projection.price().unit().as_str(), expected.unit);
            assert_eq!(
                projection.quantity().map(|quantity| quantity.amount()),
                expected.quantity
            );
            assert_eq!(
                projection
                    .quantity()
                    .map(|quantity| quantity.unit().as_str()),
                expected.quantity.map(|_| expected.unit)
            );
            assert_eq!(projection.status().as_str(), expected_status);
            assert!(projection.images().is_empty());
            assert!(projection.diagnostics().is_empty());
            signed_cases += 1;
        }
    }
    assert_eq!(signed_cases, 20);
}

#[test]
fn retained_corpus_and_signed_artifacts_are_bounded_public_material() {
    assert!(CORPUS_SOURCE.len() <= MAX_CORPUS_SOURCE_BYTES);
    for forbidden in [
        "nsec1",
        "SecretKey",
        "secret_key",
        "private_key",
        "Keys::",
        "credential",
    ] {
        assert!(!CORPUS_SOURCE.contains(forbidden), "public fixture source");
    }
    for input in &corpus::INPUTS {
        let details = FoodAvailabilityDetails::new(corpus::details_parts(
            input,
            FoodAvailabilityStatus::Active,
        ))
        .expect("public input");
        let signed = sign_public_parts(
            authored_food_availability_to_wire_parts(&details, corpus::CREATED_AT)
                .expect("bounded public wire parts"),
        );
        assert!(signed.json.len() <= MAX_CORPUS_EVENT_BYTES);
        for forbidden in ["nsec1", "secret_key", "private_key", "credential"] {
            assert!(!signed.json.contains(forbidden), "public signed artifact");
        }
        let wire = Nip01EventWire::parse_json(&signed.json).expect("public artifact validates");
        assert!(wire.extra.is_empty());
        assert_eq!(wire.tags.len(), 8 + usize::from(input.quantity.is_some()));
        assert_eq!(wire.id.len(), 64);
        assert_eq!(wire.pubkey.len(), 64);
        assert_eq!(wire.sig.len(), 128);
        project_public_event(&signed);
    }
}

#[test]
fn shared_food_limits_measure_utf8_bytes_at_exact_boundaries() {
    let content = "é".repeat(RADROOTS_FOOD_CONTENT_MAX_BYTES / 2);
    assert_eq!(content.len(), RADROOTS_FOOD_CONTENT_MAX_BYTES);
    FoodContent::new(&content).expect("exact content byte limit");
    assert_eq!(
        FoodContent::new(format!("{content}x")).expect_err("content limit plus one"),
        FoodAvailabilityError::ContentTooLarge {
            max: RADROOTS_FOOD_CONTENT_MAX_BYTES,
            actual: RADROOTS_FOOD_CONTENT_MAX_BYTES + 1,
        }
    );

    let identifier = "é".repeat(RADROOTS_FOOD_IDENTIFIER_MAX_BYTES / 2);
    FoodIdentifier::parse(&identifier).expect("exact identifier byte limit");
    assert_eq!(
        FoodIdentifier::parse(format!("{identifier}x")).expect_err("identifier limit plus one"),
        FoodAvailabilityError::IdentifierTooLarge {
            max: RADROOTS_FOOD_IDENTIFIER_MAX_BYTES,
            actual: RADROOTS_FOOD_IDENTIFIER_MAX_BYTES + 1,
        }
    );

    let text = "é".repeat(RADROOTS_FOOD_TEXT_MAX_BYTES / 2);
    FoodText::new(&text).expect("exact text byte limit");
    assert_eq!(
        FoodText::new(format!("{text}x")).expect_err("text limit plus one"),
        FoodAvailabilityError::TextTooLarge {
            max: RADROOTS_FOOD_TEXT_MAX_BYTES,
            actual: RADROOTS_FOOD_TEXT_MAX_BYTES + 1,
        }
    );

    let mut parts = corpus::details_parts(&corpus::INPUTS[0], FoodAvailabilityStatus::Active);
    parts.content = FoodContent::new(&content).expect("exact content bound");
    parts.identifier = FoodIdentifier::parse(&identifier).expect("exact identifier bound");
    parts.title = FoodText::new(&text).expect("exact title bound");
    parts.summary = FoodText::new(&text).expect("exact summary bound");
    parts.location = FoodText::new(&text).expect("exact location bound");
    let details = FoodAvailabilityDetails::new(parts).expect("strict byte-boundary input");
    let signed = sign_public_parts(
        authored_food_availability_to_wire_parts(&details, corpus::CREATED_AT)
            .expect("shared encoder admits exact domain bounds"),
    );
    assert!(signed.json.len() <= DEFAULT_RAW_JSON_MAX_BYTES);
    let projection = project_public_event(&signed);
    assert_eq!(projection.content().as_str(), content);
    assert_eq!(projection.identifier().as_str(), identifier);
    assert_eq!(projection.title().as_str(), text);
    assert_eq!(projection.summary().as_str(), text);
    assert_eq!(projection.location().as_str(), text);
}

#[test]
fn shared_encoder_and_wire_parser_enforce_escaped_signed_byte_limit() {
    let mut parts = corpus::details_parts(&corpus::INPUTS[0], FoodAvailabilityStatus::Active);
    parts.content = FoodContent::new("é\n\\\0").expect("public escaped content");
    let details = FoodAvailabilityDetails::new(parts.clone()).expect("strict seed input");
    let seed = sign_public_parts(
        authored_food_availability_to_wire_parts(&details, corpus::CREATED_AT)
            .expect("bounded escaped seed"),
    );
    let remaining_bytes = DEFAULT_RAW_JSON_MAX_BYTES - seed.json.len();
    let mut content = "é\n\\\0".to_string();
    content.push_str(&"\"".repeat(remaining_bytes / 2));
    if !remaining_bytes.is_multiple_of(2) {
        content.push('x');
    }
    parts.content = FoodContent::new(&content).expect("decoded content remains bounded");
    let details = FoodAvailabilityDetails::new(parts.clone()).expect("exact signed-wire limit");
    let signed = sign_public_parts(
        authored_food_availability_to_wire_parts(&details, corpus::CREATED_AT)
            .expect("encoder admits exact escaped wire limit"),
    );
    assert_eq!(signed.json.len(), DEFAULT_RAW_JSON_MAX_BYTES);
    assert_eq!(project_public_event(&signed).content().as_str(), content);

    content.push('x');
    parts.content = FoodContent::new(&content).expect("overflow is in serialized wire only");
    let oversized = FoodAvailabilityDetails::new(parts).expect("domain input still valid");
    assert_eq!(
        authored_food_availability_to_wire_parts(&oversized, corpus::CREATED_AT)
            .expect_err("escaped signed-wire limit plus one"),
        RadrootsFoodAvailabilityEncodeError::EventWireTooLarge {
            max: DEFAULT_RAW_JSON_MAX_BYTES,
            actual: DEFAULT_RAW_JSON_MAX_BYTES + 1,
        }
    );
    let oversized_signed = sign_public_parts(Nip01EventWireParts {
        kind: 30402,
        content,
        tags: authored_food_availability_build_tags(&oversized, corpus::CREATED_AT)
            .expect("bounded public tags"),
    });
    assert_eq!(oversized_signed.json.len(), DEFAULT_RAW_JSON_MAX_BYTES + 1);
    assert_eq!(
        Nip01EventWire::parse_json(&oversized_signed.json)
            .expect_err("raw byte budget precedes parsing"),
        EventWireError::RawJsonTooLarge {
            max: DEFAULT_RAW_JSON_MAX_BYTES,
            actual: DEFAULT_RAW_JSON_MAX_BYTES + 1,
        }
    );
}
