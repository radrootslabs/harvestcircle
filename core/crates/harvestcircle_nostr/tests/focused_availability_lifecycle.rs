use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::envelope::event_head::{
    CurrentEventHead, EventHeadCandidate, EventHeadCandidateResult, EventHeadCoordinate,
    EventHeadDecision, event_head_candidate_for_nip01_event, select_event_head,
};
use radroots_event::food::availability::FoodAvailabilityError;
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::wire::Nip01EventWire;
use radroots_event_codec::admission::deletion::{
    RadrootsAdmittedNip09DeletionRequestEvent, RadrootsNip09DeletionAdmissionError,
    RadrootsNip09SuppressionOutcome, RadrootsNip09SuppressionReason, evaluate_nip09_suppression,
    verify_and_admit_nip09_deletion_request_event,
};
use radroots_event_codec::admission::food_availability::{
    RadrootsFoodAvailabilityAdmissionError, RadrootsFoodAvailabilityAdmissionOutcome,
    admit_verified_food_availability_event,
};
use radroots_event_codec::decode::food_availability::RadrootsFoodAvailabilityProjectionError;
use radroots_event_codec::verify::{
    RadrootsNip01VerificationError, RadrootsSignatureVerifiedEvent, verify_nip01_event,
};

#[path = "fixtures/availability_lifecycle_v1.rs"]
mod corpus;

const MAX_EVENT_BYTES: usize = 4 * 1024;
const MAX_CORPUS_BYTES: usize = 64 * 1024;
const MAX_RETAINED_SOURCE_BYTES: usize = 16 * 1024;
const RETAINED_SOURCE: &str = include_str!("fixtures/availability_lifecycle_v1.rs");

struct PublicEvent {
    name: &'static str,
    json: String,
}

struct PublicCorpus {
    author: String,
    other_author: String,
    events: Vec<PublicEvent>,
}

fn sign_event(
    keys: &Keys,
    kind: u16,
    created_at: u64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> Event {
    assert!(tags.len() <= 16);
    assert!(
        content.len() + tags.iter().flatten().map(String::len).sum::<usize>() <= MAX_EVENT_BYTES
    );
    let tags = tags
        .into_iter()
        .map(|tag| Tag::parse(tag).expect("bounded public raw tag"));
    EventBuilder::new(Kind::from(kind), content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("isolated public lifecycle signing")
}

/// Both signers are dropped before any retained public JSON leaves the helper.
fn public_corpus() -> PublicCorpus {
    let owner = Keys::generate();
    let other = Keys::generate();
    let author = owner.public_key().to_hex();
    let other_author = other.public_key().to_hex();
    assert_ne!(author, other_author);
    let mut signed = Vec::<(&'static str, Event)>::new();
    for version in &corpus::VERSIONS {
        let keys = if version.other_author { &other } else { &owner };
        signed.push((
            version.name,
            sign_event(
                keys,
                30402,
                version.created_at,
                version.content,
                corpus::listing_tags(version),
            ),
        ));
    }
    let original = &corpus::VERSIONS[0];
    assert_eq!(original.name, "old_active");
    signed.push((
        "other_kind",
        sign_event(
            &owner,
            corpus::OTHER_ADDRESSABLE_KIND,
            original.created_at,
            original.content,
            corpus::listing_tags(original),
        ),
    ));
    let address = format!("30402:{author}:{}", corpus::IDENTIFIER);
    for deletion in &corpus::DELETIONS {
        let mut tags = Vec::new();
        match deletion.target {
            corpus::Target::Event(name) | corpus::Target::EventAndAddress(name) => {
                let target = signed
                    .iter()
                    .find(|(target_name, _)| *target_name == name)
                    .expect("public target recipe exists");
                tags.push(vec!["e".into(), target.1.id.to_hex()]);
            }
            corpus::Target::Address => {}
        }
        if matches!(
            deletion.target,
            corpus::Target::Address | corpus::Target::EventAndAddress(_)
        ) {
            tags.push(vec!["a".into(), address.clone()]);
        }
        tags.push(vec!["k".into(), "30402".into()]);
        let keys = if deletion.other_author {
            &other
        } else {
            &owner
        };
        signed.push((
            deletion.name,
            sign_event(
                keys,
                5,
                deletion.created_at,
                "Public deletion conformance.",
                tags,
            ),
        ));
    }
    drop(owner);
    drop(other);
    let events = signed
        .into_iter()
        .map(|(name, event)| {
            let json = event.as_json();
            assert!(json.len() <= MAX_EVENT_BYTES);
            PublicEvent { name, json }
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 18);
    assert!(events.iter().map(|event| event.json.len()).sum::<usize>() <= MAX_CORPUS_BYTES);
    PublicCorpus {
        author,
        other_author,
        events,
    }
}

fn public_json<'a>(corpus: &'a PublicCorpus, name: &str) -> &'a str {
    &corpus
        .events
        .iter()
        .find(|event| event.name == name)
        .expect("named public event")
        .json
}

fn verified(corpus: &PublicCorpus, name: &str) -> RadrootsSignatureVerifiedEvent {
    let wire = Nip01EventWire::parse_json(public_json(corpus, name))
        .expect("bounded public ID-verified wire");
    assert!(wire.extra.is_empty());
    verify_nip01_event(wire.into_envelope().expect("ID-verified envelope"))
        .expect("shared ID and signature verification")
}

fn deletion(corpus: &PublicCorpus, name: &str) -> RadrootsAdmittedNip09DeletionRequestEvent {
    let wire = Nip01EventWire::parse_json(public_json(corpus, name)).expect("public deletion wire");
    verify_and_admit_nip09_deletion_request_event(wire.into_envelope().expect("deletion envelope"))
        .expect("shared NIP-09 request admission without a target lookup")
}

fn candidate(event: &RadrootsSignatureVerifiedEvent) -> EventHeadCandidate {
    match event_head_candidate_for_nip01_event(event.event()) {
        EventHeadCandidateResult::Candidate(candidate) => candidate,
        _ => panic!("verified addressable fixture must produce a raw protocol head candidate"),
    }
}

/// Every supplied verified version enters shared ordering before product projection or suppression.
fn select_all_versions(versions: &[&RadrootsSignatureVerifiedEvent]) -> CurrentEventHead {
    assert!(!versions.is_empty());
    assert!(versions.len() <= 3);
    let mut head = None;
    for version in versions {
        match select_event_head(candidate(version), head.as_ref()) {
            EventHeadDecision::Applied(next) => head = Some(next),
            EventHeadDecision::SkippedDuplicate
            | EventHeadDecision::SkippedOlder
            | EventHeadDecision::SkippedSameTimestampHigherEventId => {}
            EventHeadDecision::CoordinateMismatch => {
                panic!("single-coordinate permutation fixture")
            }
        }
    }
    head.expect("shared selected head")
}

fn assert_focused_status(event: &RadrootsSignatureVerifiedEvent, status: &str) {
    match admit_verified_food_availability_event(event.clone()).expect("focused head admission") {
        RadrootsFoodAvailabilityAdmissionOutcome::Admitted(admitted) => {
            assert_eq!(admitted.projection().status().as_str(), status);
        }
        _ => panic!("selected focused version must be admitted"),
    }
}

#[test]
fn raw_head_selection_preserves_reverse_arrival_and_exact_duplicates() {
    let corpus = public_corpus();
    let old = verified(&corpus, "old_active");
    let new = verified(&corpus, "newer_sold");
    let mut cases = 0;
    for order in [[&old, &new, &new], [&new, &old, &new], [&new, &new, &old]] {
        let head = select_all_versions(&order);
        assert_eq!(head.event_id, *new.event().id());
        assert_eq!(head.created_at, 1_800_000_200);
        assert_eq!(
            select_event_head(candidate(&new), Some(&head)),
            EventHeadDecision::SkippedDuplicate
        );
        assert_eq!(
            select_event_head(candidate(&old), Some(&head)),
            EventHeadDecision::SkippedOlder
        );
        assert_focused_status(&new, "sold");
        cases += 1;
    }
    assert_eq!(cases, 3);
}

#[test]
fn equal_timestamp_heads_select_lowest_event_id_in_every_order() {
    let corpus = public_corpus();
    let left = verified(&corpus, "tie_left");
    let right = verified(&corpus, "tie_right");
    assert_ne!(left.event().id(), right.event().id());
    assert_eq!(
        left.event().created_at_u64(),
        right.event().created_at_u64()
    );
    let (lowest, higher) = if left.event().id() < right.event().id() {
        (&left, &right)
    } else {
        (&right, &left)
    };
    let mut cases = 0;
    for order in [[&left, &right, &left], [&right, &left, &right]] {
        let head = select_all_versions(&order);
        assert_eq!(head.event_id, *lowest.event().id());
        assert_eq!(head.created_at, 1_800_000_300);
        assert_eq!(
            select_event_head(candidate(higher), Some(&head)),
            EventHeadDecision::SkippedSameTimestampHigherEventId
        );
        assert_eq!(
            select_event_head(candidate(lowest), Some(&head)),
            EventHeadDecision::SkippedDuplicate
        );
        cases += 1;
    }
    assert_eq!(cases, 2);
}

#[test]
fn shared_coordinates_keep_distinct_authors_kinds_and_identifiers_separate() {
    let corpus = public_corpus();
    let original = verified(&corpus, "old_active");
    let head = select_all_versions(&[&original]);
    match &head.coordinate {
        EventHeadCoordinate::Addressable {
            kind,
            pubkey,
            d_tag,
        } => {
            assert_eq!(*kind, 30402);
            assert_eq!(pubkey.to_hex(), corpus.author);
            assert_eq!(d_tag, "hcav-lifecycle-v1");
        }
        _ => panic!("kind 30402 has an addressable coordinate"),
    }
    for name in ["other_identifier", "other_author", "other_kind"] {
        let distinct = verified(&corpus, name);
        assert_eq!(distinct.event().content(), original.event().content());
        assert_eq!(
            select_event_head(candidate(&distinct), Some(&head)),
            EventHeadDecision::CoordinateMismatch
        );
        assert_ne!(candidate(&distinct).coordinate, head.coordinate);
        if name == "other_kind" {
            assert_eq!(distinct.event().author(), original.event().author());
            match &candidate(&distinct).coordinate {
                EventHeadCoordinate::Addressable {
                    kind,
                    pubkey,
                    d_tag,
                } => {
                    assert_eq!(*kind, u32::from(corpus::OTHER_ADDRESSABLE_KIND));
                    assert_ne!(*kind, original.event().kind_u32());
                    assert_eq!(*pubkey, *original.event().author());
                    assert_eq!(d_tag, corpus::IDENTIFIER);
                }
                _ => panic!("control must have an addressable coordinate"),
            }
        }
        assert_eq!(
            select_all_versions(&[&distinct]).event_id,
            *distinct.event().id()
        );
    }
    assert_eq!(
        verified(&corpus, "other_author").event().author().to_hex(),
        corpus.other_author
    );
}

#[test]
fn exact_deletion_before_target_and_deleted_head_never_resurrect_older_active() {
    let corpus = public_corpus();
    // Admission first retains the request without consulting any target event.
    let request = deletion(&corpus, "exact_newer");
    assert_eq!(request.event().created_at_u64(), 1_800_000_000);
    let old = verified(&corpus, "old_active");
    let new = verified(&corpus, "newer_sold");
    assert!(request.event().created_at_u64() < new.event().created_at_u64());
    let mut cases = 0;
    for order in corpus::ALL_THREE_PERMUTATIONS {
        let mut versions = Vec::new();
        let mut requests = Vec::new();
        for index in order {
            match index {
                0 => versions.push(&old),
                1 => versions.push(&new),
                2 => requests.push(request.clone()),
                _ => unreachable!("three-item permutation"),
            }
            if !versions.is_empty() {
                let head = select_all_versions(&versions);
                let winner = if head.event_id == *new.event().id() {
                    &new
                } else {
                    &old
                };
                let decision = evaluate_nip09_suppression(winner, &requests);
                if head.event_id == *new.event().id() && !requests.is_empty() {
                    assert_eq!(
                        decision.outcome(),
                        RadrootsNip09SuppressionOutcome::Suppressed
                    );
                    assert_eq!(
                        decision.reason(),
                        RadrootsNip09SuppressionReason::EventIdReference
                    );
                    assert_eq!(
                        decision
                            .event_reference()
                            .expect("exact reference evidence")
                            .request_id(),
                        request.event().id()
                    );
                } else {
                    assert_eq!(decision.outcome(), RadrootsNip09SuppressionOutcome::Visible);
                    assert_eq!(
                        decision.reason(),
                        RadrootsNip09SuppressionReason::NoAuthorizedReference
                    );
                }
            }
        }
        let head = select_all_versions(&versions);
        assert_eq!(head.event_id, *new.event().id());
        assert_eq!(
            evaluate_nip09_suppression(&new, &requests).outcome(),
            RadrootsNip09SuppressionOutcome::Suppressed
        );
        assert_eq!(
            evaluate_nip09_suppression(&old, &requests).outcome(),
            RadrootsNip09SuppressionOutcome::Visible
        );
        assert_eq!(
            select_event_head(candidate(&old), Some(&head)),
            EventHeadDecision::SkippedOlder
        );
        assert_focused_status(&old, "active");
        assert_ne!(head.event_id, *old.event().id());
        cases += 1;
    }
    assert_eq!(cases, 6);
}

#[test]
fn address_deletion_cutoff_is_inclusive_and_arrival_order_independent() {
    let corpus = public_corpus();
    let request = deletion(&corpus, "address_cutoff");
    assert_eq!(request.event().created_at_u64(), 1_800_000_100);
    let mut cases = 0;
    for (name, outcome, reason) in [
        (
            "before_cutoff",
            RadrootsNip09SuppressionOutcome::Suppressed,
            RadrootsNip09SuppressionReason::AddressReferenceAtOrBeforeCutoff,
        ),
        (
            "at_cutoff",
            RadrootsNip09SuppressionOutcome::Suppressed,
            RadrootsNip09SuppressionReason::AddressReferenceAtOrBeforeCutoff,
        ),
        (
            "after_cutoff",
            RadrootsNip09SuppressionOutcome::Visible,
            RadrootsNip09SuppressionReason::AddressCutoffPrecedesTarget,
        ),
    ] {
        for order in [[0, 1], [1, 0]] {
            let mut target = None;
            let mut retained = Vec::new();
            for arrival in order {
                match arrival {
                    0 => target = Some(verified(&corpus, name)),
                    1 => retained.push(deletion(&corpus, "address_cutoff")),
                    _ => unreachable!("two-item arrival permutation"),
                }
                if let Some(target) = target.as_ref() {
                    let head = select_all_versions(&[target]);
                    assert_eq!(head.event_id, *target.event().id());
                    let decision = evaluate_nip09_suppression(target, &retained);
                    if retained.is_empty() {
                        assert_eq!(decision.outcome(), RadrootsNip09SuppressionOutcome::Visible);
                        assert_eq!(
                            decision.reason(),
                            RadrootsNip09SuppressionReason::NoAuthorizedReference
                        );
                    } else {
                        assert_eq!(decision.outcome(), outcome);
                        assert_eq!(decision.reason(), reason);
                    }
                } else {
                    assert_eq!(
                        retained.len(),
                        1,
                        "deletion retained before any target observation"
                    );
                }
            }
            let target = target.expect("target observed in each permutation");
            let head = select_all_versions(&[&target]);
            let decision = evaluate_nip09_suppression(&target, &retained);
            assert_eq!(head.event_id, *target.event().id());
            assert_eq!(decision.outcome(), outcome);
            assert_eq!(decision.reason(), reason);
            assert!(decision.event_reference().is_none());
            let evidence = decision
                .address_reference()
                .expect("retained matching address evidence");
            assert_eq!(evidence.inclusive_cutoff(), corpus::CUTOFF);
            assert_eq!(evidence.request_id(), request.event().id());
            assert_eq!(evidence.coordinate().kind(), 30402);
            assert_eq!(evidence.coordinate().pubkey().to_hex(), corpus.author);
            assert_eq!(evidence.coordinate().identifier(), "hcav-lifecycle-v1");
            cases += 1;
        }
    }
    assert_eq!(cases, 6);
    let combined = deletion(&corpus, "combined_cutoff");
    let target = verified(&corpus, "at_cutoff");
    let decision = evaluate_nip09_suppression(&target, std::slice::from_ref(&combined));
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
        combined.event().id()
    );
    assert_eq!(
        decision
            .address_reference()
            .expect("address evidence")
            .request_id(),
        combined.event().id()
    );
}

#[test]
fn forged_deletions_cannot_suppress_other_author_and_invalid_signatures_are_rejected() {
    let corpus = public_corpus();
    let target = verified(&corpus, "newer_sold");
    let head = select_all_versions(&[&target]);
    for name in ["forged_exact", "forged_address"] {
        let forged = deletion(&corpus, name);
        assert_eq!(forged.event().author().to_hex(), corpus.other_author);
        assert_ne!(forged.event().author(), target.event().author());
        let decision = evaluate_nip09_suppression(&target, std::slice::from_ref(&forged));
        assert_eq!(decision.outcome(), RadrootsNip09SuppressionOutcome::Visible);
        assert_eq!(
            decision.reason(),
            RadrootsNip09SuppressionReason::RequestAuthorMismatch
        );
        assert!(decision.event_reference().is_none());
        assert!(decision.address_reference().is_none());
        assert_eq!(head.event_id, *target.event().id());
    }
    let unrelated = deletion(&corpus, "unrelated_exact");
    let decision = evaluate_nip09_suppression(&target, std::slice::from_ref(&unrelated));
    assert_eq!(decision.outcome(), RadrootsNip09SuppressionOutcome::Visible);
    assert_eq!(
        decision.reason(),
        RadrootsNip09SuppressionReason::NoAuthorizedReference
    );

    let json = public_json(&corpus, "exact_newer");
    let wire = Nip01EventWire::parse_json_unverified(json).expect("public deletion wire");
    let signature_field = format!("\"sig\":\"{}\"", wire.sig);
    assert_eq!(json.matches(&signature_field).count(), 1);
    let tampered = json.replacen(
        &signature_field,
        &format!("\"sig\":\"{}\"", "0".repeat(128)),
        1,
    );
    let envelope = Nip01EventWire::parse_json_unverified(&tampered)
        .expect("bounded invalid-signature wire")
        .into_unverified_envelope()
        .expect("untrusted deletion envelope");
    assert_eq!(
        verify_and_admit_nip09_deletion_request_event(envelope)
            .expect_err("invalid deletion has no admitted request authority"),
        RadrootsNip09DeletionAdmissionError::Nip01Verification(
            RadrootsNip01VerificationError::SignatureInvalid
        )
    );
}

#[test]
fn newer_verified_unsupported_heads_never_promote_older_active_versions() {
    let corpus = public_corpus();
    let old = verified(&corpus, "old_active");
    let mut cases = 0;
    for name in ["generic_head", "malformed_head"] {
        let unsupported = verified(&corpus, name);
        for order in [
            [&old, &unsupported, &old],
            [&unsupported, &old, &unsupported],
        ] {
            // The unsupported version participates before focused admission is attempted.
            let head = select_all_versions(&order);
            assert_eq!(head.event_id, *unsupported.event().id());
            assert_eq!(head.created_at, 1_800_000_200);
            let result = admit_verified_food_availability_event(unsupported.clone());
            if name == "generic_head" {
                match result.expect("verified generic exclusion") {
                    RadrootsFoodAvailabilityAdmissionOutcome::Excluded(candidate) => {
                        assert_eq!(
                            candidate.partition(),
                            ClassifiedListingPartition::GenericNip99
                        );
                        assert_eq!(candidate.event().id(), &head.event_id);
                    }
                    _ => panic!("generic winning head remains unsupported"),
                }
            } else {
                assert_eq!(
                    result.expect_err("malformed focused winning head remains known"),
                    RadrootsFoodAvailabilityAdmissionError::Projection(
                        RadrootsFoodAvailabilityProjectionError::Domain(
                            FoodAvailabilityError::StatusInvalid
                        )
                    )
                );
            }
            assert_eq!(
                evaluate_nip09_suppression(&unsupported, &[]).outcome(),
                RadrootsNip09SuppressionOutcome::Visible
            );
            assert_eq!(
                select_event_head(candidate(&old), Some(&head)),
                EventHeadDecision::SkippedOlder
            );
            assert_focused_status(&old, "active");
            assert_ne!(head.event_id, *old.event().id());
            cases += 1;
        }
    }
    assert_eq!(cases, 4);
}

#[test]
fn lifecycle_corpus_is_bounded_and_retains_only_verified_public_material() {
    assert_eq!(corpus::VERSIONS.len(), 11);
    assert_eq!(corpus::DELETIONS.len(), 6);
    assert_eq!(corpus::ALL_THREE_PERMUTATIONS.len(), 6);
    assert!(RETAINED_SOURCE.len() <= MAX_RETAINED_SOURCE_BYTES);
    for forbidden in [
        "nsec1",
        "SecretKey",
        "secret_key",
        "private_key",
        "Keys::",
        "credential",
    ] {
        assert!(
            !RETAINED_SOURCE.contains(forbidden),
            "public retained raw recipes"
        );
    }
    let corpus = public_corpus();
    assert_eq!(corpus.events.len(), 18);
    for event in &corpus.events {
        for forbidden in ["nsec1", "secret_key", "private_key", "credential"] {
            assert!(
                !event.json.contains(forbidden),
                "public signed lifecycle artifact"
            );
        }
        let verified = verified(&corpus, event.name);
        assert!(matches!(verified.event().kind_u32(), 5 | 30402 | 30023));
        let author = verified.event().author().to_hex();
        assert!(author == corpus.author || author == corpus.other_author);
    }
}
