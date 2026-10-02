use std::error::Error;

use harvestcircle_application::availability_outcomes::{FetchTargetState, TargetFingerprint};
use harvestcircle_application::{
    AvailabilityDiscoveryOutcome, AvailabilityDiscoveryProgress, AvailabilityDiscoveryRequest,
    AvailabilityDiscoveryState, AvailabilityDiscoveryStopReason,
    AvailabilityDiscoveryTargetOutcome, AvailabilityDiscoveryUsage, AvailabilityLocalQueryScope,
    DISCOVERY_FETCH_RAW_RESERVATION_BYTES, MAX_DISCOVERY_FETCH_CALLS, MAX_DISCOVERY_METADATA_BYTES,
    MAX_DISCOVERY_RETURNED_EVENTS, MAX_DISCOVERY_RETURNED_PER_TARGET, MAX_DISCOVERY_TARGETS,
    RequestId, SessionGeneration,
};
use harvestcircle_domain::error::AvailabilityFailure;
use harvestcircle_domain::{
    AvailabilityQueryContext, AvailabilityQueryError, PublicKey, SafeError, SafeErrorCode,
};
use radroots_transport::outcome::FetchTargetOutcome;

const AUTHOR_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

fn scope() -> AvailabilityLocalQueryScope {
    AvailabilityLocalQueryScope::new(
        PublicKey::from_hex(AUTHOR_HEX).expect("fixed public curve author"),
        AvailabilityQueryContext::new([1; 32], [2; 32], u64::MAX, u64::MAX)
            .expect("nonzero structural context and store"),
        SessionGeneration::from_value(u64::MAX),
    )
}

fn fingerprint(index: u8) -> TargetFingerprint {
    TargetFingerprint::parse(format!("{index:064x}")).expect("fixed public target fingerprint")
}

fn request(targets: Vec<TargetFingerprint>) -> AvailabilityDiscoveryRequest {
    AvailabilityDiscoveryRequest::new(
        RequestId::new(7).expect("nonzero request"),
        scope(),
        targets,
        30_000,
    )
    .expect("bounded unique structural request")
}

fn progress(
    state: FetchTargetState,
    reason: AvailabilityDiscoveryStopReason,
    returned: u16,
) -> AvailabilityDiscoveryProgress {
    AvailabilityDiscoveryProgress::new(
        AvailabilityDiscoveryState::Requested(state),
        reason,
        returned,
    )
    .expect("consistent bounded target progress")
}

fn not_requested() -> AvailabilityDiscoveryProgress {
    AvailabilityDiscoveryProgress::new(
        AvailabilityDiscoveryState::NotRequested,
        AvailabilityDiscoveryStopReason::None,
        0,
    )
    .expect("explicit unrequested stream")
}

fn target(
    id: TargetFingerprint,
    listings: AvailabilityDiscoveryProgress,
    profiles: AvailabilityDiscoveryProgress,
    deletions: AvailabilityDiscoveryProgress,
) -> AvailabilityDiscoveryTargetOutcome {
    AvailabilityDiscoveryTargetOutcome::new(id, listings, profiles, deletions)
        .expect("one shared bounded target allowance")
}

fn unrequested_target(index: u8) -> AvailabilityDiscoveryTargetOutcome {
    target(
        fingerprint(index),
        not_requested(),
        not_requested(),
        not_requested(),
    )
}

fn usage(calls: u8, returned: u16) -> AvailabilityDiscoveryUsage {
    AvailabilityDiscoveryUsage::new(calls, returned).expect("bounded supplied operation accounting")
}

fn assert_code(error: SafeError, code: SafeErrorCode) {
    assert_eq!(error.code(), code);
    let standard_error: &dyn Error = &error;
    assert!(standard_error.source().is_none());
}

#[test]
fn request_admits_empty_and_exact_target_limit() {
    assert_eq!(MAX_DISCOVERY_TARGETS, 16);
    for count in [0, 1, 16] {
        let targets: Vec<_> = (1..=count).map(fingerprint).collect();
        let pointer = targets.as_ptr();
        let expected: Vec<_> = targets
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect();
        let value = request(targets);
        assert_eq!(value.targets().len(), usize::from(count));
        assert_eq!(value.targets().as_ptr(), pointer);
        assert_eq!(
            value
                .targets()
                .iter()
                .map(TargetFingerprint::as_str)
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert_code(
        AvailabilityDiscoveryRequest::new(
            RequestId::new(7).expect("request"),
            scope(),
            (1..=17).map(fingerprint).collect(),
            1,
        )
        .expect_err("seventeen selected targets"),
        SafeErrorCode::AvailabilityCapacity,
    );
}

#[test]
fn request_refuses_duplicates_and_invalid_deadlines() {
    for targets in [
        vec![fingerprint(1), fingerprint(1)],
        vec![
            fingerprint(1),
            fingerprint(2),
            fingerprint(3),
            fingerprint(1),
        ],
    ] {
        assert_code(
            AvailabilityDiscoveryRequest::new(
                RequestId::new(7).expect("request"),
                scope(),
                targets,
                1,
            )
            .expect_err("duplicate canonical target"),
            SafeErrorCode::AvailabilityInvalidInput,
        );
    }
    let uppercase = TargetFingerprint::parse("AB".repeat(32)).expect("shared canonicalization");
    let lowercase = TargetFingerprint::parse("ab".repeat(32)).expect("canonical target");
    assert_eq!(uppercase.as_str(), "ab".repeat(32));
    assert_code(
        AvailabilityDiscoveryRequest::new(
            RequestId::new(7).expect("request"),
            scope(),
            vec![uppercase, lowercase],
            1,
        )
        .expect_err("canonical aliases are duplicate targets"),
        SafeErrorCode::AvailabilityInvalidInput,
    );
    for deadline in [1, 30_000] {
        assert_eq!(
            AvailabilityDiscoveryRequest::new(
                RequestId::new(7).expect("request"),
                scope(),
                vec![],
                deadline
            )
            .expect("exact allowed deadline")
            .deadline_millis(),
            deadline
        );
    }
    for deadline in [0, 30_001, u64::MAX] {
        assert_code(
            AvailabilityDiscoveryRequest::new(
                RequestId::new(7).expect("request"),
                scope(),
                vec![],
                deadline,
            )
            .expect_err("outside absolute deadline range"),
            SafeErrorCode::AvailabilityInvalidInput,
        );
    }
}

#[test]
fn request_preserves_scope_and_request_without_signer() {
    let value = AvailabilityDiscoveryRequest::new(
        RequestId::new(u64::MAX).expect("full-width request"),
        scope(),
        vec![fingerprint(1)],
        30_000,
    )
    .expect("pure structural values require no signing capability");
    assert_eq!(value.request_id().get(), u64::MAX);
    assert_eq!(value.scope().owner().to_hex(), AUTHOR_HEX);
    assert_eq!(value.scope().context().context_id(), &[1; 32]);
    assert_eq!(value.scope().context().store_generation(), &[2; 32]);
    assert_eq!(value.scope().context().source_revision(), u64::MAX);
    assert_eq!(value.scope().context().projection_generation(), u64::MAX);
    assert_eq!(value.scope().session_generation().value(), u64::MAX);
    assert_eq!(value.deadline_millis(), 30_000);
    let value = AvailabilityDiscoveryOutcome::new(
        value,
        usage(0, 0),
        vec![unrequested_target(1)],
        Vec::<u8>::new(),
    )
    .expect("structural unrequested outcome");
    assert_eq!(value.request().request_id().get(), u64::MAX);
    assert_eq!(
        value.request().scope().context().projection_generation(),
        u64::MAX
    );
}

#[test]
fn progress_preserves_every_shared_state_and_empty_distinction() {
    let unrequested = not_requested();
    assert_eq!(
        unrequested.state(),
        AvailabilityDiscoveryState::NotRequested
    );
    assert_eq!(unrequested.reason(), AvailabilityDiscoveryStopReason::None);
    assert_eq!(unrequested.returned(), 0);
    let cases = [
        (
            FetchTargetState::Complete,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::BudgetExhausted,
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::DeadlineExpired,
        ),
        (
            FetchTargetState::Unavailable,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::FailedRetryable,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::FailedTerminal,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::Cancelled,
            AvailabilityDiscoveryStopReason::Stopped,
        ),
    ];
    for (state, reason) in cases {
        let value = progress(state, reason, 0);
        assert_eq!(value.state(), AvailabilityDiscoveryState::Requested(state));
        assert_eq!(value.reason(), reason);
        assert_eq!(value.returned(), 0);
        assert_ne!(value.state(), unrequested.state());
        if state != FetchTargetState::Unavailable {
            for count in [1, 64] {
                assert_eq!(progress(state, reason, count).returned(), count);
            }
        }
    }
    assert_ne!(
        progress(
            FetchTargetState::Complete,
            AvailabilityDiscoveryStopReason::None,
            0
        )
        .state(),
        progress(
            FetchTargetState::Unavailable,
            AvailabilityDiscoveryStopReason::None,
            0
        )
        .state()
    );
}

#[test]
fn progress_rejects_inconsistent_stop_reasons_and_counts() {
    let states = [
        AvailabilityDiscoveryState::NotRequested,
        AvailabilityDiscoveryState::Requested(FetchTargetState::Complete),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Partial),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Unavailable),
        AvailabilityDiscoveryState::Requested(FetchTargetState::FailedRetryable),
        AvailabilityDiscoveryState::Requested(FetchTargetState::FailedTerminal),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Cancelled),
    ];
    for state in states {
        for reason in [
            AvailabilityDiscoveryStopReason::None,
            AvailabilityDiscoveryStopReason::BudgetExhausted,
            AvailabilityDiscoveryStopReason::DeadlineExpired,
            AvailabilityDiscoveryStopReason::Stopped,
        ] {
            for count in [0, 1, 64, 65] {
                let valid = match state {
                    AvailabilityDiscoveryState::NotRequested => {
                        reason == AvailabilityDiscoveryStopReason::None && count == 0
                    }
                    AvailabilityDiscoveryState::Requested(
                        FetchTargetState::Complete
                        | FetchTargetState::FailedRetryable
                        | FetchTargetState::FailedTerminal,
                    ) => reason == AvailabilityDiscoveryStopReason::None,
                    AvailabilityDiscoveryState::Requested(FetchTargetState::Partial) => {
                        reason != AvailabilityDiscoveryStopReason::Stopped
                    }
                    AvailabilityDiscoveryState::Requested(FetchTargetState::Unavailable) => {
                        reason == AvailabilityDiscoveryStopReason::None && count == 0
                    }
                    AvailabilityDiscoveryState::Requested(FetchTargetState::Cancelled) => {
                        reason == AvailabilityDiscoveryStopReason::Stopped
                    }
                };
                let result = AvailabilityDiscoveryProgress::new(state, reason, count);
                if count > 64 {
                    assert_code(
                        result.expect_err("returned cap checked first"),
                        SafeErrorCode::AvailabilityCapacity,
                    );
                } else if valid {
                    let value = result.expect("admitted state/reason/count matrix entry");
                    assert_eq!(
                        (value.state(), value.reason(), value.returned()),
                        (state, reason, count)
                    );
                } else {
                    assert_code(
                        result.expect_err("inconsistent progress facts"),
                        SafeErrorCode::AvailabilityInvalidInput,
                    );
                }
            }
        }
    }
}

#[test]
fn target_streams_remain_independent() {
    let listings = progress(
        FetchTargetState::Complete,
        AvailabilityDiscoveryStopReason::None,
        10,
    );
    let profiles = progress(
        FetchTargetState::Partial,
        AvailabilityDiscoveryStopReason::DeadlineExpired,
        2,
    );
    let deletions = progress(
        FetchTargetState::FailedTerminal,
        AvailabilityDiscoveryStopReason::None,
        1,
    );
    let value = target(fingerprint(1), listings, profiles, deletions);
    assert_eq!(value.target(), &fingerprint(1));
    assert_eq!(value.listings(), listings);
    assert_eq!(value.profiles(), profiles);
    assert_eq!(value.deletions(), deletions);
    assert_eq!(
        value.listings().state(),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Complete)
    );
    assert_eq!(
        value.profiles().reason(),
        AvailabilityDiscoveryStopReason::DeadlineExpired
    );
    assert_eq!(value.deletions().returned(), 1);
}

#[test]
fn per_target_limit_is_shared_across_all_three_streams() {
    assert_eq!(MAX_DISCOVERY_RETURNED_PER_TARGET, 64);
    for counts in [
        [0, 0, 0],
        [1, 0, 0],
        [64, 0, 0],
        [0, 64, 0],
        [0, 0, 64],
        [21, 21, 21],
        [22, 21, 21],
    ] {
        let stages = counts.map(|count| {
            progress(
                FetchTargetState::Partial,
                AvailabilityDiscoveryStopReason::None,
                count,
            )
        });
        let value = target(fingerprint(1), stages[0], stages[1], stages[2]);
        assert_eq!(
            [
                value.listings().returned(),
                value.profiles().returned(),
                value.deletions().returned()
            ],
            counts
        );
    }
    for counts in [
        [23, 21, 21],
        [64, 1, 0],
        [0, 64, 1],
        [1, 0, 64],
        [64, 64, 64],
    ] {
        let stages = counts.map(|count| {
            progress(
                FetchTargetState::Partial,
                AvailabilityDiscoveryStopReason::None,
                count,
            )
        });
        assert_code(
            AvailabilityDiscoveryTargetOutcome::new(
                fingerprint(1),
                stages[0],
                stages[1],
                stages[2],
            )
            .expect_err("sum exceeds the one target allowance"),
            SafeErrorCode::AvailabilityCapacity,
        );
    }
}

#[test]
fn usage_enforces_operation_wide_calls_events_and_no_refunds() {
    assert_eq!(MAX_DISCOVERY_FETCH_CALLS, 4);
    assert_eq!(MAX_DISCOVERY_RETURNED_EVENTS, 1024);
    assert_eq!(DISCOVERY_FETCH_RAW_RESERVATION_BYTES, 8_388_608);
    for calls in 0..=4 {
        for returned in [0, 1, 1024] {
            if calls == 0 && returned > 0 {
                assert_code(
                    AvailabilityDiscoveryUsage::new(calls, returned)
                        .expect_err("returns without any call"),
                    SafeErrorCode::AvailabilityInvalidInput,
                );
            } else {
                let value = usage(calls, returned);
                assert_eq!(value.fetch_calls(), calls);
                assert_eq!(value.returned_events(), returned);
                assert_eq!(value.reserved_raw_bytes(), u64::from(calls) * 8_388_608);
            }
        }
    }
    assert_eq!(usage(4, 0).reserved_raw_bytes(), 33_554_432);
    assert_eq!(
        usage(4, 1024).reserved_raw_bytes(),
        usage(4, 0).reserved_raw_bytes()
    );
    for (calls, returned) in [(5, 0), (5, 1024), (4, 1025), (0, 1025), (u8::MAX, u16::MAX)] {
        assert_code(
            AvailabilityDiscoveryUsage::new(calls, returned).expect_err("aggregate accounting cap"),
            SafeErrorCode::AvailabilityCapacity,
        );
    }
}

#[test]
fn outcome_preserves_items_after_partial_capped_failed_and_cancelled_targets() {
    for (state, reason) in [
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::BudgetExhausted,
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::DeadlineExpired,
        ),
        (
            FetchTargetState::FailedRetryable,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::FailedTerminal,
            AvailabilityDiscoveryStopReason::None,
        ),
        (
            FetchTargetState::Cancelled,
            AvailabilityDiscoveryStopReason::Stopped,
        ),
    ] {
        let value = AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1)]),
            usage(1, 3),
            vec![target(
                fingerprint(1),
                progress(state, reason, 3),
                not_requested(),
                not_requested(),
            )],
            vec![11_u8, 22, 33],
        )
        .expect("valid earlier results survive non-complete target states");
        assert_eq!(value.items(), &[11, 22, 33]);
        assert_eq!(value.usage().returned_events(), 3);
        assert_eq!(
            value.listings_state(),
            AvailabilityDiscoveryState::Requested(state)
        );
        assert_eq!(value.targets()[0].listings().reason(), reason);
        assert_eq!(value.into_items(), vec![11, 22, 33]);
    }
}

#[test]
fn outcome_refuses_missing_extra_and_duplicate_targets() {
    for supplied in [
        vec![],
        vec![unrequested_target(2)],
        vec![unrequested_target(1), unrequested_target(2)],
    ] {
        assert_code(
            AvailabilityDiscoveryOutcome::new(
                request(vec![fingerprint(1)]),
                usage(0, 0),
                supplied,
                Vec::<u8>::new(),
            )
            .expect_err("missing or unrequested target fingerprint"),
            SafeErrorCode::AvailabilityScopeMismatch,
        );
    }
    assert_code(
        AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1), fingerprint(2)]),
            usage(0, 0),
            vec![unrequested_target(1), unrequested_target(1)],
            Vec::<u8>::new(),
        )
        .expect_err("duplicate supplied outcome"),
        SafeErrorCode::AvailabilityInvalidInput,
    );
    assert_code(
        AvailabilityDiscoveryOutcome::new(
            request((1..=16).map(fingerprint).collect()),
            usage(0, 0),
            (1..=17).map(unrequested_target).collect(),
            Vec::<u8>::new(),
        )
        .expect_err("target cap before set validation"),
        SafeErrorCode::AvailabilityCapacity,
    );
    let value = AvailabilityDiscoveryOutcome::new(
        request(vec![fingerprint(1), fingerprint(2), fingerprint(3)]),
        usage(0, 0),
        vec![
            unrequested_target(3),
            unrequested_target(1),
            unrequested_target(2),
        ],
        Vec::<u8>::new(),
    )
    .expect("permuted exact fingerprint set");
    assert_eq!(
        value
            .targets()
            .iter()
            .map(|value| value.target().as_str())
            .collect::<Vec<_>>(),
        vec![
            format!("{:064x}", 3),
            format!("{:064x}", 1),
            format!("{:064x}", 2)
        ]
    );
}

#[test]
fn outcome_binds_returned_count_and_valid_item_count() {
    for retained in [0, 1, 3] {
        let value = AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1)]),
            usage(1, 3),
            vec![target(
                fingerprint(1),
                progress(
                    FetchTargetState::Partial,
                    AvailabilityDiscoveryStopReason::None,
                    3,
                ),
                not_requested(),
                not_requested(),
            )],
            vec![9_u8; retained],
        )
        .expect("valid items may be fewer than returned candidates");
        assert_eq!(value.items().len(), retained);
    }
    for returned in [0, 2, 4] {
        assert_code(
            AvailabilityDiscoveryOutcome::new(
                request(vec![fingerprint(1)]),
                usage(1, returned),
                vec![target(
                    fingerprint(1),
                    progress(
                        FetchTargetState::Partial,
                        AvailabilityDiscoveryStopReason::None,
                        3,
                    ),
                    not_requested(),
                    not_requested(),
                )],
                Vec::<u8>::new(),
            )
            .expect_err("exact aggregate returned count"),
            SafeErrorCode::AvailabilityInvalidInput,
        );
    }
    assert_code(
        AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1)]),
            usage(1, 3),
            vec![target(
                fingerprint(1),
                progress(
                    FetchTargetState::Partial,
                    AvailabilityDiscoveryStopReason::None,
                    3,
                ),
                not_requested(),
                not_requested(),
            )],
            vec![9_u8; 4],
        )
        .expect_err("valid items cannot exceed returned candidates"),
        SafeErrorCode::AvailabilityInvalidInput,
    );
    assert_code(
        AvailabilityDiscoveryOutcome::new(request(vec![]), usage(0, 0), vec![], vec![9_u8; 1025])
            .expect_err("item cap before empty-source/count validation"),
        SafeErrorCode::AvailabilityCapacity,
    );
    assert_code(
        AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1)]),
            usage(0, 0),
            vec![target(
                fingerprint(1),
                progress(
                    FetchTargetState::Complete,
                    AvailabilityDiscoveryStopReason::None,
                    0,
                ),
                not_requested(),
                not_requested(),
            )],
            Vec::<u8>::new(),
        )
        .expect_err("completion requires a supplied fetch call"),
        SafeErrorCode::AvailabilityInvalidInput,
    );
}

#[test]
fn empty_selection_never_fabricates_complete_or_network_work() {
    let value =
        AvailabilityDiscoveryOutcome::new(request(vec![]), usage(0, 0), vec![], Vec::<u8>::new())
            .expect("explicit no-source selection");
    assert_eq!(
        value.listings_state(),
        AvailabilityDiscoveryState::NotRequested
    );
    assert_eq!(
        value.profiles_state(),
        AvailabilityDiscoveryState::NotRequested
    );
    assert_eq!(
        value.deletions_state(),
        AvailabilityDiscoveryState::NotRequested
    );
    assert_eq!(value.usage().reserved_raw_bytes(), 0);
    assert!(value.targets().is_empty());
    assert!(value.items().is_empty());
    for (calls, returned, items) in [
        (1, 0, vec![]),
        (1, 1, vec![]),
        (1, 1, vec![1_u8]),
        (0, 0, vec![1_u8]),
    ] {
        assert_code(
            AvailabilityDiscoveryOutcome::new(
                request(vec![]),
                usage(calls, returned),
                vec![],
                items,
            )
            .expect_err("empty selection cannot fabricate work or items"),
            SafeErrorCode::AvailabilityInvalidInput,
        );
    }
    assert_code(
        AvailabilityDiscoveryOutcome::new(
            request(vec![]),
            usage(0, 0),
            vec![unrequested_target(1)],
            Vec::<u8>::new(),
        )
        .expect_err("no target may be fabricated"),
        SafeErrorCode::AvailabilityScopeMismatch,
    );
}

#[test]
fn stream_completeness_requires_every_selected_target() {
    let states = [
        AvailabilityDiscoveryState::NotRequested,
        AvailabilityDiscoveryState::Requested(FetchTargetState::Complete),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Partial),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Unavailable),
        AvailabilityDiscoveryState::Requested(FetchTargetState::FailedRetryable),
        AvailabilityDiscoveryState::Requested(FetchTargetState::FailedTerminal),
        AvailabilityDiscoveryState::Requested(FetchTargetState::Cancelled),
    ];
    for first in states {
        for second in states {
            let stage = |state| {
                AvailabilityDiscoveryProgress::new(
                    state,
                    if state == AvailabilityDiscoveryState::Requested(FetchTargetState::Cancelled) {
                        AvailabilityDiscoveryStopReason::Stopped
                    } else {
                        AvailabilityDiscoveryStopReason::None
                    },
                    0,
                )
                .expect("zero-count stream state")
            };
            let value = AvailabilityDiscoveryOutcome::new(
                request(vec![fingerprint(1), fingerprint(2)]),
                usage(1, 0),
                vec![
                    target(fingerprint(1), stage(first), stage(first), stage(first)),
                    target(fingerprint(2), stage(second), stage(second), stage(second)),
                ],
                Vec::<u8>::new(),
            )
            .expect("exact selected stream evidence");
            let expected = if first == second {
                first
            } else {
                AvailabilityDiscoveryState::Requested(FetchTargetState::Partial)
            };
            assert_eq!(value.listings_state(), expected);
            assert_eq!(value.profiles_state(), expected);
            assert_eq!(value.deletions_state(), expected);
        }
    }
}

#[test]
fn one_stream_failure_never_erases_another_stream_completion() {
    for interrupted in [
        FetchTargetState::Partial,
        FetchTargetState::Unavailable,
        FetchTargetState::FailedRetryable,
        FetchTargetState::FailedTerminal,
        FetchTargetState::Cancelled,
    ] {
        let reason = if interrupted == FetchTargetState::Cancelled {
            AvailabilityDiscoveryStopReason::Stopped
        } else {
            AvailabilityDiscoveryStopReason::None
        };
        for stream in 0..3 {
            let mut stages = [progress(
                FetchTargetState::Complete,
                AvailabilityDiscoveryStopReason::None,
                0,
            ); 3];
            stages[stream] = progress(interrupted, reason, 0);
            let value = AvailabilityDiscoveryOutcome::new(
                request(vec![fingerprint(1)]),
                usage(1, 0),
                vec![target(fingerprint(1), stages[0], stages[1], stages[2])],
                Vec::<u8>::new(),
            )
            .expect("independent stream statuses");
            for (index, observed) in [
                value.listings_state(),
                value.profiles_state(),
                value.deletions_state(),
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(
                    observed,
                    AvailabilityDiscoveryState::Requested(if index == stream {
                        interrupted
                    } else {
                        FetchTargetState::Complete
                    })
                );
            }
        }
    }
}

#[test]
fn outcome_retains_owned_nonclone_items_without_copy() {
    struct PrivateItem {
        marker: String,
    }
    let items = vec![PrivateItem {
        marker: "HCAV_PRIVATE_NONDEBUG_ITEM".to_owned(),
    }];
    let pointer = items.as_ptr();
    let capacity = items.capacity();
    let targets = vec![target(
        fingerprint(1),
        progress(
            FetchTargetState::FailedRetryable,
            AvailabilityDiscoveryStopReason::None,
            1,
        ),
        not_requested(),
        not_requested(),
    )];
    let targets_pointer = targets.as_ptr();
    let value = AvailabilityDiscoveryOutcome::new(
        request(vec![fingerprint(1)]),
        usage(1, 1),
        targets,
        items,
    )
    .expect("no item Clone/Debug requirement");
    assert_eq!(value.items().as_ptr(), pointer);
    assert_eq!(value.targets().as_ptr(), targets_pointer);
    assert_eq!(value.items()[0].marker, "HCAV_PRIVATE_NONDEBUG_ITEM");
    assert!(!format!("{value:?}").contains("HCAV_PRIVATE_NONDEBUG_ITEM"));
    assert!(!value.metadata_json().contains("HCAV_PRIVATE_NONDEBUG_ITEM"));
    let returned = value.into_items();
    assert_eq!(returned.as_ptr(), pointer);
    assert_eq!(returned.capacity(), capacity);
    assert_eq!(returned[0].marker, "HCAV_PRIVATE_NONDEBUG_ITEM");
}

#[test]
fn metadata_serialization_has_exact_safe_schema() {
    let value = AvailabilityDiscoveryOutcome::new(
        request(vec![fingerprint(1)]),
        usage(1, 3),
        vec![target(
            fingerprint(1),
            progress(
                FetchTargetState::Complete,
                AvailabilityDiscoveryStopReason::None,
                1,
            ),
            progress(
                FetchTargetState::Partial,
                AvailabilityDiscoveryStopReason::BudgetExhausted,
                2,
            ),
            not_requested(),
        )],
        vec!["HCAV_PRIVATE_ITEM_PAYLOAD"],
    )
    .expect("independently described metadata fixture");
    let expected = concat!(
        "{\"version\":1,\"fetch_calls\":1,\"reserved_raw_bytes\":8388608,\"returned_events\":3,\"retained_items\":1,",
        "\"listings\":\"complete\",\"profiles\":\"partial\",\"deletions\":\"not_requested\",\"targets\":[",
        "{\"target\":\"0000000000000000000000000000000000000000000000000000000000000001\",",
        "\"listings\":{\"state\":\"complete\",\"reason\":\"none\",\"returned\":1},",
        "\"profiles\":{\"state\":\"partial\",\"reason\":\"budget_exhausted\",\"returned\":2},",
        "\"deletions\":{\"state\":\"not_requested\",\"reason\":\"none\",\"returned\":0}}]}"
    );
    assert_eq!(value.metadata_json(), expected);
    assert_eq!(value.metadata_json(), value.metadata_json());
    let empty =
        AvailabilityDiscoveryOutcome::new(request(vec![]), usage(0, 0), vec![], Vec::<u8>::new())
            .expect("empty metadata");
    assert_eq!(
        empty.metadata_json(),
        "{\"version\":1,\"fetch_calls\":0,\"reserved_raw_bytes\":0,\"returned_events\":0,\"retained_items\":0,\"listings\":\"not_requested\",\"profiles\":\"not_requested\",\"deletions\":\"not_requested\",\"targets\":[]}"
    );
}

#[test]
fn metadata_serialization_is_bounded_at_sixteen_targets() {
    assert_eq!(MAX_DISCOVERY_METADATA_BYTES, 16_384);
    let targets: Vec<_> = (1..=16)
        .rev()
        .map(|index| {
            target(
                fingerprint(index),
                progress(
                    FetchTargetState::Complete,
                    AvailabilityDiscoveryStopReason::None,
                    32,
                ),
                progress(
                    FetchTargetState::Partial,
                    AvailabilityDiscoveryStopReason::DeadlineExpired,
                    16,
                ),
                progress(
                    FetchTargetState::FailedRetryable,
                    AvailabilityDiscoveryStopReason::None,
                    16,
                ),
            )
        })
        .collect();
    let value = AvailabilityDiscoveryOutcome::new(
        request((1..=16).map(fingerprint).collect()),
        usage(4, 1024),
        targets,
        vec!["HCAV_PRIVATE_LARGE_RESULT"; 1024],
    )
    .expect("exact full target/result/accounting bounds");
    let json = value.metadata_json();
    assert!(json.is_ascii());
    assert!(json.len() <= 16_384);
    assert_eq!(json.matches("\"target\":").count(), 16);
    assert_eq!(json.matches("\"returned\":32").count(), 16);
    assert_eq!(json.matches("\"returned\":16").count(), 32);
    assert!(json.starts_with("{\"version\":1,\"fetch_calls\":4,\"reserved_raw_bytes\":33554432,\"returned_events\":1024,\"retained_items\":1024,"));
    let first = format!("\"target\":\"{:064x}\"", 16);
    let last = format!("\"target\":\"{:064x}\"", 1);
    assert!(
        json.find(&first).expect("supplied first target")
            < json.find(&last).expect("supplied last target")
    );
    assert!(!json.contains("HCAV_PRIVATE_LARGE_RESULT"));
    for (state, reason, name, reason_name) in [
        (
            FetchTargetState::Complete,
            AvailabilityDiscoveryStopReason::None,
            "complete",
            "none",
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::BudgetExhausted,
            "partial",
            "budget_exhausted",
        ),
        (
            FetchTargetState::Partial,
            AvailabilityDiscoveryStopReason::DeadlineExpired,
            "partial",
            "deadline_expired",
        ),
        (
            FetchTargetState::Unavailable,
            AvailabilityDiscoveryStopReason::None,
            "unavailable",
            "none",
        ),
        (
            FetchTargetState::FailedRetryable,
            AvailabilityDiscoveryStopReason::None,
            "failed_retryable",
            "none",
        ),
        (
            FetchTargetState::FailedTerminal,
            AvailabilityDiscoveryStopReason::None,
            "failed_terminal",
            "none",
        ),
        (
            FetchTargetState::Cancelled,
            AvailabilityDiscoveryStopReason::Stopped,
            "cancelled",
            "stopped",
        ),
    ] {
        let value = AvailabilityDiscoveryOutcome::new(
            request(vec![fingerprint(1)]),
            usage(1, 0),
            vec![target(
                fingerprint(1),
                progress(state, reason, 0),
                not_requested(),
                not_requested(),
            )],
            Vec::<u8>::new(),
        )
        .expect("every exact metadata state spelling");
        assert!(value.metadata_json().contains(&format!(
            "\"listings\":{{\"state\":\"{name}\",\"reason\":\"{reason_name}\",\"returned\":0}}"
        )));
    }
}

#[test]
fn diagnostics_omit_private_payloads_and_remote_messages() {
    struct PrivateItem(&'static str);
    const PRIVATE_MARKER: &str = "HCAV_PRIVATE_PAYLOAD_MARKER";
    const REMOTE_MARKER: &str = "HCAV_HOSTILE_REMOTE_MESSAGE_\"\n\0🥕";
    let shared = FetchTargetOutcome::new(fingerprint(0xab), FetchTargetState::FailedRetryable)
        .with_message(REMOTE_MARKER);
    assert_eq!(shared.message(), Some(REMOTE_MARKER));
    let stage = progress(shared.state(), AvailabilityDiscoveryStopReason::None, 1);
    let target_value = target(
        shared.target().clone(),
        stage,
        not_requested(),
        not_requested(),
    );
    let target_debug = format!("{target_value:?}");
    let requested = request(vec![shared.target().clone()]);
    let request_debug = format!("{requested:?}");
    let value = AvailabilityDiscoveryOutcome::new(
        requested,
        usage(1, 1),
        vec![target_value],
        vec![PrivateItem(PRIVATE_MARKER)],
    )
    .expect("retains only selected shared identity/state facts");
    assert_eq!(value.items()[0].0, PRIVATE_MARKER);
    let context_bytes = format!("{:?}", [1_u8; 32]);
    let store_bytes = format!("{:?}", [2_u8; 32]);
    for diagnostic in [
        request_debug,
        target_debug,
        format!("{value:?}"),
        format!("{stage:?}"),
    ] {
        for forbidden in [
            PRIVATE_MARKER,
            "HCAV_HOSTILE_REMOTE_MESSAGE_",
            AUTHOR_HEX,
            context_bytes.as_str(),
            store_bytes.as_str(),
            shared.target().as_str(),
            "RequestId",
            "request_id",
        ] {
            assert!(!diagnostic.contains(forbidden));
        }
    }
    let json = value.metadata_json();
    assert!(
        json.contains(shared.target().as_str()),
        "metadata admits only the bounded canonical public target identity"
    );
    for forbidden in [
        PRIVATE_MARKER,
        "HCAV_HOSTILE_REMOTE_MESSAGE_",
        AUTHOR_HEX,
        "request_id",
        "context_id",
        "store_generation",
        "scope",
        "message",
        "endpoint",
        "\"events\":",
        "payload",
    ] {
        assert!(!json.contains(forbidden));
    }
}

#[test]
fn query_errors_map_to_static_distinct_availability_errors() {
    for (source, expected_code, expected_message) in [
        (
            AvailabilityQueryError::InvalidInput,
            SafeErrorCode::AvailabilityInvalidInput,
            "The availability request is invalid.",
        ),
        (
            AvailabilityQueryError::InputTooLarge,
            SafeErrorCode::AvailabilityInvalidInput,
            "The availability request is invalid.",
        ),
        (
            AvailabilityQueryError::ScopeMismatch,
            SafeErrorCode::AvailabilityScopeMismatch,
            "The availability request belongs to another scope.",
        ),
        (
            AvailabilityQueryError::StaleQuery,
            SafeErrorCode::AvailabilityStaleQuery,
            "The availability query is stale.",
        ),
        (
            AvailabilityQueryError::Capacity,
            SafeErrorCode::AvailabilityCapacity,
            "The availability operation reached its capacity.",
        ),
    ] {
        let value = SafeError::from(source);
        assert_eq!(value.code(), expected_code);
        assert_eq!(value.message().as_str(), expected_message);
        assert_eq!(value.to_string(), expected_message);
    }
}

#[test]
fn availability_failures_have_fixed_public_messages() {
    for (source, expected_code, expected_message) in [
        (
            AvailabilityFailure::InvalidInput,
            SafeErrorCode::AvailabilityInvalidInput,
            "The availability request is invalid.",
        ),
        (
            AvailabilityFailure::UnsupportedProfile,
            SafeErrorCode::AvailabilityUnsupportedProfile,
            "This availability profile is unsupported.",
        ),
        (
            AvailabilityFailure::ScopeMismatch,
            SafeErrorCode::AvailabilityScopeMismatch,
            "The availability request belongs to another scope.",
        ),
        (
            AvailabilityFailure::StaleQuery,
            SafeErrorCode::AvailabilityStaleQuery,
            "The availability query is stale.",
        ),
        (
            AvailabilityFailure::Capacity,
            SafeErrorCode::AvailabilityCapacity,
            "The availability operation reached its capacity.",
        ),
        (
            AvailabilityFailure::Unavailable,
            SafeErrorCode::AvailabilityUnavailable,
            "Availability discovery is unavailable.",
        ),
    ] {
        let value = SafeError::from(source);
        assert_eq!(value.code(), expected_code);
        assert_eq!(value.message().as_str(), expected_message);
        assert_eq!(value.to_string(), expected_message);
        assert!(!format!("{value:?}").contains("HCAV_HOSTILE_REMOTE_MESSAGE"));
        let copied = value;
        assert_eq!(copied, value);
        let standard_error: &dyn Error = &value;
        assert!(standard_error.source().is_none());
    }
}
