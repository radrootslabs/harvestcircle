use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::envelope::EventEnvelope;
use radroots_event::listing::classified::{
    ClassifiedListingPartition, classify_classified_listing_tags,
};
use radroots_event::wire::Nip01EventWire;
use radroots_event_codec::admission::food_availability::{
    RadrootsAdmittedFoodAvailabilityEvent, RadrootsFoodAvailabilityAdmissionError,
    RadrootsFoodAvailabilityAdmissionOutcome, verify_and_admit_food_availability_event,
};
use radroots_event_codec::decode::food_availability::{
    RadrootsFoodAvailabilityImageDiagnostic, RadrootsFoodAvailabilityProjectionError,
};
use radroots_event_codec::verify::RadrootsNip01VerificationError;

#[path = "fixtures/availability_admission_v1.rs"]
mod corpus;

const MAX_RECIPE_BYTES: usize = 4 * 1024;
const MAX_RECIPE_TAGS: usize = 16;
const MAX_SIGNED_EVENT_BYTES: usize = 4 * 1024;
const MAX_RETAINED_CORPUS_BYTES: usize = 16 * 1024;
const RETAINED_CORPUS: &str = include_str!("fixtures/availability_admission_v1.rs");

struct SignedPublicEvent {
    json: String,
    author: String,
}

fn sign_public_recipe(recipe: corpus::Recipe) -> SignedPublicEvent {
    assert!(recipe.tags.len() <= MAX_RECIPE_TAGS);
    let recipe_bytes =
        recipe.content.len() + recipe.tags.iter().flatten().map(String::len).sum::<usize>();
    assert!(recipe_bytes <= MAX_RECIPE_BYTES);
    let keys = Keys::generate();
    let author = keys.public_key().to_hex();
    let tags = recipe
        .tags
        .into_iter()
        .map(|tag| Tag::parse(tag).expect("bounded raw incoming tag"))
        .collect::<Vec<_>>();
    let event = EventBuilder::new(Kind::from(30402_u16), recipe.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(corpus::CREATED_AT))
        .sign_with_keys(&keys)
        .expect("isolated incoming fixture signing");
    drop(keys);
    let json = event.as_json();
    assert!(json.len() <= MAX_SIGNED_EVENT_BYTES);
    SignedPublicEvent { json, author }
}

fn untrusted_envelope(json: &str) -> EventEnvelope {
    assert!(json.len() <= MAX_SIGNED_EVENT_BYTES);
    let wire = Nip01EventWire::parse_json_unverified(json)
        .expect("bounded structurally valid public NIP-01 input");
    assert!(wire.extra.is_empty());
    wire.into_unverified_envelope()
        .expect("untrusted public envelope")
}

fn admit_public_json(
    json: &str,
) -> Result<RadrootsFoodAvailabilityAdmissionOutcome, RadrootsFoodAvailabilityAdmissionError> {
    verify_and_admit_food_availability_event(untrusted_envelope(json))
}

fn admitted(signed: &SignedPublicEvent) -> RadrootsAdmittedFoodAvailabilityEvent {
    match admit_public_json(&signed.json).expect("shared tolerant admission") {
        RadrootsFoodAvailabilityAdmissionOutcome::Admitted(event) => {
            assert_eq!(event.event().author().to_hex(), signed.author);
            assert_eq!(event.event().kind_u32(), 30402);
            *event
        }
        _ => panic!("focused incoming recipe must be admitted"),
    }
}

enum ExpectedAdmission {
    Focused,
    Excluded(ClassifiedListingPartition),
    Error(RadrootsFoodAvailabilityProjectionError),
}

fn assert_partition(
    recipe: corpus::Recipe,
    partition: ClassifiedListingPartition,
    expected: ExpectedAdmission,
) {
    let signed = sign_public_recipe(recipe);
    let envelope = untrusted_envelope(&signed.json);
    assert_eq!(classify_classified_listing_tags(envelope.tags()), partition);
    let result = verify_and_admit_food_availability_event(envelope);
    match expected {
        ExpectedAdmission::Focused => match result.expect("focused admission") {
            RadrootsFoodAvailabilityAdmissionOutcome::Admitted(event) => {
                assert_eq!(event.event().author().to_hex(), signed.author);
                assert_eq!(event.projection().identifier().as_str(), "hcav-incoming-v1");
            }
            _ => panic!("focused partition must produce an admitted listing"),
        },
        ExpectedAdmission::Excluded(expected_partition) => {
            match result.expect("verified exclusion") {
                RadrootsFoodAvailabilityAdmissionOutcome::Excluded(candidate) => {
                    assert_eq!(candidate.partition(), expected_partition);
                    assert_eq!(candidate.event().author().to_hex(), signed.author);
                }
                _ => panic!("non-focused partition must be excluded"),
            }
        }
        ExpectedAdmission::Error(error) => {
            assert_eq!(
                result.expect_err("exact profile refusal"),
                RadrootsFoodAvailabilityAdmissionError::Projection(error)
            );
        }
    }
}

#[test]
fn raw_marker_partitions_have_exact_admission_outcomes_before_field_validation() {
    assert_eq!(
        corpus::FOCUSED_MARKERS,
        ["radroots:price_unit", "radroots:quantity"]
    );
    assert_eq!(
        corpus::OPERATIONAL_MARKERS,
        ["radroots:primary_bin", "radroots:bin", "radroots:price"]
    );
    let mut cases = 0;
    assert_partition(
        corpus::focused_recipe(),
        ClassifiedListingPartition::FocusedFoodAvailability,
        ExpectedAdmission::Focused,
    );
    cases += 1;
    for marker in corpus::FOCUSED_MARKERS {
        assert_partition(
            corpus::recipe_with_tags(&[&[marker]]),
            ClassifiedListingPartition::FocusedFoodAvailability,
            ExpectedAdmission::Error(RadrootsFoodAvailabilityProjectionError::TagInvalid),
        );
        cases += 1;
    }
    for recipe in [
        corpus::recipe_with_tags(&[]),
        corpus::edited_recipe(&[corpus::Edit::Remove("radroots:price_unit")]),
        corpus::recipe_with_tags(&[
            &["RADROOTS:PRICE_UNIT", "lb"],
            &["summary", "radroots:quantity"],
        ]),
    ] {
        assert_partition(
            recipe,
            ClassifiedListingPartition::GenericNip99,
            ExpectedAdmission::Excluded(ClassifiedListingPartition::GenericNip99),
        );
        cases += 1;
    }
    for marker in corpus::OPERATIONAL_MARKERS {
        // Raw marker presence selects exclusion before malformed shape or capability checks.
        assert_partition(
            corpus::recipe_with_tags(&[&[marker], &["delivery"]]),
            ClassifiedListingPartition::OperationalListing,
            ExpectedAdmission::Excluded(ClassifiedListingPartition::OperationalListing),
        );
        cases += 1;
    }
    for focused in corpus::FOCUSED_MARKERS {
        for operational in corpus::OPERATIONAL_MARKERS {
            for markers in [[focused, operational], [operational, focused]] {
                assert_partition(
                    corpus::recipe_with_tags(&[&[markers[0]], &[markers[1]], &["delivery"]]),
                    ClassifiedListingPartition::Ambiguous,
                    ExpectedAdmission::Error(
                        RadrootsFoodAvailabilityProjectionError::ProfileAmbiguous,
                    ),
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 21);
}

#[test]
fn tolerant_incoming_values_normalize_and_missing_quantity_stays_unknown() {
    let signed = sign_public_recipe(corpus::focused_recipe());
    let event = admitted(&signed);
    let projection = event.projection();
    assert_eq!(event.contract().id, "radroots.food.availability.v1");
    assert_eq!(
        projection.content().as_str(),
        "Public incoming availability conformance."
    );
    assert_eq!(projection.identifier().as_str(), "hcav-incoming-v1");
    assert_eq!(
        projection.title().as_str(),
        "Incoming availability conformance"
    );
    assert_eq!(
        projection.summary().as_str(),
        "Public incoming protocol fixture"
    );
    assert_eq!(projection.location().as_str(), "Protocol test location");
    assert_eq!(projection.published_at().as_u64(), 1_800_000_000);
    assert_eq!(projection.price().amount(), "3.5");
    assert_eq!(projection.price().currency().as_str(), "CAD");
    assert_eq!(projection.price().unit().as_str(), "lb");
    assert_eq!(projection.status().as_str(), "active");
    assert!(projection.quantity().is_none());
    assert!(projection.images().is_empty());
    assert!(projection.diagnostics().is_empty());
    // Admission preserves the authenticated input instead of rewriting normalized tags.
    assert_eq!(
        event.event().tags_as_vec(),
        corpus::raw_tags(corpus::BASE_TAGS)
    );

    let signed = sign_public_recipe(corpus::edited_recipe(&[
        corpus::Edit::Append(&["radroots:quantity", "0012.5000", "lb"]),
        corpus::Edit::Append(&["image", "https://images.example.test/incoming.webp"]),
        corpus::Edit::Append(&["t", "conformance"]),
    ]));
    let event = admitted(&signed);
    let projection = event.projection();
    let quantity = projection.quantity().expect("known incoming quantity");
    assert_eq!(quantity.amount(), "12.5");
    assert_eq!(quantity.unit().as_str(), "lb");
    assert_eq!(projection.images().len(), 1);
    let image = &projection.images()[0];
    assert_eq!(
        image.raw_tag(),
        ["image", "https://images.example.test/incoming.webp"]
    );
    assert_eq!(
        image.url(),
        Some("https://images.example.test/incoming.webp")
    );
    assert!(image.dimensions().is_none());
    assert!(!image.qualifies());
    let expected_diagnostics = [
        RadrootsFoodAvailabilityImageDiagnostic::ShapeInvalid,
        RadrootsFoodAvailabilityImageDiagnostic::DimensionsMissing,
    ];
    assert_eq!(image.diagnostics(), expected_diagnostics);
    assert_eq!(projection.diagnostics(), expected_diagnostics);
}

#[test]
fn malformed_focused_fields_return_exact_shared_admission_errors() {
    assert_eq!(corpus::MALFORMED_CASES.len(), 18);
    let mut cases = 0;
    for case in &corpus::MALFORMED_CASES {
        let signed = sign_public_recipe(corpus::edited_recipe(case.edits));
        let error = admit_public_json(&signed.json).expect_err(case.name);
        assert_eq!(error, case.expected, "{}", case.name);
        assert_eq!(error.code(), case.code, "{}", case.name);
        cases += 1;
    }
    assert_eq!(cases, 18);
}

fn replace_public_field(json: &str, field: &str, original: &str, replacement: &str) -> String {
    let original_field = format!("\"{field}\":\"{original}\"");
    assert_eq!(json.matches(&original_field).count(), 1);
    json.replacen(
        &original_field,
        &format!("\"{field}\":\"{replacement}\""),
        1,
    )
}

#[test]
fn invalid_id_and_signature_never_admit_a_listing_or_author_across_partitions() {
    let neighbor = sign_public_recipe(corpus::focused_recipe());
    assert_eq!(
        admitted(&neighbor).event().author().to_hex(),
        neighbor.author
    );
    let mut refused = 0;
    for recipe in [
        corpus::focused_recipe(),
        corpus::recipe_with_tags(&[]),
        corpus::recipe_with_tags(&[&["radroots:bin"]]),
        corpus::recipe_with_tags(&[&["radroots:quantity"], &["radroots:price"]]),
    ] {
        let signed = sign_public_recipe(recipe);
        let wire =
            Nip01EventWire::parse_json_unverified(&signed.json).expect("public fixture wire");
        let first = if wire.id.starts_with('0') { '1' } else { '0' };
        let wrong_id = format!("{first}{}", &wire.id[1..]);
        let invalid_id_json = replace_public_field(&signed.json, "id", &wire.id, &wrong_id);
        let invalid_signature_json =
            replace_public_field(&signed.json, "sig", &wire.sig, &"0".repeat(128));
        let both_invalid_json =
            replace_public_field(&invalid_signature_json, "id", &wire.id, &wrong_id);
        for json in [&invalid_id_json, &both_invalid_json] {
            let result = admit_public_json(json);
            assert!(
                result.is_err(),
                "no admitted or excluded event can expose an author"
            );
            assert_eq!(
                result.expect_err("ID verification precedes signature and profile admission"),
                RadrootsFoodAvailabilityAdmissionError::Nip01Verification(
                    RadrootsNip01VerificationError::IdMismatch {
                        expected: wire.id.clone(),
                        actual: wrong_id.clone(),
                    }
                )
            );
            refused += 1;
        }
        let result = admit_public_json(&invalid_signature_json);
        assert!(
            result.is_err(),
            "invalid signature provides no listing or author authority"
        );
        assert_eq!(
            result.expect_err("signature verification precedes profile admission"),
            RadrootsFoodAvailabilityAdmissionError::Nip01Verification(
                RadrootsNip01VerificationError::SignatureInvalid
            )
        );
        refused += 1;
    }
    assert_eq!(refused, 12);
    // Each failed candidate leaves an unrelated authentic observation independently admissible.
    assert_eq!(
        admitted(&neighbor).event().author().to_hex(),
        neighbor.author
    );
    assert!(admitted(&neighbor).projection().quantity().is_none());
}

#[test]
fn retained_incoming_recipes_and_signed_artifacts_are_bounded_public_material() {
    assert!(RETAINED_CORPUS.len() <= MAX_RETAINED_CORPUS_BYTES);
    for forbidden in [
        "nsec1",
        "SecretKey",
        "secret_key",
        "private_key",
        "Keys::",
        "credential",
    ] {
        assert!(
            !RETAINED_CORPUS.contains(forbidden),
            "public retained recipe source"
        );
    }
    let signed = sign_public_recipe(corpus::focused_recipe());
    for forbidden in ["nsec1", "secret_key", "private_key", "credential"] {
        assert!(!signed.json.contains(forbidden), "public signed artifact");
    }
    let wire = Nip01EventWire::parse_json(&signed.json).expect("public ID-verified artifact");
    assert!(wire.extra.is_empty());
    assert_eq!(wire.id.len(), 64);
    assert_eq!(wire.pubkey.len(), 64);
    assert_eq!(wire.sig.len(), 128);
    assert_eq!(wire.pubkey, signed.author);
    admitted(&signed);
}
