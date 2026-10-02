use harvestcircle_domain::{
    AvailabilityEventVersion, AvailabilityHeadState, AvailabilityHeadView,
    AvailabilityListingCoordinate, AvailabilityObservation, AvailabilityUnsupportedReason,
    AvailabilityVersionView, EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError,
    SafeErrorCode, UnixTimestamp,
};
use harvestcircle_nostr::parse_verified_kind0;
use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::envelope::event_head::{
    CurrentEventHead, EventHeadCandidate, EventHeadCandidateResult, EventHeadCoordinate,
    EventHeadDecision, event_head_candidate_for_nip01_event, select_event_head,
};
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::wire::{
    DEFAULT_EXTRA_MAX_FIELDS, DEFAULT_EXTRA_TOTAL_JSON_MAX_BYTES, DEFAULT_RAW_JSON_MAX_BYTES,
    DEFAULT_TAG_ELEMENT_MAX_BYTES, Nip01EventWire,
};
use radroots_event_codec::admission::deletion::{
    RadrootsAdmittedNip09DeletionRequestEvent, RadrootsNip09SuppressionOutcome,
    RadrootsNip09SuppressionReason, evaluate_nip09_suppression_from_borrowed_requests_v1,
    verify_and_admit_nip09_deletion_request_event,
};
use radroots_event_codec::admission::food_availability::{
    RadrootsFoodAvailabilityAdmissionOutcome, admit_verified_food_availability_event,
};
use radroots_event_codec::decode::food_availability::RadrootsFoodAvailabilityImageDiagnostic;
use radroots_event_codec::verify::{
    RadrootsNip01VerificationError, RadrootsSignatureVerifiedEvent, verify_nip01_event,
};

#[path = "fixtures/availability_admission_v1.rs"]
mod admission;
#[path = "fixtures/availability_lifecycle_v1.rs"]
mod lifecycle;

const MAX_RECIPE_BYTES: usize = 8 * 1024;
const MAX_RECIPE_TAGS: usize = 32;
const MAX_RECIPE_ELEMENTS: usize = 128;
const MAX_SIGNED_EVENT_BYTES: usize = 16 * 1024;
const MAX_LIFECYCLE_EVENT_BYTES: usize = 4 * 1024;
const MAX_LIFECYCLE_CORPUS_BYTES: usize = 64 * 1024;
const MAX_RETAINED_RECIPE_SOURCE_BYTES: usize = 32 * 1024;
const MAX_SOURCE_BYTES: usize = 2048;
const MAX_DELETION_REQUESTS: usize = 4096;
const OBSERVED_AT: i64 = 1_800_000_500;
const SOURCE: &str = "wss://relay.example.test/observations?scope=public";

struct PublicFixture {
    json: String,
    author: String,
}

fn assert_recipe_source_bounds() {
    let sources = [
        include_str!("fixtures/availability_admission_v1.rs"),
        include_str!("fixtures/availability_lifecycle_v1.rs"),
    ];
    assert!(
        sources.iter().map(|source| source.len()).sum::<usize>()
            <= MAX_RETAINED_RECIPE_SOURCE_BYTES
    );
    for source in sources {
        for forbidden in ["nsec1", "secret_key", "private_key", "credential"] {
            assert!(
                !source.contains(forbidden),
                "retained recipes contain only public data"
            );
        }
    }
}

fn sign_bounded_event(
    keys: &Keys,
    kind: u16,
    created_at: u64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> Event {
    assert_recipe_source_bounds();
    assert!(tags.len() <= MAX_RECIPE_TAGS);
    assert!(tags.iter().map(Vec::len).sum::<usize>() <= MAX_RECIPE_ELEMENTS);
    assert!(
        content.len() + tags.iter().flatten().map(String::len).sum::<usize>() <= MAX_RECIPE_BYTES
    );
    assert!(
        tags.iter()
            .flatten()
            .all(|value| value.len() <= DEFAULT_TAG_ELEMENT_MAX_BYTES)
    );
    EventBuilder::new(Kind::from(kind), content)
        .tags(
            tags.into_iter()
                .map(|tag| Tag::parse(tag).expect("bounded public raw tag")),
        )
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("isolated public fixture signing")
}

/// The ephemeral signer is discarded before retained public JSON is returned.
fn public_fixture_at(recipe: admission::Recipe, kind: u16, created_at: u64) -> PublicFixture {
    let keys = Keys::generate();
    let author = keys.public_key().to_hex();
    let event = sign_bounded_event(&keys, kind, created_at, &recipe.content, recipe.tags);
    drop(keys);
    let json = event.as_json();
    assert!(json.len() <= MAX_SIGNED_EVENT_BYTES);
    PublicFixture { json, author }
}

fn public_fixture(recipe: admission::Recipe) -> PublicFixture {
    public_fixture_at(recipe, 30402, admission::CREATED_AT)
}

fn verified(json: &str) -> RadrootsSignatureVerifiedEvent {
    assert!(json.len() <= MAX_SIGNED_EVENT_BYTES);
    let wire = Nip01EventWire::parse_json_unverified(json).expect("default-bounded public wire");
    verify_nip01_event(
        wire.into_unverified_envelope()
            .expect("structurally valid envelope"),
    )
    .expect("real shared ID and signature verification")
}

fn observation() -> AvailabilityObservation {
    AvailabilityObservation::parse(
        SOURCE,
        UnixTimestamp::from_seconds(OBSERVED_AT).expect("local observation time"),
    )
    .expect("bounded pure source reference")
}

fn version_view(json: &str) -> AvailabilityVersionView {
    AvailabilityVersionView::from_verified(verified(json), json, observation())
        .expect("verified version view")
}

fn candidate(event: &RadrootsSignatureVerifiedEvent) -> EventHeadCandidate {
    match event_head_candidate_for_nip01_event(event.event()) {
        EventHeadCandidateResult::Candidate(candidate) => candidate,
        _ => panic!("verified addressable recipe must retain a raw head"),
    }
}

fn selected(events: &[&RadrootsSignatureVerifiedEvent]) -> CurrentEventHead {
    assert!(!events.is_empty() && events.len() <= 3);
    let mut current = None;
    for event in events {
        match select_event_head(candidate(event), current.as_ref()) {
            EventHeadDecision::Applied(next) => current = Some(next),
            EventHeadDecision::SkippedDuplicate
            | EventHeadDecision::SkippedOlder
            | EventHeadDecision::SkippedSameTimestampHigherEventId => {}
            EventHeadDecision::CoordinateMismatch => {
                panic!("one raw coordinate per ordering fixture")
            }
        }
    }
    current.expect("shared selected head")
}

struct LifecycleCorpus {
    author: String,
    other_author: String,
    events: Vec<(&'static str, String)>,
}

/// Both temporary signers are discarded before the bounded public corpus escapes.
fn lifecycle_corpus() -> LifecycleCorpus {
    let owner = Keys::generate();
    let other = Keys::generate();
    let author = owner.public_key().to_hex();
    let other_author = other.public_key().to_hex();
    assert_ne!(author, other_author);
    let mut signed = Vec::<(&'static str, Event)>::new();
    for version in &lifecycle::VERSIONS {
        signed.push((
            version.name,
            sign_bounded_event(
                if version.other_author { &other } else { &owner },
                30402,
                version.created_at,
                version.content,
                lifecycle::listing_tags(version),
            ),
        ));
    }
    let original = &lifecycle::VERSIONS[0];
    signed.push((
        "other_kind",
        sign_bounded_event(
            &owner,
            lifecycle::OTHER_ADDRESSABLE_KIND,
            original.created_at,
            original.content,
            lifecycle::listing_tags(original),
        ),
    ));
    let address = format!("30402:{author}:{}", lifecycle::IDENTIFIER);
    for deletion in &lifecycle::DELETIONS {
        let mut tags = Vec::new();
        if let lifecycle::Target::Event(name) | lifecycle::Target::EventAndAddress(name) =
            deletion.target
        {
            let (_, target) = signed
                .iter()
                .find(|(target_name, _)| *target_name == name)
                .expect("reused lifecycle target exists");
            tags.push(vec!["e".into(), target.id.to_hex()]);
        }
        if matches!(
            deletion.target,
            lifecycle::Target::Address | lifecycle::Target::EventAndAddress(_)
        ) {
            tags.push(vec!["a".into(), address.clone()]);
        }
        tags.push(vec!["k".into(), "30402".into()]);
        signed.push((
            deletion.name,
            sign_bounded_event(
                if deletion.other_author {
                    &other
                } else {
                    &owner
                },
                5,
                deletion.created_at,
                "Public deletion conformance.",
                tags,
            ),
        ));
    }
    drop(owner);
    drop(other);
    let events: Vec<_> = signed
        .into_iter()
        .map(|(name, event)| {
            let json = event.as_json();
            assert!(json.len() <= MAX_LIFECYCLE_EVENT_BYTES);
            (name, json)
        })
        .collect();
    assert_eq!(events.len(), 18);
    assert!(events.iter().map(|(_, json)| json.len()).sum::<usize>() <= MAX_LIFECYCLE_CORPUS_BYTES);
    LifecycleCorpus {
        author,
        other_author,
        events,
    }
}

fn lifecycle_json<'a>(corpus: &'a LifecycleCorpus, name: &str) -> &'a str {
    &corpus
        .events
        .iter()
        .find(|(event_name, _)| *event_name == name)
        .expect("named unchanged lifecycle recipe")
        .1
}

fn deletion(corpus: &LifecycleCorpus, name: &str) -> RadrootsAdmittedNip09DeletionRequestEvent {
    let wire = Nip01EventWire::parse_json_unverified(lifecycle_json(corpus, name))
        .expect("default-bounded deletion wire");
    verify_and_admit_nip09_deletion_request_event(
        wire.into_unverified_envelope().expect("request envelope"),
    )
    .expect("real shared signature verification and deletion admission")
}

fn replace_once(json: &str, original: &str, replacement: &str) -> String {
    assert_eq!(
        json.matches(original).count(),
        1,
        "unique public fixture token"
    );
    let replaced = json.replacen(original, replacement, 1);
    assert!(replaced.len() <= DEFAULT_RAW_JSON_MAX_BYTES + 1);
    replaced
}

fn replace_string_field(json: &str, field: &str, original: &str, replacement: &str) -> String {
    replace_once(
        json,
        &format!("\"{field}\":\"{original}\""),
        &format!("\"{field}\":\"{replacement}\""),
    )
}

fn add_extras(json: &str, extras: &str) -> String {
    assert!(json.ends_with('}'));
    assert!(json.len() + extras.len() <= DEFAULT_RAW_JSON_MAX_BYTES);
    format!("{},{}{}", &json[..json.len() - 1], extras, '}')
}

fn text_at_bytes(unit: &str, size: usize) -> String {
    unit.repeat(size / unit.len()) + &"x".repeat(size % unit.len())
}

fn assert_safe(error: SafeError, rejected_input: &str) {
    assert_eq!(error.code(), SafeErrorCode::InvalidProfileMetadata);
    for rendered in [error.to_string(), format!("{error:?}")] {
        assert!(rendered.len() <= 256);
        assert!(!rendered.contains(rejected_input));
    }
}

#[test]
fn admitted_view_preserves_exact_version_coordinate_and_signed_times() {
    let fixture = public_fixture(admission::focused_recipe());
    let event = verified(&fixture.json);
    let admitted =
        match admit_verified_food_availability_event(event.clone()).expect("real admission") {
            RadrootsFoodAvailabilityAdmissionOutcome::Admitted(admitted) => admitted,
            _ => panic!("focused recipe"),
        };
    let view = version_view(&fixture.json);
    assert_eq!(view.version().event_id(), *event.event().id());
    assert_eq!(view.publisher().public_key().to_hex(), fixture.author);
    assert_eq!(view.created_at(), event.event().created_at());
    assert_eq!(view.created_at().as_u64(), admission::CREATED_AT);
    assert_eq!(view.observation().observed_at().as_seconds(), OBSERVED_AT);
    assert_eq!(view.observation().source().as_str(), SOURCE);
    assert_eq!(view.original_json(), fixture.json);
    assert_eq!(view.raw_coordinate(), &candidate(&event).coordinate);
    assert_eq!(
        view.listing_coordinate()
            .expect("strict app reference")
            .identifier(),
        "hcav-incoming-v1"
    );
    assert_eq!(view.focused(), Some(admitted.projection()));
    assert_eq!(view.unsupported_reason(), None);
    assert_eq!(view.profile_metadata(), None);

    let wide = public_fixture_at(admission::focused_recipe(), 30402, i64::MAX as u64 + 1);
    let wide_view = version_view(&wide.json);
    assert_eq!(wide_view.created_at().as_u64(), i64::MAX as u64 + 1);
    assert_eq!(
        wide_view.observation().observed_at().as_seconds(),
        OBSERVED_AT
    );
}

#[test]
fn admitted_view_preserves_unknown_quantity_and_exact_normalized_price() {
    let fixture = public_fixture(admission::focused_recipe());
    let view = version_view(&fixture.json);
    let food = view.focused().expect("focused tolerant projection");
    assert_eq!(food.quantity(), None);
    assert_eq!(food.price().amount(), "3.5");
    assert_eq!(food.price().currency().as_str(), "CAD");
    assert_eq!(food.price().unit().as_str(), "lb");
    assert_eq!(food.status().as_str(), "active");
    assert_eq!(food.published_at().as_u64(), 1_800_000_000);
    assert_eq!(food.content().as_str(), admission::CONTENT);
    assert_eq!(food.title().as_str(), "Incoming availability conformance");
    assert_eq!(food.summary().as_str(), "Public incoming protocol fixture");
    assert_eq!(food.location().as_str(), "Protocol test location");
}

#[test]
fn admitted_view_preserves_known_quantity_without_unit_conversion() {
    for (amount, unit, expected) in [("0012.5000", "lb", "12.5"), ("0.1250", "kg", "0.125")] {
        let mut recipe = admission::focused_recipe();
        recipe
            .tags
            .iter_mut()
            .find(|tag| tag[0] == "radroots:price_unit")
            .expect("price unit")[1] = unit.into();
        recipe
            .tags
            .push(vec!["radroots:quantity".into(), amount.into(), unit.into()]);
        let fixture = public_fixture(recipe);
        let view = version_view(&fixture.json);
        let food = view.focused().expect("focused quantity");
        let quantity = food.quantity().expect("known quantity");
        assert_eq!(quantity.amount(), expected);
        assert_eq!(quantity.unit().as_str(), unit);
        assert_eq!(food.price().unit().as_str(), unit);
        assert_eq!(food.price().amount(), "3.5");
    }
}

#[test]
fn admitted_view_retains_optional_image_diagnostics_and_original_wire() {
    let fixture = public_fixture(admission::edited_recipe(&[
        admission::Edit::Append(&["image", "https://images.example.test/incoming.webp"]),
        admission::Edit::Append(&["t", "opaque-public-discovery-tag"]),
    ]));
    let exact = format!(
        " \n{}\t",
        add_extras(
            &fixture.json,
            "\"untrusted_extra\":{\"stock\":999,\"claim\":\"not-signed\"}"
        )
    );
    let view =
        AvailabilityVersionView::from_verified(verified(&fixture.json), &exact, observation())
            .expect("exact extras and formatting");
    assert_eq!(view.original_json().as_bytes(), exact.as_bytes());
    let food = view
        .focused()
        .expect("optional image cannot discard listing");
    assert_eq!(food.quantity(), None);
    assert_eq!(food.images().len(), 1);
    let image = &food.images()[0];
    assert_eq!(
        image.url(),
        Some("https://images.example.test/incoming.webp")
    );
    assert_eq!(image.dimensions(), None);
    assert!(!image.qualifies());
    assert_eq!(
        image.raw_tag(),
        &["image", "https://images.example.test/incoming.webp"]
    );
    assert!(
        image
            .diagnostics()
            .contains(&RadrootsFoodAvailabilityImageDiagnostic::ShapeInvalid)
    );
    assert!(
        image
            .diagnostics()
            .contains(&RadrootsFoodAvailabilityImageDiagnostic::DimensionsMissing)
    );
    let retained =
        Nip01EventWire::parse_json_unverified(view.original_json()).expect("retained wire");
    assert!(
        retained
            .tags
            .iter()
            .any(|tag| tag == &["t", "opaque-public-discovery-tag"])
    );
    assert_eq!(retained.extra.len(), 1);
    assert_eq!(
        retained
            .into_unverified_envelope()
            .expect("extras have no envelope authority"),
        *verified(&fixture.json).event()
    );
}

#[test]
fn profile_failure_does_not_remove_an_admitted_listing_view() {
    let keys = Keys::generate();
    let author = keys.public_key().to_hex();
    let recipe = admission::focused_recipe();
    let listing = sign_bounded_event(
        &keys,
        30402,
        admission::CREATED_AT,
        &recipe.content,
        recipe.tags,
    );
    let malformed_profile =
        sign_bounded_event(&keys, 0, admission::CREATED_AT, "not profile JSON", vec![]);
    let overlong_profile = sign_bounded_event(
        &keys,
        0,
        admission::CREATED_AT,
        &format!("{{\"name\":\"{}\"}}", "p".repeat(129)),
        vec![],
    );
    drop(keys);
    let listing_json = listing.as_json();
    let view = version_view(&listing_json);
    let publisher = PublicKey::from_hex(&author).expect("public profile author");
    for event in [malformed_profile, overlong_profile] {
        let json = event.as_json();
        assert!(json.len() <= MAX_SIGNED_EVENT_BYTES);
        assert!(parse_verified_kind0(&json, publisher).is_err());
        assert_eq!(view.profile_metadata(), None);
        assert_eq!(
            view.focused()
                .expect("listing survives optional metadata failure")
                .status()
                .as_str(),
            "active"
        );
        assert_eq!(view.original_json(), listing_json);
    }
    assert_eq!(
        view.with_profile(None)
            .expect("absence remains absence")
            .profile_metadata(),
        None
    );
}

#[test]
fn generic_and_operational_heads_never_become_focused_food() {
    let generic = public_fixture(admission::recipe_with_tags(&[]));
    let view = version_view(&generic.json);
    assert_eq!(view.focused(), None);
    assert_eq!(
        view.unsupported_reason(),
        Some(&AvailabilityUnsupportedReason::Excluded(
            ClassifiedListingPartition::GenericNip99
        ))
    );
    for marker in admission::OPERATIONAL_MARKERS {
        let fixture = public_fixture(admission::recipe_with_tags(&[&[marker]]));
        let view = version_view(&fixture.json);
        assert_eq!(view.focused(), None);
        assert_eq!(
            view.unsupported_reason(),
            Some(&AvailabilityUnsupportedReason::Excluded(
                ClassifiedListingPartition::OperationalListing
            ))
        );
        let head =
            AvailabilityHeadView::from_selected(selected(&[&verified(&fixture.json)]), view, &[])
                .expect("raw operational head retained");
        assert_eq!(head.state(), AvailabilityHeadState::Unsupported);
        assert_eq!(head.focused(), None);
    }
    assert_eq!(
        admission::FOCUSED_MARKERS,
        ["radroots:price_unit", "radroots:quantity"]
    );
}

#[test]
fn ambiguous_and_malformed_verified_heads_remain_unsupported() {
    for marker in admission::OPERATIONAL_MARKERS {
        let mut recipe = admission::focused_recipe();
        recipe.tags.push(vec![marker.into()]);
        let fixture = public_fixture(recipe);
        let view = version_view(&fixture.json);
        assert_eq!(view.focused(), None);
        assert_eq!(
            view.unsupported_reason(),
            Some(&AvailabilityUnsupportedReason::ProjectionRejected(
                "food_profile_ambiguous"
            ))
        );
    }
    for case in &admission::MALFORMED_CASES {
        let fixture = public_fixture(admission::edited_recipe(case.edits));
        let event = verified(&fixture.json);
        assert_eq!(
            admit_verified_food_availability_event(event.clone()).expect_err(case.name),
            case.expected
        );
        let view = version_view(&fixture.json);
        assert_eq!(view.version().event_id(), *event.event().id());
        assert_eq!(view.raw_coordinate(), &candidate(&event).coordinate);
        assert_eq!(view.focused(), None, "{}", case.name);
        assert_eq!(
            view.unsupported_reason(),
            Some(&AvailabilityUnsupportedReason::ProjectionRejected(
                case.code
            )),
            "{}",
            case.name
        );
    }
}

#[test]
fn unsupported_head_preserves_empty_missing_and_long_identifiers() {
    for identifier in [
        None,
        Some(String::new()),
        Some("d".repeat(4025)),
        Some("d".repeat(4026)),
        Some("é".repeat(2048)),
        Some(" opaque:\0:é ".into()),
    ] {
        let mut recipe = admission::focused_recipe();
        recipe.tags.retain(|tag| tag[0] != "d");
        if let Some(identifier) = &identifier {
            recipe.tags.insert(0, vec!["d".into(), identifier.clone()]);
        }
        let fixture = public_fixture(recipe);
        let view = version_view(&fixture.json);
        assert_eq!(view.focused(), None);
        assert!(matches!(
            view.unsupported_reason(),
            Some(AvailabilityUnsupportedReason::ProjectionRejected(_))
        ));
        match view.raw_coordinate() {
            EventHeadCoordinate::Addressable {
                kind,
                pubkey,
                d_tag,
            } => {
                assert_eq!(*kind, 30402);
                assert_eq!(pubkey.to_hex(), fixture.author);
                assert_eq!(d_tag, identifier.as_deref().unwrap_or(""));
            }
            _ => panic!("raw kind-30402 head"),
        }
        let fits_app = identifier
            .as_ref()
            .is_some_and(|value| !value.is_empty() && value.len() <= 4025);
        assert_eq!(view.listing_coordinate().is_some(), fits_app);
        if let Some(coordinate) = view.listing_coordinate() {
            assert_eq!(
                coordinate.identifier(),
                identifier.as_deref().expect("nonempty app identifier")
            );
        }
        let head =
            AvailabilityHeadView::from_selected(selected(&[&verified(&fixture.json)]), view, &[])
                .expect("unsupported raw head survives");
        assert_eq!(head.state(), AvailabilityHeadState::Unsupported);
        assert!(head.version().is_some());
        assert_eq!(head.focused(), None);
    }
}

#[test]
fn newer_unsupported_head_blocks_older_compatible_view() {
    let corpus = lifecycle_corpus();
    let old = verified(lifecycle_json(&corpus, "old_active"));
    assert_eq!(
        version_view(lifecycle_json(&corpus, "old_active"))
            .focused()
            .expect("older food")
            .status()
            .as_str(),
        "active"
    );
    for name in ["generic_head", "malformed_head"] {
        let newer = verified(lifecycle_json(&corpus, name));
        for order in [[&old, &newer, &old], [&newer, &old, &newer]] {
            let current = selected(&order);
            assert_eq!(current.event_id, *newer.event().id());
            let head = AvailabilityHeadView::from_selected(
                current.clone(),
                version_view(lifecycle_json(&corpus, name)),
                &[],
            )
            .expect("selected unsupported version");
            assert_eq!(head.state(), AvailabilityHeadState::Unsupported);
            assert_eq!(
                head.version()
                    .expect("retained winner")
                    .version()
                    .event_id(),
                *newer.event().id()
            );
            assert_eq!(head.focused(), None);
            assert!(
                AvailabilityHeadView::from_selected(
                    current,
                    version_view(lifecycle_json(&corpus, "old_active")),
                    &[]
                )
                .is_err()
            );
        }
    }
}

#[test]
fn missing_unsupported_and_deleted_states_remain_distinct() {
    let corpus = lifecycle_corpus();
    let focused = version_view(lifecycle_json(&corpus, "newer_sold"));
    let missing = AvailabilityHeadView::missing(
        focused
            .listing_coordinate()
            .expect("app coordinate")
            .clone(),
    );
    assert_eq!(missing.state(), AvailabilityHeadState::Missing);
    assert_eq!(missing.version(), None);
    assert_eq!(missing.suppression(), None);
    assert_eq!(missing.focused(), None);
    let event = verified(lifecycle_json(&corpus, "newer_sold"));
    let visible = AvailabilityHeadView::from_selected(selected(&[&event]), focused.clone(), &[])
        .expect("visible selected version");
    assert_eq!(visible.state(), AvailabilityHeadState::Focused);
    assert_eq!(
        visible.focused().expect("visible food").status().as_str(),
        "sold"
    );
    let deleted = AvailabilityHeadView::from_selected(
        selected(&[&event]),
        focused,
        &[deletion(&corpus, "exact_newer")],
    )
    .expect("shared suppression");
    assert_eq!(deleted.state(), AvailabilityHeadState::Deleted);
    assert_eq!(deleted.focused(), None);
    assert_eq!(
        deleted
            .version()
            .expect("historical food remains")
            .focused()
            .expect("historical projection")
            .status()
            .as_str(),
        "sold"
    );
    let generic_json = lifecycle_json(&corpus, "generic_head");
    let unsupported = AvailabilityHeadView::from_selected(
        selected(&[&verified(generic_json)]),
        version_view(generic_json),
        &[],
    )
    .expect("unsupported winner");
    assert_eq!(unsupported.state(), AvailabilityHeadState::Unsupported);
    assert_eq!(unsupported.focused(), None);
}

#[test]
fn selected_head_mismatch_rejects_an_older_version() {
    let corpus = lifecycle_corpus();
    let old_json = lifecycle_json(&corpus, "old_active");
    let new = verified(lifecycle_json(&corpus, "newer_sold"));
    assert!(
        AvailabilityHeadView::from_selected(selected(&[&new]), version_view(old_json), &[])
            .is_err()
    );
    let actual = selected(&[&verified(old_json)]);
    let mut wrong_id = actual.clone();
    wrong_id.event_id = *new.event().id();
    let mut wrong_time = actual.clone();
    wrong_time.created_at += 1;
    let mut mismatches = vec![wrong_id, wrong_time];
    for name in ["other_identifier", "other_author", "other_kind"] {
        let mut wrong_coordinate = actual.clone();
        wrong_coordinate.coordinate =
            candidate(&verified(lifecycle_json(&corpus, name))).coordinate;
        assert_eq!(wrong_coordinate.event_id, actual.event_id);
        assert_eq!(wrong_coordinate.created_at, actual.created_at);
        assert_ne!(wrong_coordinate.coordinate, actual.coordinate);
        mismatches.push(wrong_coordinate);
    }
    assert_eq!(mismatches.len(), 5);
    for wrong in mismatches {
        let error = AvailabilityHeadView::from_selected(wrong, version_view(old_json), &[])
            .expect_err("all selected head fields bind the exact version");
        assert_safe(error, "Public lifecycle conformance.");
    }
    assert!(AvailabilityHeadView::from_selected(actual, version_view(old_json), &[]).is_ok());
}

#[test]
fn deleted_view_retains_shared_author_and_cutoff_evidence() {
    let corpus = lifecycle_corpus();
    assert_ne!(corpus.author, corpus.other_author);
    let target_json = lifecycle_json(&corpus, "at_cutoff");
    let target = verified(target_json);
    let exact = deletion(&corpus, "combined_cutoff");
    let address = deletion(&corpus, "address_cutoff");
    let forged = deletion(&corpus, "forged_address");
    assert_eq!(forged.event().author().to_hex(), corpus.other_author);
    for permutation in lifecycle::ALL_THREE_PERMUTATIONS {
        let source = [&exact, &address, &forged];
        let requests: Vec<_> = permutation
            .into_iter()
            .map(|index| (*source[index]).clone())
            .collect();
        let expected =
            evaluate_nip09_suppression_from_borrowed_requests_v1(&target, requests.iter());
        let head = AvailabilityHeadView::from_selected(
            selected(&[&target]),
            version_view(target_json),
            &requests,
        )
        .expect("shared evidence retained");
        assert_eq!(head.state(), AvailabilityHeadState::Deleted);
        let decision = head.suppression().expect("canonical decision");
        assert_eq!(decision, &expected);
        assert_eq!(
            decision.outcome(),
            RadrootsNip09SuppressionOutcome::Suppressed
        );
        assert_eq!(
            decision.reason(),
            RadrootsNip09SuppressionReason::EventIdAndAddressReference
        );
        assert_eq!(
            decision
                .event_reference()
                .expect("exact evidence")
                .request_id(),
            exact.event().id()
        );
        let evidence = decision.address_reference().expect("address evidence");
        assert_eq!(
            evidence.coordinate().as_str(),
            format!("30402:{}:{}", corpus.author, lifecycle::IDENTIFIER)
        );
        assert_eq!(evidence.inclusive_cutoff(), lifecycle::CUTOFF);
        assert_eq!(
            evidence.request_id(),
            std::cmp::min(exact.event().id(), address.event().id())
        );
    }
    let new_json = lifecycle_json(&corpus, "newer_sold");
    let new = verified(new_json);
    let forged_only = AvailabilityHeadView::from_selected(
        selected(&[&new]),
        version_view(new_json),
        &[deletion(&corpus, "forged_exact")],
    )
    .expect("request admission is not author authorization");
    assert_eq!(forged_only.state(), AvailabilityHeadState::Focused);
    assert_eq!(
        forged_only
            .suppression()
            .expect("author mismatch evidence")
            .reason(),
        RadrootsNip09SuppressionReason::RequestAuthorMismatch
    );
    for (name, expected_state, expected_reason) in [
        (
            "before_cutoff",
            AvailabilityHeadState::Deleted,
            RadrootsNip09SuppressionReason::AddressReferenceAtOrBeforeCutoff,
        ),
        (
            "at_cutoff",
            AvailabilityHeadState::Deleted,
            RadrootsNip09SuppressionReason::AddressReferenceAtOrBeforeCutoff,
        ),
        (
            "after_cutoff",
            AvailabilityHeadState::Focused,
            RadrootsNip09SuppressionReason::AddressCutoffPrecedesTarget,
        ),
    ] {
        let json = lifecycle_json(&corpus, name);
        let head = AvailabilityHeadView::from_selected(
            selected(&[&verified(json)]),
            version_view(json),
            std::slice::from_ref(&address),
        )
        .expect("inclusive shared address cutoff");
        assert_eq!(head.state(), expected_state);
        assert_eq!(
            head.suppression().expect("cutoff decision").reason(),
            expected_reason
        );
    }
    // Dedicated capacity boundary: at most4097 duplicate public requests and16MiB
    // of admitted fixture inputs; production must borrow this slice, never clone it.
    let request_bytes = lifecycle_json(&corpus, "address_cutoff").len();
    assert!(request_bytes * (MAX_DELETION_REQUESTS + 1) <= 16 * 1024 * 1024);
    let mut requests = vec![address; MAX_DELETION_REQUESTS];
    assert_eq!(requests.len(), 4096);
    assert_eq!(
        AvailabilityHeadView::from_selected(
            selected(&[&target]),
            version_view(target_json),
            &requests
        )
        .expect("exact request capacity")
        .state(),
        AvailabilityHeadState::Deleted
    );
    requests.push(exact);
    assert_eq!(requests.len(), 4097);
    assert_safe(
        AvailabilityHeadView::from_selected(
            selected(&[&target]),
            version_view(target_json),
            &requests,
        )
        .expect_err("one request beyond capacity"),
        target_json,
    );
}

#[test]
fn invalid_signatures_cannot_create_verified_views() {
    let fixture = public_fixture(admission::focused_recipe());
    let wire = Nip01EventWire::parse_json_unverified(&fixture.json).expect("public base wire");
    let invalid_signature = replace_string_field(&fixture.json, "sig", &wire.sig, &"0".repeat(128));
    let unverified = Nip01EventWire::parse_json_unverified(&invalid_signature)
        .expect("structurally valid invalid signature")
        .into_unverified_envelope()
        .expect("unverified envelope");
    assert_eq!(
        verify_nip01_event(unverified).expect_err("signature gate owns verified typestate"),
        RadrootsNip01VerificationError::SignatureInvalid
    );
    assert_safe(
        AvailabilityVersionView::from_verified(
            verified(&fixture.json),
            &invalid_signature,
            observation(),
        )
        .expect_err("invalid signature wire cannot bind another verified event"),
        &invalid_signature,
    );
    let replacement = if wire.id.starts_with('0') { '1' } else { '0' };
    let bad_id = format!("{replacement}{}", &wire.id[1..]);
    let invalid_id = replace_string_field(&fixture.json, "id", &wire.id, &bad_id);
    let envelope = Nip01EventWire::parse_json_unverified(&invalid_id)
        .expect("invalid ID is not structurally trusted")
        .into_unverified_envelope()
        .expect("unverified envelope");
    assert!(matches!(
        verify_nip01_event(envelope),
        Err(RadrootsNip01VerificationError::IdMismatch { .. })
    ));
}

#[test]
fn original_wire_must_match_the_verified_event() {
    let fixture = public_fixture(admission::focused_recipe());
    let neighbor = public_fixture(admission::focused_recipe());
    let event = verified(&fixture.json);
    let wire = Nip01EventWire::parse_json_unverified(&fixture.json).expect("public wire fields");
    let neighbor_wire = Nip01EventWire::parse_json_unverified(&neighbor.json)
        .expect("distinct valid public fields");
    let mismatches = [
        replace_string_field(&fixture.json, "id", &wire.id, &neighbor_wire.id),
        replace_string_field(&fixture.json, "pubkey", &wire.pubkey, &neighbor.author),
        replace_once(
            &fixture.json,
            &format!("\"created_at\":{}", admission::CREATED_AT),
            "\"created_at\":1800000101",
        ),
        replace_once(&fixture.json, "\"kind\":30402", "\"kind\":30023"),
        replace_once(
            &fixture.json,
            "[\"status\",\"active\"]",
            "[\"status\",\"sold\"]",
        ),
        replace_string_field(
            &fixture.json,
            "content",
            &wire.content,
            "Different public content.",
        ),
        replace_string_field(&fixture.json, "sig", &wire.sig, &neighbor_wire.sig),
    ];
    for mismatch in &mismatches {
        let parsed = Nip01EventWire::parse_json_unverified(mismatch)
            .expect("each altered field stays default structurally valid")
            .into_unverified_envelope()
            .expect("unverified altered envelope");
        assert_ne!(&parsed, event.event());
        assert_safe(
            AvailabilityVersionView::from_verified(event.clone(), mismatch, observation())
                .expect_err("full envelope equality required"),
            mismatch,
        );
    }
    assert_eq!(mismatches.len(), 7);
    for extras in ["\"untrusted\":true", "\"untrusted\":false"] {
        let exact = format!("\n {} \t", add_extras(&fixture.json, extras));
        let retained = AvailabilityVersionView::from_verified(event.clone(), &exact, observation())
            .expect("unknown extras and formatting remain unauthenticated");
        assert_eq!(retained.original_json(), exact);
        assert_eq!(retained.version().event_id(), *event.event().id());
        assert_eq!(
            retained
                .focused()
                .expect("extras cannot change quantity")
                .quantity(),
            None
        );
    }
}

#[test]
fn view_wire_and_source_retention_obey_shared_byte_limits() {
    let mut recipe = admission::focused_recipe();
    recipe.content = "Public wire byte boundary é🥕e\u{301}.".into();
    let fixture = public_fixture(recipe);
    assert!(fixture.json.chars().count() < fixture.json.len());
    let event = verified(&fixture.json);
    assert_eq!(DEFAULT_RAW_JSON_MAX_BYTES, 256 * 1024);
    // Dedicated raw-wire exception to the16KiB signed-fixture bound: whitespace
    // padding creates exactly256KiB and256KiB+1 without enlarging signed fields.
    let exact = format!(
        "{}{}",
        fixture.json,
        " ".repeat(DEFAULT_RAW_JSON_MAX_BYTES - fixture.json.len())
    );
    assert_eq!(exact.len(), DEFAULT_RAW_JSON_MAX_BYTES);
    assert_eq!(
        AvailabilityVersionView::from_verified(event.clone(), &exact, observation())
            .expect("exact raw-wire capacity")
            .original_json()
            .as_bytes(),
        exact.as_bytes()
    );
    let over = exact + " ";
    assert_eq!(over.len(), DEFAULT_RAW_JSON_MAX_BYTES + 1);
    assert_safe(
        AvailabilityVersionView::from_verified(event.clone(), &over, observation())
            .expect_err("raw capacity plus one"),
        "hcav-incoming-v1",
    );
    let invalid_tags = replace_once(&fixture.json, "\"tags\":[", "\"tags\":[[0],");
    let overlong_tag = replace_once(
        &fixture.json,
        "[\"status\",\"active\"]",
        &format!(
            "[\"status\",\"{}\"]",
            "x".repeat(DEFAULT_TAG_ELEMENT_MAX_BYTES + 1)
        ),
    );
    let too_many_fields = (0..=DEFAULT_EXTRA_MAX_FIELDS)
        .map(|index| format!("\"extra_{index}\":0"))
        .collect::<Vec<_>>()
        .join(",");
    let oversized_extra = format!(
        "\"extra\":\"{}\"",
        "x".repeat(DEFAULT_EXTRA_TOTAL_JSON_MAX_BYTES + 1)
    );
    for invalid in [
        invalid_tags,
        overlong_tag,
        add_extras(&fixture.json, &too_many_fields),
        add_extras(&fixture.json, &oversized_extra),
    ] {
        assert!(invalid.len() <= DEFAULT_RAW_JSON_MAX_BYTES);
        assert!(
            Nip01EventWire::parse_json_unverified(&invalid).is_err(),
            "shared default structural boundary"
        );
        assert_safe(
            AvailabilityVersionView::from_verified(event.clone(), &invalid, observation())
                .expect_err("shared limits precede retention"),
            &invalid,
        );
    }
    let other_kind = public_fixture_at(
        admission::focused_recipe(),
        lifecycle::OTHER_ADDRESSABLE_KIND,
        admission::CREATED_AT,
    );
    assert_safe(
        AvailabilityVersionView::from_verified(
            verified(&other_kind.json),
            &other_kind.json,
            observation(),
        )
        .expect_err("view only supports kind30402"),
        &other_kind.json,
    );
    for unit in ["x", "é", "🥕", "e\u{301}"] {
        let prefix = "wss://relay.example.test/source/";
        let source = prefix.to_owned() + &text_at_bytes(unit, MAX_SOURCE_BYTES - prefix.len());
        assert_eq!(source.len(), MAX_SOURCE_BYTES);
        let observed = AvailabilityObservation::parse(&source, UnixTimestamp::UNIX_EPOCH)
            .expect("exact UTF-8 source byte bound");
        assert_eq!(observed.source().as_str().as_bytes(), source.as_bytes());
        let over = source + "x";
        assert_eq!(over.len(), MAX_SOURCE_BYTES + 1);
        assert_safe(
            AvailabilityObservation::parse(&over, UnixTimestamp::UNIX_EPOCH)
                .expect_err("source byte bound plus one"),
            &over,
        );
    }
}

#[test]
fn view_errors_never_echo_untrusted_input() {
    const PAYLOAD: &str = "private-debug-payload-marker";
    let mut recipe = admission::focused_recipe();
    recipe.content = PAYLOAD.into();
    recipe
        .tags
        .iter_mut()
        .find(|tag| tag[0] == "d")
        .expect("identifier")[1] = format!("{PAYLOAD}:\0 ");
    let fixture = public_fixture(recipe);
    let source = format!("wss://relay.example.test/{PAYLOAD}?metadata={PAYLOAD}");
    let observation = AvailabilityObservation::parse(&source, UnixTimestamp::UNIX_EPOCH)
        .expect("public opaque source");
    let version = AvailabilityVersionView::from_verified(
        verified(&fixture.json),
        &fixture.json,
        observation.clone(),
    )
    .expect("unsupported evidence view");
    let metadata = ProfileMetadata::new(
        Some(PAYLOAD.into()),
        None,
        None,
        Some(PAYLOAD.into()),
        Some(format!("https://images.example.test/{PAYLOAD}")),
    )
    .expect("bounded display fields");
    let candidate = Kind0ProfileCandidate::new(
        EventId::from_bytes([7; 32]),
        PublicKey::from_hex(&fixture.author).expect("author"),
        UnixTimestamp::UNIX_EPOCH,
        metadata,
    );
    let attached = version
        .with_profile(Some(&candidate))
        .expect("caller-associated display metadata");
    let head = AvailabilityHeadView::from_selected(
        selected(&[&verified(&fixture.json)]),
        attached.clone(),
        &[],
    )
    .expect("unsupported head");
    let missing = AvailabilityHeadView::missing(
        AvailabilityListingCoordinate::parse(&format!("30402:{}:{PAYLOAD}", fixture.author))
            .expect("opaque missing reference"),
    );
    for debug in [
        format!("{observation:?}"),
        format!("{version:?}"),
        format!("{attached:?}"),
        format!("{head:?}"),
        format!("{missing:?}"),
    ] {
        assert!(debug.len() <= 1024, "bounded safe summary");
        for sensitive in [PAYLOAD, "wss://", "https://", "\"tags\"", "\"sig\""] {
            assert!(
                !debug.contains(sensitive),
                "debug cannot expose raw payloads"
            );
        }
    }
    for source in [
        format!("https://relay.example.test/{PAYLOAD}"),
        format!("wss://user:{PAYLOAD}@relay.example.test"),
        format!("wss://relay.example.test/#{PAYLOAD}"),
        format!("wss://relay.example.test/\n{PAYLOAD}"),
    ] {
        assert_safe(
            AvailabilityObservation::parse(&source, UnixTimestamp::UNIX_EPOCH)
                .expect_err("shared source validation returns static errors"),
            PAYLOAD,
        );
    }
    let malformed = format!("{{{PAYLOAD}");
    assert_safe(
        AvailabilityVersionView::from_verified(verified(&fixture.json), &malformed, observation)
            .expect_err("malformed wire safe error"),
        PAYLOAD,
    );
}

#[test]
fn optional_profile_attachment_checks_publisher_and_preserves_missing_fields() {
    let fixture = public_fixture(admission::focused_recipe());
    let view = version_view(&fixture.json);
    assert_eq!(view.profile_metadata(), None);
    let metadata = ProfileMetadata::new(None, Some(" Caller display ".into()), None, None, None)
        .expect("bounded optional display metadata");
    // This constructor has no signature typestate. The association proves only
    // matching author and bounded display data, not verified profile or ownership.
    let candidate = Kind0ProfileCandidate::new(
        EventId::from_bytes([9; 32]),
        PublicKey::from_hex(&fixture.author).expect("listing author"),
        UnixTimestamp::UNIX_EPOCH,
        metadata.clone(),
    );
    let attached = view
        .with_profile(Some(&candidate))
        .expect("same-author caller display metadata");
    assert_eq!(attached.profile_metadata(), Some(&metadata));
    let attached_metadata = attached.profile_metadata().expect("attached fields");
    assert_eq!(attached_metadata.display_name(), Some("Caller display"));
    assert_eq!(attached_metadata.name(), None);
    assert_eq!(attached_metadata.nip05(), None);
    assert_eq!(attached_metadata.about(), None);
    assert_eq!(attached_metadata.picture(), None);
    assert_eq!(view.profile_metadata(), None, "immutable original");
    assert_eq!(attached.version(), view.version());
    assert_eq!(attached.focused(), view.focused());
    assert_eq!(attached.original_json(), view.original_json());
    assert_eq!(
        attached
            .with_profile(None)
            .expect("explicit absent optional metadata")
            .profile_metadata(),
        None
    );
    let other = public_fixture(admission::focused_recipe());
    assert_ne!(other.author, fixture.author);
    let wrong = Kind0ProfileCandidate::new(
        EventId::from_bytes([8; 32]),
        PublicKey::from_hex(&other.author).expect("other public author"),
        UnixTimestamp::UNIX_EPOCH,
        metadata,
    );
    assert_safe(
        view.with_profile(Some(&wrong))
            .expect_err("author mismatch before metadata copy"),
        "Caller display",
    );
    assert_eq!(
        view.focused()
            .expect("failed optional attachment preserves listing")
            .status()
            .as_str(),
        "active"
    );
    assert_eq!(
        view.version(),
        AvailabilityEventVersion::from_canonical(*verified(&fixture.json).event().id())
    );
}

#[test]
fn observation_preserves_bounded_source_reference_and_observation_time() {
    let source = "wss://Relay.Example.test:443/path?region=ca-bc";
    let local = UnixTimestamp::from_seconds(i64::MAX).expect("local timestamp upper bound");
    let observation = AvailabilityObservation::parse(source, local)
        .expect("pure shared validation preserves supplied spelling");
    assert_eq!(observation.source().as_str(), source);
    assert_eq!(observation.observed_at(), local);
    assert_ne!(
        observation.source().as_str(),
        "wss://relay.example.test/path?region=ca-bc"
    );
    let empty_time =
        AvailabilityObservation::parse("ws://127.0.0.1:8080/isolated", UnixTimestamp::UNIX_EPOCH)
            .expect("provenance is not endpoint authorization");
    assert_eq!(empty_time.observed_at().as_seconds(), 0);
    for invalid in [
        "",
        "http://relay.example.test",
        "WSS://relay.example.test",
        " wss://relay.example.test",
        "wss://",
        "wss://relay.example.test:0",
    ] {
        let error = AvailabilityObservation::parse(invalid, local)
            .expect_err("delegate selected shared pure source validation");
        assert_eq!(error.code(), SafeErrorCode::InvalidProfileMetadata);
        assert!(error.to_string().len() <= 128);
    }
}
