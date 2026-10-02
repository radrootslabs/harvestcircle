use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harvestcircle_application::availability_outcomes::{FetchTargetState, TargetFingerprint};
use harvestcircle_application::{
    ActorMailbox, AvailabilityDiscoveryOutcome, AvailabilityDiscoveryProgress,
    AvailabilityDiscoveryRequest, AvailabilityDiscoveryState, AvailabilityDiscoveryStopReason,
    AvailabilityDiscoveryTargetOutcome, AvailabilityDiscoveryUsage, AvailabilityLocalAdmission,
    AvailabilityLocalQueryScope, AvailabilityLocalReadPort, AvailabilityMonotonicClock,
    AvailabilityQueryService, AvailabilityRefreshPort, BoxFuture, CommandContext, CommandEnvelope,
    CommandReceipt, CommandRejection, CommandResult, CommandSubmission, RequestId,
    ScopedAvailabilityQuery, SessionGeneration, SnapshotRevision,
};
use harvestcircle_domain::availability::query::EventTimestamp;
use harvestcircle_domain::error::AvailabilityFailure;
use harvestcircle_domain::{
    AvailabilityEventVersion, AvailabilityHeadState, AvailabilityHeadView,
    AvailabilityListingCoordinate, AvailabilityObservation, AvailabilityOrderKey, AvailabilityPage,
    AvailabilityPageContinuation, AvailabilityPageCursor, AvailabilityPageLimit,
    AvailabilityQueryContext, AvailabilityQueryError, AvailabilityQueryFilters,
    AvailabilityVersionView, PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp,
};
use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::envelope::event_head::{
    EventHeadCandidateResult, EventHeadDecision, event_head_candidate_for_nip01_event,
    select_event_head,
};
use radroots_event::wire::{DEFAULT_TAG_ELEMENT_MAX_BYTES, Nip01EventWire};
use radroots_event_codec::admission::deletion::verify_and_admit_nip09_deletion_request_event;
use radroots_event_codec::verify::{RadrootsSignatureVerifiedEvent, verify_nip01_event};
use tokio::sync::{mpsc, oneshot};

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const OTHER_AUTHOR: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
const MAX_PUBLIC_EVENTS: usize = 6;
const MAX_RECIPE_BYTES: usize = 8 * 1024;
const MAX_PUBLIC_EVENT_BYTES: usize = 16 * 1024;
const MAX_PUBLIC_FIXTURE_BYTES: usize = 96 * 1024;

type Discovery = AvailabilityDiscoveryOutcome<AvailabilityEventVersion>;
type Envelope = CommandEnvelope<AvailabilityDiscoveryRequest, Discovery>;

fn scope_with(
    owner: &str,
    context: u8,
    store: u8,
    source: u64,
    projection: u64,
    session: u64,
) -> AvailabilityLocalQueryScope {
    AvailabilityLocalQueryScope::new(
        PublicKey::from_hex(owner).expect("fixed valid public curve author"),
        AvailabilityQueryContext::new([context; 32], [store; 32], source, projection)
            .expect("nonzero structural identities"),
        SessionGeneration::from_value(session),
    )
}

fn scope() -> AvailabilityLocalQueryScope {
    scope_with(AUTHOR, 1, 2, 5, 7, 3)
}

fn changes() -> Vec<(AvailabilityLocalQueryScope, SafeErrorCode)> {
    vec![
        (
            scope_with(OTHER_AUTHOR, 1, 2, 5, 7, 3),
            SafeErrorCode::AvailabilityScopeMismatch,
        ),
        (
            scope_with(AUTHOR, 9, 2, 5, 7, 3),
            SafeErrorCode::AvailabilityScopeMismatch,
        ),
        (
            scope_with(AUTHOR, 1, 9, 5, 7, 3),
            SafeErrorCode::AvailabilityStaleQuery,
        ),
        (
            scope_with(AUTHOR, 1, 2, 6, 7, 3),
            SafeErrorCode::AvailabilityStaleQuery,
        ),
        (
            scope_with(AUTHOR, 1, 2, 5, 8, 3),
            SafeErrorCode::AvailabilityStaleQuery,
        ),
        (
            scope_with(AUTHOR, 1, 2, 5, 7, 4),
            SafeErrorCode::AvailabilityStaleQuery,
        ),
    ]
}

fn coordinate(identifier: &str) -> AvailabilityListingCoordinate {
    AvailabilityListingCoordinate::parse(&format!("30402:{AUTHOR}:{identifier}"))
        .expect("bounded exact public coordinate")
}

fn version(index: u8) -> AvailabilityEventVersion {
    AvailabilityEventVersion::from_hex(&format!("{index:064x}"))
        .expect("exact public event-ID reference")
}

fn query(rows: u16) -> ScopedAvailabilityQuery {
    ScopedAvailabilityQuery::new(
        scope(),
        AvailabilityQueryFilters::new(None, None, None),
        AvailabilityPageLimit::new(rows).expect("bounded request limit"),
        None,
    )
    .expect("pure structural query")
}

fn page(
    rows: Vec<AvailabilityHeadView>,
    continuation: AvailabilityPageContinuation,
    generation: u64,
) -> AvailabilityPage<AvailabilityHeadView> {
    AvailabilityPage::new(
        AvailabilityPageLimit::new(100).expect("adapter's broad admitted bound"),
        rows,
        continuation,
        generation,
    )
    .expect("bounded adapter page")
}

fn request_at(
    id: u64,
    binding: AvailabilityLocalQueryScope,
    targets: &[u8],
    deadline_millis: u64,
) -> AvailabilityDiscoveryRequest {
    AvailabilityDiscoveryRequest::new(
        RequestId::new(id).expect("nonzero existing command identity"),
        binding,
        targets
            .iter()
            .map(|index| TargetFingerprint::parse(format!("{index:064x}")).expect("public target"))
            .collect(),
        deadline_millis,
    )
    .expect("bounded structural refresh request")
}

fn request(id: u64) -> AvailabilityDiscoveryRequest {
    request_at(id, scope(), &[1], 1000)
}

fn not_requested() -> AvailabilityDiscoveryProgress {
    AvailabilityDiscoveryProgress::new(
        AvailabilityDiscoveryState::NotRequested,
        AvailabilityDiscoveryStopReason::None,
        0,
    )
    .expect("explicit unrequested stream")
}

fn outcome(request: AvailabilityDiscoveryRequest) -> Discovery {
    let targets = request
        .targets()
        .iter()
        .cloned()
        .map(|target| {
            AvailabilityDiscoveryTargetOutcome::new(
                target,
                not_requested(),
                not_requested(),
                not_requested(),
            )
            .expect("unrequested selected target")
        })
        .collect();
    AvailabilityDiscoveryOutcome::new(
        request,
        AvailabilityDiscoveryUsage::new(0, 0).expect("no fabricated work"),
        targets,
        vec![],
    )
    .expect("valid independent structural completion")
}

fn failure<T>(result: Result<T, SafeError>) -> SafeError {
    match result {
        Ok(_) => panic!("operation must fail closed"),
        Err(error) => error,
    }
}

fn assert_code(error: SafeError, code: SafeErrorCode) {
    assert_eq!(error.code(), code);
    assert!(std::error::Error::source(&error).is_none());
}

fn assert_failed(receipt: CommandReceipt<Discovery>, id: u64, code: SafeErrorCode) {
    assert_eq!(receipt.request_id().get(), id);
    match receipt.into_result() {
        CommandResult::Failed(error) => assert_code(error, code),
        _ => panic!("fixed typed failure under original request ID"),
    }
}

async fn pending<F: Future + ?Sized>(future: Pin<&mut F>) {
    tokio::select! {
        biased;
        value = future => { drop(value); panic!("controlled barrier must remain pending"); }
        () = std::future::ready(()) => {}
    }
}

async fn ready<F: Future>(future: F) -> F::Output {
    tokio::pin!(future);
    tokio::select! {
        biased;
        value = &mut future => value,
        () = std::future::ready(()) => panic!("submission must return without waiting for completion"),
    }
}

struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

fn arm(slot: &Mutex<Option<Gate>>) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (entered, seen) = oneshot::channel();
    let (release, wait) = oneshot::channel();
    assert!(
        slot.lock()
            .expect("fixture gate")
            .replace(Gate {
                entered,
                release: wait
            })
            .is_none()
    );
    (seen, release)
}

async fn pass(slot: &Mutex<Option<Gate>>) {
    let gate = slot.lock().expect("fixture gate").take();
    if let Some(gate) = gate {
        gate.entered.send(()).expect("controlled barrier observer");
        gate.release.await.expect("controlled barrier release");
    }
}

struct Admission {
    current: Mutex<Result<AvailabilityLocalQueryScope, SafeError>>,
    calls: AtomicUsize,
    gate: Mutex<Option<Gate>>,
}

impl Admission {
    fn set(&self, value: Result<AvailabilityLocalQueryScope, SafeError>) {
        *self.current.lock().expect("trusted fixture scope") = value;
    }
}

impl AvailabilityLocalAdmission for Admission {
    fn current_scope<'a>(
        &'a self,
    ) -> BoxFuture<'a, Result<AvailabilityLocalQueryScope, SafeError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            pass(&self.gate).await;
            *self.current.lock().expect("trusted fixture scope")
        })
    }
}

#[derive(Default)]
struct Reader {
    pages: Mutex<VecDeque<Result<AvailabilityPage<AvailabilityHeadView>, SafeError>>>,
    heads: Mutex<VecDeque<Result<AvailabilityHeadView, SafeError>>>,
    versions: Mutex<VecDeque<Result<Option<AvailabilityVersionView>, SafeError>>>,
    page_calls: AtomicUsize,
    head_calls: AtomicUsize,
    version_calls: AtomicUsize,
    queries: Mutex<Vec<ScopedAvailabilityQuery>>,
    head_requests: Mutex<Vec<(AvailabilityLocalQueryScope, AvailabilityListingCoordinate)>>,
    version_requests: Mutex<
        Vec<(
            AvailabilityLocalQueryScope,
            AvailabilityListingCoordinate,
            AvailabilityEventVersion,
        )>,
    >,
    gate: Mutex<Option<Gate>>,
}

impl Reader {
    fn calls(&self) -> usize {
        self.page_calls.load(Ordering::SeqCst)
            + self.head_calls.load(Ordering::SeqCst)
            + self.version_calls.load(Ordering::SeqCst)
    }
}

impl AvailabilityLocalReadPort for Reader {
    fn read_page<'a>(
        &'a self,
        query: &'a ScopedAvailabilityQuery,
    ) -> BoxFuture<'a, Result<AvailabilityPage<AvailabilityHeadView>, SafeError>> {
        Box::pin(async move {
            self.page_calls.fetch_add(1, Ordering::SeqCst);
            self.queries
                .lock()
                .expect("captured request")
                .push(query.clone());
            let result = self
                .pages
                .lock()
                .expect("fixture pages")
                .pop_front()
                .expect("allocated page response");
            pass(&self.gate).await;
            result
        })
    }

    fn read_head<'a>(
        &'a self,
        scope: &'a AvailabilityLocalQueryScope,
        coordinate: &'a AvailabilityListingCoordinate,
    ) -> BoxFuture<'a, Result<AvailabilityHeadView, SafeError>> {
        Box::pin(async move {
            self.head_calls.fetch_add(1, Ordering::SeqCst);
            self.head_requests
                .lock()
                .expect("captured request")
                .push((*scope, coordinate.clone()));
            let result = self
                .heads
                .lock()
                .expect("fixture heads")
                .pop_front()
                .expect("allocated head response");
            pass(&self.gate).await;
            result
        })
    }

    fn read_version<'a>(
        &'a self,
        scope: &'a AvailabilityLocalQueryScope,
        coordinate: &'a AvailabilityListingCoordinate,
        version: AvailabilityEventVersion,
    ) -> BoxFuture<'a, Result<Option<AvailabilityVersionView>, SafeError>> {
        Box::pin(async move {
            self.version_calls.fetch_add(1, Ordering::SeqCst);
            self.version_requests
                .lock()
                .expect("captured request")
                .push((*scope, coordinate.clone(), version));
            let result = self
                .versions
                .lock()
                .expect("fixture versions")
                .pop_front()
                .expect("allocated exact response");
            pass(&self.gate).await;
            result
        })
    }
}

enum SubmitMode {
    Queue,
    ForeignAccepted(RequestId),
    Receipt(Box<CommandReceipt<Discovery>>),
}

struct Refresh {
    mailbox: ActorMailbox<AvailabilityDiscoveryRequest, Discovery>,
    calls: AtomicUsize,
    seen: Mutex<Vec<(CommandContext, AvailabilityDiscoveryRequest)>>,
    mode: Mutex<Option<SubmitMode>>,
}

impl AvailabilityRefreshPort for Refresh {
    fn submit(
        &self,
        context: CommandContext,
        request: AvailabilityDiscoveryRequest,
    ) -> CommandSubmission<Discovery> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen
            .lock()
            .expect("captured submission")
            .push((context, request.clone()));
        let mode = self
            .mode
            .lock()
            .expect("fixture mode")
            .take()
            .unwrap_or(SubmitMode::Queue);
        match mode {
            SubmitMode::Queue => self.mailbox.submit(context, request),
            SubmitMode::ForeignAccepted(id) => self.mailbox.submit(
                CommandContext::new(id, context.expected_revision(), context.deadline()),
                request,
            ),
            SubmitMode::Receipt(receipt) => CommandSubmission::Rejected(*receipt),
        }
    }
}

struct FixtureClock(Mutex<Instant>);

impl FixtureClock {
    fn advance(&self, duration: Duration) {
        let mut now = self.0.lock().expect("per-instance monotonic fixture clock");
        *now += duration;
    }
}

impl AvailabilityMonotonicClock for FixtureClock {
    fn now(&self) -> Instant {
        *self.0.lock().expect("per-instance monotonic fixture clock")
    }
}

struct Harness {
    admission: Arc<Admission>,
    reader: Arc<Reader>,
    refresh: Arc<Refresh>,
    clock: Arc<FixtureClock>,
    receiver: mpsc::Receiver<Envelope>,
    service: AvailabilityQueryService,
}

impl Harness {
    fn new() -> Self {
        let admission = Arc::new(Admission {
            current: Mutex::new(Ok(scope())),
            calls: AtomicUsize::new(0),
            gate: Mutex::new(None),
        });
        let reader = Arc::new(Reader::default());
        let (mailbox, receiver) =
            ActorMailbox::bounded(NonZeroUsize::new(1).expect("bounded real queue"));
        let refresh = Arc::new(Refresh {
            mailbox,
            calls: AtomicUsize::new(0),
            seen: Mutex::new(vec![]),
            mode: Mutex::new(None),
        });
        let clock = Arc::new(FixtureClock(Mutex::new(Instant::now())));
        let service = AvailabilityQueryService::new_with_clock(
            admission.clone(),
            reader.clone(),
            refresh.clone(),
            clock.clone(),
        );
        Self {
            admission,
            reader,
            refresh,
            clock,
            receiver,
            service,
        }
    }

    fn context(&self, id: u64) -> CommandContext {
        CommandContext::new(
            RequestId::new(id).expect("nonzero request"),
            Some(SnapshotRevision::from_value(41)),
            self.clock.now() + Duration::from_millis(500),
        )
    }

    fn assert_no_availability_io(&self) {
        assert_eq!(self.reader.calls(), 0);
        assert_eq!(self.refresh.calls.load(Ordering::SeqCst), 0);
        assert_eq!(self.refresh.mailbox.available_capacity(), 1);
    }
}

fn sign_public(keys: &Keys, kind: u16, created_at: u64, tags: Vec<Vec<String>>) -> Event {
    const CONTENT: &str = "Public availability port fixture.";
    assert!(tags.len() <= 32);
    assert!(tags.iter().map(Vec::len).sum::<usize>() <= 128);
    assert!(
        CONTENT.len() + tags.iter().flatten().map(String::len).sum::<usize>() <= MAX_RECIPE_BYTES
    );
    assert!(
        tags.iter()
            .flatten()
            .all(|value| value.len() <= DEFAULT_TAG_ELEMENT_MAX_BYTES)
    );
    EventBuilder::new(Kind::from(kind), CONTENT)
        .tags(
            tags.into_iter()
                .map(|tag| Tag::parse(tag).expect("bounded public recipe")),
        )
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("isolated ephemeral public fixture signing")
}

fn verified(json: &str) -> RadrootsSignatureVerifiedEvent {
    assert!(json.len() <= MAX_PUBLIC_EVENT_BYTES);
    verify_nip01_event(
        Nip01EventWire::parse_json_unverified(json)
            .expect("bounded public wire")
            .into_unverified_envelope()
            .expect("public envelope"),
    )
    .expect("real shared ID and signature verification")
}

/// All ephemeral keys are dropped before public evidence or a service escapes.
/// A boolean selects only a public recipe's generic/focused profile, not admission.
fn public_heads(specs: &[(&str, bool)], delete_first: bool) -> Vec<AvailabilityHeadView> {
    assert!(!specs.is_empty());
    assert!(specs.len() + usize::from(delete_first) <= MAX_PUBLIC_EVENTS);
    let keys = Keys::generate();
    let mut events = Vec::new();
    for (index, (identifier, focused)) in specs.iter().enumerate() {
        let mut tags: Vec<Vec<String>> = [
            vec!["d", identifier],
            vec!["title", "Public port listing"],
            vec!["summary", "Public local fixture"],
            vec!["published_at", "1800000000"],
            vec!["location", "Fixture location"],
            vec!["price", "3.5", "CAD"],
            vec!["status", "active"],
        ]
        .into_iter()
        .map(|tag| tag.into_iter().map(str::to_owned).collect())
        .collect();
        if *focused {
            tags.push(vec!["radroots:price_unit".into(), "lb".into()]);
        }
        events.push(sign_public(
            &keys,
            30402,
            1_800_000_100 + index as u64,
            tags,
        ));
    }
    let deletion = delete_first.then(|| {
        sign_public(
            &keys,
            5,
            1_800_000_200,
            vec![
                vec!["e".into(), events[0].id.to_hex()],
                vec!["k".into(), "30402".into()],
            ],
        )
    });
    drop(keys);
    let jsons: Vec<_> = events.into_iter().map(|event| event.as_json()).collect();
    let deletion_json = deletion.map(|event| event.as_json());
    assert!(
        jsons.iter().map(String::len).sum::<usize>()
            + deletion_json.as_ref().map_or(0, String::len)
            <= MAX_PUBLIC_FIXTURE_BYTES
    );
    assert!(
        jsons
            .iter()
            .all(|json| json.len() <= MAX_PUBLIC_EVENT_BYTES)
    );
    let deletions: Vec<_> = deletion_json
        .into_iter()
        .map(|json| {
            assert!(json.len() <= MAX_PUBLIC_EVENT_BYTES);
            verify_and_admit_nip09_deletion_request_event(
                Nip01EventWire::parse_json_unverified(&json)
                    .expect("public deletion wire")
                    .into_unverified_envelope()
                    .expect("public deletion envelope"),
            )
            .expect("real shared same-author deletion verification")
        })
        .collect();
    jsons
        .iter()
        .enumerate()
        .map(|(index, json)| {
            let event = verified(json);
            let candidate = match event_head_candidate_for_nip01_event(event.event()) {
                EventHeadCandidateResult::Candidate(candidate) => candidate,
                _ => panic!("public addressable head evidence"),
            };
            let selected = match select_event_head(candidate, None) {
                EventHeadDecision::Applied(selected) => selected,
                _ => panic!("first verified head must be applied"),
            };
            let observation = AvailabilityObservation::parse(
                "wss://fixture.example.test/public",
                UnixTimestamp::from_seconds(1_800_000_500).expect("public observation time"),
            )
            .expect("pure bounded public source reference, no connection");
            let view = AvailabilityVersionView::from_verified(event, json, observation)
                .expect("actual verified version view");
            AvailabilityHeadView::from_selected(
                selected,
                view,
                if index == 0 { &deletions } else { &[] },
            )
            .expect("shared selection and suppression evidence")
        })
        .collect()
}

#[tokio::test]
async fn admitted_local_page_uses_no_network_or_signer() {
    let head = public_heads(&[("page", true)], false).remove(0);
    assert_eq!(head.state(), AvailabilityHeadState::Focused);
    let expected_id = head.version().expect("verified public listing").version();
    let rows = vec![head];
    let allocation = rows.as_ptr();
    let h = Harness::new();
    let service =
        AvailabilityQueryService::new(h.admission.clone(), h.reader.clone(), h.refresh.clone());
    h.reader.pages.lock().expect("fixture").push_back(Ok(page(
        rows,
        AvailabilityPageContinuation::End,
        7,
    )));
    let query = query(1);
    let result = service
        .read_page(&query)
        .await
        .expect("admitted local page");
    assert_eq!(
        result.items().as_ptr(),
        allocation,
        "owned rows move without dataset cloning"
    );
    assert_eq!(result.items().len(), 1);
    assert_eq!(
        result.items()[0]
            .version()
            .expect("retained exact evidence")
            .version(),
        expected_id
    );
    assert_eq!(result.projection_generation(), 7);
    assert_eq!(result.continuation(), &AvailabilityPageContinuation::End);
    assert_eq!(h.reader.page_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *h.reader.queries.lock().expect("captured query"),
        vec![query]
    );
    assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        h.refresh.calls.load(Ordering::SeqCst),
        0,
        "local read submits no discovery work"
    );
    assert_eq!(h.refresh.mailbox.available_capacity(), 1);
}

#[tokio::test]
async fn admitted_local_detail_uses_no_network_or_signer() {
    let present = public_heads(&[("detail", true)], false).remove(0);
    let expected = present.clone();
    let coordinate = present
        .version()
        .expect("verified view")
        .listing_coordinate()
        .expect("focused coordinate")
        .clone();
    let h = Harness::new();
    h.reader
        .heads
        .lock()
        .expect("fixture")
        .push_back(Ok(present));
    let value = h
        .service
        .read_head(&scope(), &coordinate)
        .await
        .expect("admitted current detail");
    assert_eq!(value, expected);
    assert_eq!(
        *h.reader.head_requests.lock().expect("captured head"),
        vec![(scope(), coordinate)]
    );
    assert_eq!(h.reader.head_calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    assert_eq!(h.refresh.calls.load(Ordering::SeqCst), 0);
    assert_eq!(h.refresh.mailbox.available_capacity(), 1);
}

#[tokio::test]
async fn page_denial_precedes_availability_io() {
    for denied in [
        SafeError::new(
            SafeErrorCode::StorageUnavailable,
            SafeMessage::new("Local admission unavailable."),
        ),
        SafeError::new(
            SafeErrorCode::InvalidApplicationState,
            SafeMessage::new("Local account admission denied."),
        ),
        SafeError::from(AvailabilityFailure::Unavailable),
    ] {
        let h = Harness::new();
        h.admission.set(Err(denied));
        assert_eq!(failure(h.service.read_page(&query(1)).await), denied);
        assert_eq!(h.admission.calls.load(Ordering::SeqCst), 1);
        h.assert_no_availability_io();
    }
}

#[tokio::test]
async fn detail_denial_precedes_availability_io() {
    let denied = SafeError::new(
        SafeErrorCode::StorageQuarantined,
        SafeMessage::new("Local admission quarantined."),
    );
    let h = Harness::new();
    h.admission.set(Err(denied));
    let coordinate = coordinate("denied");
    assert_eq!(
        failure(h.service.read_head(&scope(), &coordinate).await),
        denied
    );
    assert_eq!(
        failure(
            h.service
                .read_version(&scope(), &coordinate, version(1))
                .await
        ),
        denied
    );
    assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    h.assert_no_availability_io();
}

#[tokio::test]
async fn refresh_denial_precedes_submission() {
    let denied = SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("No locally admitted account."),
    );
    let h = Harness::new();
    h.admission.set(Err(denied));
    assert_eq!(
        failure(h.service.submit_refresh(request(1), h.context(1)).await),
        denied
    );
    h.assert_no_availability_io();
    for (current, expected) in changes() {
        let h = Harness::new();
        h.admission.set(Ok(current));
        assert_code(
            failure(h.service.submit_refresh(request(1), h.context(1)).await),
            expected,
        );
        h.assert_no_availability_io();
    }
}

#[tokio::test]
async fn page_rejects_cross_scope_and_stale_bindings_before_io() {
    for (current, expected) in changes() {
        let h = Harness::new();
        h.admission.set(Ok(current));
        assert_code(failure(h.service.read_page(&query(1)).await), expected);
        let coordinate = coordinate("preflight");
        assert_code(
            failure(h.service.read_head(&scope(), &coordinate).await),
            expected,
        );
        assert_code(
            failure(
                h.service
                    .read_version(&scope(), &coordinate, version(1))
                    .await,
            ),
            expected,
        );
        assert_eq!(h.admission.calls.load(Ordering::SeqCst), 3);
        h.assert_no_availability_io();
    }
}

#[tokio::test]
async fn exact_detail_preserves_coordinate_and_version_identity() {
    let mut heads = public_heads(&[("historical", true), ("historical", false)], true);
    let unsupported = heads.pop().expect("unsupported public view");
    let deleted = heads.pop().expect("same-author deleted public view");
    assert_eq!(deleted.state(), AvailabilityHeadState::Deleted);
    assert_eq!(unsupported.state(), AvailabilityHeadState::Unsupported);
    let coordinate = deleted
        .version()
        .expect("deleted history retained")
        .listing_coordinate()
        .expect("logical coordinate")
        .clone();
    let h = Harness::new();
    for head in [deleted, unsupported] {
        let expected_version = head.version().expect("retained verified history").clone();
        let id = expected_version.version();
        let original_wire = expected_version.original_json().to_owned();
        h.reader
            .heads
            .lock()
            .expect("fixture")
            .push_back(Ok(head.clone()));
        h.reader
            .versions
            .lock()
            .expect("fixture")
            .push_back(Ok(Some(expected_version)));
        assert_eq!(
            h.service
                .read_head(&scope(), &coordinate)
                .await
                .expect("current evidence"),
            head
        );
        let exact = h
            .service
            .read_version(&scope(), &coordinate, id)
            .await
            .expect("exact historical evidence")
            .expect("retained version");
        assert_eq!(exact.version(), id);
        assert_eq!(exact.listing_coordinate(), Some(&coordinate));
        assert_eq!(exact.original_json(), original_wire);
    }
    h.reader
        .versions
        .lock()
        .expect("fixture")
        .push_back(Ok(None));
    assert_eq!(
        h.service
            .read_version(&scope(), &coordinate, version(9))
            .await
            .expect("no retained version"),
        None
    );
    assert_eq!(h.reader.version_calls.load(Ordering::SeqCst), 3);
    let captured = h
        .reader
        .version_requests
        .lock()
        .expect("captured exact requests");
    assert!(
        captured
            .iter()
            .all(|(binding, actual, _)| *binding == scope() && actual == &coordinate)
    );
    assert_eq!(captured.last().expect("absence request").2, version(9));
    assert_eq!(h.refresh.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_capacity_and_storage_errors_preserve_safe_codes() {
    for error in [
        SafeError::from(AvailabilityFailure::Capacity),
        SafeError::new(
            SafeErrorCode::StorageUnavailable,
            SafeMessage::new("The local reader is unavailable."),
        ),
        SafeError::new(
            SafeErrorCode::StorageCorrupt,
            SafeMessage::new("The local reader found corrupt state."),
        ),
        SafeError::new(
            SafeErrorCode::StorageQuarantined,
            SafeMessage::new("The local reader is quarantined."),
        ),
    ] {
        let h = Harness::new();
        h.reader
            .pages
            .lock()
            .expect("fixture")
            .push_back(Err(error));
        h.reader
            .heads
            .lock()
            .expect("fixture")
            .push_back(Err(error));
        h.reader
            .versions
            .lock()
            .expect("fixture")
            .push_back(Err(error));
        let coordinate = coordinate("safe-errors");
        assert_eq!(failure(h.service.read_page(&query(1)).await), error);
        assert_eq!(
            failure(h.service.read_head(&scope(), &coordinate).await),
            error
        );
        assert_eq!(
            failure(
                h.service
                    .read_version(&scope(), &coordinate, version(1))
                    .await
            ),
            error
        );
        assert_eq!(h.reader.calls(), 3);
        assert_eq!(
            h.admission.calls.load(Ordering::SeqCst),
            6,
            "returned port errors still pass post-authority checks"
        );
        assert_eq!(h.refresh.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn page_rejects_a_different_projection_generation() {
    for generation in [0, 6, 8, u64::MAX] {
        let h = Harness::new();
        h.reader.pages.lock().expect("fixture").push_back(Ok(page(
            vec![],
            AvailabilityPageContinuation::End,
            generation,
        )));
        assert_code(
            failure(h.service.read_page(&query(1)).await),
            SafeErrorCode::AvailabilityStaleQuery,
        );
        assert_eq!(h.reader.page_calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn owner_switch_rejects_delayed_page() {
    let late_denial = SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("Local authority was withdrawn."),
    );
    let returned_error = SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("Earlier reader error."),
    );
    for changed in [
        Ok(scope_with(OTHER_AUTHOR, 1, 2, 5, 7, 3)),
        Ok(scope_with(AUTHOR, 9, 2, 5, 7, 3)),
        Err(late_denial),
    ] {
        for port_error in [false, true] {
            let h = Harness::new();
            h.reader
                .pages
                .lock()
                .expect("fixture")
                .push_back(if port_error {
                    Err(returned_error)
                } else {
                    Ok(page(vec![], AvailabilityPageContinuation::End, 7))
                });
            let (mut entered, release) = arm(&h.reader.gate);
            let query = query(1);
            let mut future = Box::pin(h.service.read_page(&query));
            pending(future.as_mut()).await;
            entered
                .try_recv()
                .expect("actual reader crossed awaited barrier");
            assert_eq!(h.reader.page_calls.load(Ordering::SeqCst), 1);
            h.admission.set(changed);
            release.send(()).expect("complete delayed local read");
            let error = failure(future.await);
            match changed {
                Ok(_) => assert_code(error, SafeErrorCode::AvailabilityScopeMismatch),
                Err(expected) => assert_eq!(error, expected),
            }
            assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn generation_switch_rejects_delayed_detail() {
    for (changed, expected) in changes().into_iter().skip(2) {
        for historical in [false, true] {
            let h = Harness::new();
            let coordinate = coordinate("delayed-detail");
            if historical {
                h.reader
                    .versions
                    .lock()
                    .expect("fixture")
                    .push_back(Ok(None));
            } else {
                h.reader
                    .heads
                    .lock()
                    .expect("fixture")
                    .push_back(Ok(AvailabilityHeadView::missing(coordinate.clone())));
            }
            let (mut entered, release) = arm(&h.reader.gate);
            let binding = scope();
            let operation: BoxFuture<'_, Result<(), SafeError>> = if historical {
                Box::pin(async {
                    h.service
                        .read_version(&binding, &coordinate, version(1))
                        .await
                        .map(|_| ())
                })
            } else {
                Box::pin(async { h.service.read_head(&binding, &coordinate).await.map(|_| ()) })
            };
            let mut future = operation;
            pending(future.as_mut()).await;
            entered.try_recv().expect("actual awaited detail");
            h.admission.set(Ok(changed));
            release.send(()).expect("complete delayed detail");
            assert_code(failure(future.await), expected);
            assert_eq!(h.reader.calls(), 1);
            assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn refresh_submission_returns_before_completion() {
    let mut h = Harness::new();
    let original = request(1);
    let context = h.context(1);
    let operation = ready(h.service.submit_refresh(original.clone(), context))
        .await
        .expect("independent accepted submission");
    assert_eq!(operation.request_id(), original.request_id());
    assert_eq!(h.refresh.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.refresh.mailbox.available_capacity(), 0);
    assert_eq!(h.reader.calls(), 0);
    let mut receipt = Box::pin(operation.receipt());
    pending(receipt.as_mut()).await;
    let envelope = h.receiver.try_recv().expect("real bounded queued command");
    assert_eq!(
        envelope.context(),
        context,
        "whole absolute context forwarded unchanged"
    );
    assert_eq!(envelope.command(), &original);
    let (_, submitted, reply) = envelope.into_parts();
    reply
        .send(CommandReceipt::new(
            submitted.request_id(),
            CommandResult::Completed(outcome(submitted)),
        ))
        .expect("actual worker completion reply");
    let received = receipt.await;
    assert_eq!(received.request_id().get(), 1);
    assert!(matches!(received.result(), CommandResult::Completed(_)));

    let mut dropped = Harness::new();
    let operation = ready(
        dropped
            .service
            .submit_refresh(request(2), dropped.context(2)),
    )
    .await
    .expect("queued refresh");
    assert_eq!(dropped.refresh.mailbox.available_capacity(), 0);
    drop(operation);
    assert_eq!(
        dropped.refresh.mailbox.available_capacity(),
        0,
        "caller loss does not dequeue work or release queued capacity"
    );
    let envelope = dropped
        .receiver
        .try_recv()
        .expect("queued work remains after caller loss");
    assert_eq!(envelope.context().request_id().get(), 2);
    assert_eq!(envelope.command().request_id().get(), 2);
    let (_, submitted, reply) = envelope.into_parts();
    assert!(
        reply
            .send(CommandReceipt::new(
                submitted.request_id(),
                CommandResult::Completed(outcome(submitted))
            ))
            .is_err(),
        "only reply receiver was lost; no worker cancellation claim"
    );
}

#[tokio::test]
async fn stalled_refresh_does_not_block_local_page() {
    let present = public_heads(&[("cached", true)], false).remove(0);
    let id = present.version().expect("public cached evidence").version();
    let mut h = Harness::new();
    h.reader.pages.lock().expect("fixture").push_back(Ok(page(
        vec![present],
        AvailabilityPageContinuation::End,
        7,
    )));
    let operation = h
        .service
        .submit_refresh(request(1), h.context(1))
        .await
        .expect("independent refresh");
    let mut refresh_receipt = Box::pin(operation.receipt());
    pending(refresh_receipt.as_mut()).await;
    let local = ready(h.service.read_page(&query(1)))
        .await
        .expect("local page while refresh is stalled");
    assert_eq!(local.items().len(), 1);
    assert_eq!(
        local.items()[0]
            .version()
            .expect("cached data retained")
            .version(),
        id
    );
    assert_eq!(local.continuation(), &AvailabilityPageContinuation::End);
    assert_eq!(h.refresh.mailbox.available_capacity(), 0);
    pending(refresh_receipt.as_mut()).await;
    let (_, submitted, reply) = h
        .receiver
        .try_recv()
        .expect("independently queued refresh")
        .into_parts();
    reply
        .send(CommandReceipt::new(
            submitted.request_id(),
            CommandResult::Completed(outcome(submitted)),
        ))
        .expect("complete queued fixture");
    assert!(matches!(
        refresh_receipt.await.result(),
        CommandResult::Completed(_)
    ));
    assert_eq!(h.reader.page_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refresh_failure_preserves_readable_cached_data() {
    let present = public_heads(&[("cached-after-failure", true)], false).remove(0);
    let expected = present.clone();
    let coordinate = present
        .version()
        .expect("verified cache")
        .listing_coordinate()
        .expect("logical coordinate")
        .clone();
    let mut h = Harness::new();
    let operation = h
        .service
        .submit_refresh(request(1), h.context(1))
        .await
        .expect("queued refresh");
    let (_, submitted, reply) = h
        .receiver
        .try_recv()
        .expect("real independent command")
        .into_parts();
    let failure = SafeError::from(AvailabilityFailure::Unavailable);
    reply
        .send(CommandReceipt::new(
            submitted.request_id(),
            CommandResult::Failed(failure),
        ))
        .expect("actual failed worker receipt");
    assert_failed(
        operation.receipt().await,
        1,
        SafeErrorCode::AvailabilityUnavailable,
    );
    h.reader
        .heads
        .lock()
        .expect("fixture")
        .push_back(Ok(present));
    assert_eq!(
        h.service
            .read_head(&scope(), &coordinate)
            .await
            .expect("readable cache survives failed refresh"),
        expected
    );
    assert_eq!(h.reader.head_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        h.refresh.calls.load(Ordering::SeqCst),
        1,
        "failure does not automatically resubmit"
    );
}

#[tokio::test]
async fn refresh_receipts_preserve_request_identity() {
    let retained = version(9);
    for variant in 0..7 {
        let mut h = Harness::new();
        let original = request(7);
        let context = h.context(7);
        let operation = h
            .service
            .submit_refresh(original.clone(), context)
            .await
            .expect("accepted real ticket");
        assert_eq!(operation.request_id().get(), 7);
        let envelope = h.receiver.try_recv().expect("actual submission");
        assert_eq!(envelope.context(), context);
        let (_, submitted, reply) = envelope.into_parts();
        let error = SafeError::new(
            SafeErrorCode::StorageUnavailable,
            SafeMessage::new("Refresh adapter unavailable."),
        );
        let mut retained_pointer = None;
        if variant == 6 {
            drop(reply);
        } else {
            let result = match variant {
                0 => {
                    let progress = |state, reason| {
                        AvailabilityDiscoveryProgress::new(
                            AvailabilityDiscoveryState::Requested(state),
                            reason,
                            1,
                        )
                        .expect("independent finite stream progress")
                    };
                    let target = AvailabilityDiscoveryTargetOutcome::new(
                        submitted.targets()[0].clone(),
                        progress(
                            FetchTargetState::Partial,
                            AvailabilityDiscoveryStopReason::BudgetExhausted,
                        ),
                        progress(
                            FetchTargetState::Complete,
                            AvailabilityDiscoveryStopReason::None,
                        ),
                        progress(
                            FetchTargetState::FailedRetryable,
                            AvailabilityDiscoveryStopReason::None,
                        ),
                    )
                    .expect("three independently retained stream counts");
                    let items = vec![retained];
                    retained_pointer = Some(items.as_ptr());
                    CommandResult::Completed(
                        AvailabilityDiscoveryOutcome::new(
                            submitted,
                            AvailabilityDiscoveryUsage::new(1, 3).expect("actual supplied counts"),
                            vec![target],
                            items,
                        )
                        .expect("valid partial outcome"),
                    )
                }
                1 => CommandResult::Rejected(CommandRejection::MailboxSaturated),
                2 => CommandResult::Conflicted {
                    current_revision: SnapshotRevision::from_value(43),
                },
                3 => CommandResult::TimedOut,
                4 => CommandResult::Closed,
                5 => CommandResult::Failed(error),
                _ => unreachable!("all original variants enumerated"),
            };
            reply
                .send(CommandReceipt::new(original.request_id(), result))
                .expect("actual receipt delivery");
        }
        h.clock.advance(Duration::from_secs(2));
        assert!(
            context.is_expired(h.clock.now()),
            "receipt collection is genuinely after original deadline"
        );
        let receipt = operation.receipt().await;
        assert_eq!(receipt.request_id(), original.request_id());
        match (variant, receipt.into_result()) {
            (0, CommandResult::Completed(value)) => {
                assert_eq!(value.request(), &original);
                assert_eq!(value.items(), &[retained]);
                assert_eq!(Some(value.items().as_ptr()), retained_pointer);
                assert_eq!(value.usage().returned_events(), 3);
                assert_eq!(value.targets()[0].listings().returned(), 1);
                assert_eq!(value.targets()[0].profiles().returned(), 1);
                assert_eq!(value.targets()[0].deletions().returned(), 1);
                assert_eq!(
                    value.listings_state(),
                    AvailabilityDiscoveryState::Requested(FetchTargetState::Partial)
                );
                assert_eq!(
                    value.profiles_state(),
                    AvailabilityDiscoveryState::Requested(FetchTargetState::Complete)
                );
                assert_eq!(
                    value.deletions_state(),
                    AvailabilityDiscoveryState::Requested(FetchTargetState::FailedRetryable)
                );
                assert_eq!(
                    value.targets()[0].listings().reason(),
                    AvailabilityDiscoveryStopReason::BudgetExhausted
                );
            }
            (1, CommandResult::Rejected(CommandRejection::MailboxSaturated)) => {}
            (2, CommandResult::Conflicted { current_revision }) => {
                assert_eq!(current_revision.value(), 43)
            }
            (3, CommandResult::TimedOut) | (4 | 6, CommandResult::Closed) => {}
            (5, CommandResult::Failed(actual)) => assert_eq!(actual, error),
            _ => panic!("original valid receipt facts must be preserved"),
        }
    }
    let mut reordered = Harness::new();
    let original = request_at(8, scope(), &[2, 1], 1000);
    let operation = reordered
        .service
        .submit_refresh(original, reordered.context(8))
        .await
        .expect("two-target request");
    let (_, _, reply) = reordered
        .receiver
        .try_recv()
        .expect("real request")
        .into_parts();
    reply
        .send(CommandReceipt::new(
            RequestId::new(8).expect("ID"),
            CommandResult::Completed(outcome(request_at(8, scope(), &[1, 2], 1000))),
        ))
        .expect("same target set in valid reordered outcome");
    match operation.receipt().await.into_result() {
        CommandResult::Completed(value) => {
            assert_eq!(value.request().targets()[0].as_str(), format!("{:064x}", 1))
        }
        _ => panic!("target set agreement preserves valid order differences"),
    }
    for result in [
        CommandResult::Rejected(CommandRejection::MailboxSaturated),
        CommandResult::TimedOut,
        CommandResult::Closed,
        CommandResult::Conflicted {
            current_revision: SnapshotRevision::from_value(99),
        },
        CommandResult::Failed(SafeError::from(AvailabilityFailure::UnsupportedProfile)),
    ] {
        let h = Harness::new();
        let expected = match &result {
            CommandResult::Rejected(_) => 0,
            CommandResult::TimedOut => 1,
            CommandResult::Closed => 2,
            CommandResult::Conflicted { .. } => 3,
            CommandResult::Failed(_) => 4,
            CommandResult::Completed(_) => unreachable!("rejected submission variants"),
        };
        *h.refresh.mode.lock().expect("fixture") = Some(SubmitMode::Receipt(Box::new(
            CommandReceipt::new(RequestId::new(1).expect("ID"), result),
        )));
        let operation = h
            .service
            .submit_refresh(request(1), h.context(1))
            .await
            .expect("immediate rejected submission");
        let receipt = ready(operation.receipt()).await;
        assert_eq!(receipt.request_id().get(), 1);
        match (expected, receipt.into_result()) {
            (0, CommandResult::Rejected(CommandRejection::MailboxSaturated))
            | (1, CommandResult::TimedOut)
            | (2, CommandResult::Closed) => {}
            (3, CommandResult::Conflicted { current_revision }) => {
                assert_eq!(current_revision.value(), 99)
            }
            (4, CommandResult::Failed(error)) => assert_eq!(
                error,
                SafeError::from(AvailabilityFailure::UnsupportedProfile)
            ),
            _ => panic!("immediate submission facts retained"),
        }
    }
}

#[tokio::test]
async fn scope_switch_rejects_delayed_refresh_completion() {
    let mut changed: Vec<_> = changes()
        .into_iter()
        .map(|(binding, code)| (Ok(binding), code))
        .collect();
    let denied = SafeError::new(
        SafeErrorCode::StorageQuarantined,
        SafeMessage::new("Refresh completion admission withdrawn."),
    );
    changed.push((Err(denied), denied.code()));
    for (current, code) in changed {
        for failed_worker in [false, true] {
            let mut h = Harness::new();
            let operation = h
                .service
                .submit_refresh(request(1), h.context(1))
                .await
                .expect("accepted independent work");
            let mut receipt = Box::pin(operation.receipt());
            pending(receipt.as_mut()).await;
            let (_, submitted, reply) = h
                .receiver
                .try_recv()
                .expect("delayed real command")
                .into_parts();
            h.admission.set(current);
            let result = if failed_worker {
                CommandResult::Failed(SafeError::from(AvailabilityFailure::Unavailable))
            } else {
                CommandResult::Completed(outcome(submitted))
            };
            reply
                .send(CommandReceipt::new(
                    RequestId::new(1).expect("original ID"),
                    result,
                ))
                .expect("complete delayed refresh");
            let receipt = receipt.await;
            if let Err(expected) = current {
                assert_eq!(receipt.request_id().get(), 1);
                match receipt.into_result() {
                    CommandResult::Failed(error) => assert_eq!(error, expected),
                    _ => panic!("post-authority denial wins"),
                }
            } else {
                assert_failed(receipt, 1, code);
            }
            assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn current_detail_refuses_wrong_missing_or_selected_coordinate() {
    let mut heads = public_heads(
        &[
            ("foreign", true),
            ("foreign", true),
            ("foreign", false),
            ("requested", true),
            ("", true),
        ],
        true,
    );
    let empty = heads.pop().expect("broader raw empty coordinate");
    let requested = heads.pop().expect("requested public coordinate");
    let coordinate = requested
        .version()
        .expect("actual requested evidence")
        .listing_coordinate()
        .expect("logical request")
        .clone();
    let wrong = heads[0]
        .version()
        .expect("actual foreign evidence")
        .listing_coordinate()
        .expect("other listing")
        .clone();
    assert_ne!(wrong, coordinate);
    assert_eq!(heads[0].state(), AvailabilityHeadState::Deleted);
    assert_eq!(heads[1].state(), AvailabilityHeadState::Focused);
    assert_eq!(heads[2].state(), AvailabilityHeadState::Unsupported);
    let mut responses = vec![AvailabilityHeadView::missing(wrong)];
    responses.extend(heads);
    responses.push(empty);
    for response in responses {
        let h = Harness::new();
        h.reader
            .heads
            .lock()
            .expect("fixture")
            .push_back(Ok(response));
        assert_code(
            failure(h.service.read_head(&scope(), &coordinate).await),
            SafeErrorCode::AvailabilityScopeMismatch,
        );
        assert_eq!(h.reader.head_calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    }
    let h = Harness::new();
    h.reader
        .heads
        .lock()
        .expect("fixture")
        .push_back(Ok(AvailabilityHeadView::missing(coordinate.clone())));
    let missing = h
        .service
        .read_head(&scope(), &coordinate)
        .await
        .expect("exact missing coordinate preserved");
    assert_eq!(missing.state(), AvailabilityHeadState::Missing);
    assert_eq!(missing.listing_coordinate(), Some(&coordinate));
}

#[tokio::test]
async fn exact_detail_refuses_wrong_returned_event_version() {
    let heads = public_heads(
        &[
            ("exact", true),
            ("exact", true),
            ("other", true),
            ("", true),
        ],
        false,
    );
    let coordinate = heads[0]
        .version()
        .expect("first real version")
        .listing_coordinate()
        .expect("logical request")
        .clone();
    let requested_id = heads[0].version().expect("first real version").version();
    assert_ne!(
        requested_id,
        heads[1].version().expect("second real version").version()
    );
    for (id, returned) in [
        (
            requested_id,
            heads[1].version().expect("wrong exact version").clone(),
        ),
        (
            heads[2].version().expect("foreign coordinate").version(),
            heads[2].version().expect("foreign coordinate").clone(),
        ),
        (
            heads[3].version().expect("raw empty coordinate").version(),
            heads[3].version().expect("raw empty coordinate").clone(),
        ),
    ] {
        let h = Harness::new();
        h.reader
            .versions
            .lock()
            .expect("fixture")
            .push_back(Ok(Some(returned)));
        assert_code(
            failure(h.service.read_version(&scope(), &coordinate, id).await),
            SafeErrorCode::AvailabilityScopeMismatch,
        );
        assert_eq!(h.reader.version_calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.admission.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn page_refuses_over_requested_rows_and_foreign_continuation() {
    let query = query(1);
    let h = Harness::new();
    h.reader.pages.lock().expect("fixture").push_back(Ok(page(
        vec![
            AvailabilityHeadView::missing(coordinate("first")),
            AvailabilityHeadView::missing(coordinate("second")),
        ],
        AvailabilityPageContinuation::End,
        7,
    )));
    assert_code(
        failure(h.service.read_page(&query).await),
        SafeErrorCode::AvailabilityCapacity,
    );

    let foreign_query = ScopedAvailabilityQuery::new(
        scope(),
        AvailabilityQueryFilters::new(None, None, None),
        AvailabilityPageLimit::new(2).expect("different limit"),
        None,
    )
    .expect("foreign complete query");
    let after = AvailabilityOrderKey::new(EventTimestamp::new(11), version(3));
    let h = Harness::new();
    h.reader.pages.lock().expect("fixture").push_back(Ok(page(
        vec![],
        AvailabilityPageContinuation::More(AvailabilityPageCursor::encode(
            foreign_query.fingerprint(),
            after,
        )),
        7,
    )));
    assert_code(
        failure(h.service.read_page(&query).await),
        SafeErrorCode::AvailabilityStaleQuery,
    );

    let digest: String = query
        .fingerprint()
        .bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let literal = format!("hcq1:{digest}:000000000000000b:{:064x}", 3);
    let cursor = AvailabilityPageCursor::parse(&literal, query.fingerprint())
        .expect("independently framed valid continuation");
    let continued_query = ScopedAvailabilityQuery::new(
        scope(),
        AvailabilityQueryFilters::new(None, None, None),
        AvailabilityPageLimit::new(1).expect("original limit"),
        Some(&literal),
    )
    .expect("continuation retains complete original request");
    let h = Harness::new();
    h.reader.pages.lock().expect("fixture").push_back(Ok(page(
        vec![],
        AvailabilityPageContinuation::More(cursor),
        7,
    )));
    let returned = h
        .service
        .read_page(&continued_query)
        .await
        .expect("filtered/capped empty page can retain More");
    assert!(returned.items().is_empty());
    match returned.continuation() {
        AvailabilityPageContinuation::More(cursor) => {
            assert_eq!(cursor.as_str(), literal);
            assert_eq!(cursor.after(), after);
        }
        AvailabilityPageContinuation::End => panic!("More must not silently reset to End"),
    }
    assert_eq!(
        *h.reader.queries.lock().expect("forwarded exact query"),
        vec![continued_query]
    );
}

#[tokio::test]
async fn refresh_refuses_context_identity_and_deadline_mismatch_before_submission() {
    let h = Harness::new();
    assert_code(
        failure(h.service.submit_refresh(request(1), h.context(2)).await),
        SafeErrorCode::AvailabilityInvalidInput,
    );
    h.assert_no_availability_io();
    for remaining in [
        Duration::from_millis(1001),
        Duration::from_millis(1000) + Duration::from_nanos(1),
    ] {
        let h = Harness::new();
        let context = CommandContext::new(
            RequestId::new(1).expect("ID"),
            None,
            h.clock.now() + remaining,
        );
        assert_code(
            failure(h.service.submit_refresh(request(1), context).await),
            SafeErrorCode::AvailabilityInvalidInput,
        );
        h.assert_no_availability_io();
    }
    for remaining in [Duration::from_millis(1000), Duration::from_millis(500)] {
        let mut h = Harness::new();
        let context = CommandContext::new(
            RequestId::new(1).expect("ID"),
            Some(SnapshotRevision::from_value(81)),
            h.clock.now() + remaining,
        );
        let operation = h
            .service
            .submit_refresh(request(1), context)
            .await
            .expect("equal or shorter complete Duration accepted");
        assert_eq!(operation.request_id().get(), 1);
        assert_eq!(h.refresh.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            h.refresh.seen.lock().expect("original context")[0].0,
            context
        );
        let envelope = h
            .receiver
            .try_recv()
            .expect("actual original deadline submission");
        assert_eq!(envelope.context(), context);
        drop(operation);
    }
    for elapsed in [Duration::ZERO, Duration::from_nanos(1)] {
        let h = Harness::new();
        let context = CommandContext::new(
            RequestId::new(1).expect("ID"),
            None,
            h.clock.now() - elapsed,
        );
        let operation = h
            .service
            .submit_refresh(request(1), context)
            .await
            .expect("typed expired command receipt");
        h.assert_no_availability_io();
        let receipt = operation.receipt().await;
        assert_eq!(receipt.request_id().get(), 1);
        assert!(matches!(receipt.result(), CommandResult::TimedOut));
    }
}

#[tokio::test]
async fn refresh_expiry_during_admission_never_submits_or_renews_deadline() {
    let h = Harness::new();
    let context = h.context(1);
    let original_now = h.clock.now();
    assert!(!context.is_expired(original_now));
    assert_eq!(
        context.deadline().duration_since(original_now),
        Duration::from_millis(500)
    );
    let (mut entered, release) = arm(&h.admission.gate);
    let mut submission = Box::pin(h.service.submit_refresh(request(1), context));
    pending(submission.as_mut()).await;
    entered
        .try_recv()
        .expect("actual trusted admission is awaited");
    h.assert_no_availability_io();
    h.clock.advance(Duration::from_millis(500));
    assert_eq!(h.clock.now(), context.deadline());
    release
        .send(())
        .expect("complete admission after absolute deadline");
    let operation = submission
        .await
        .expect("original correlated timeout operation");
    assert_eq!(operation.request_id().get(), 1);
    h.assert_no_availability_io();
    let receipt = operation.receipt().await;
    assert_eq!(receipt.request_id().get(), 1);
    assert!(matches!(receipt.result(), CommandResult::TimedOut));
    assert!(
        h.refresh
            .seen
            .lock()
            .expect("no renewed submission")
            .is_empty()
    );

    let mut shorter = Harness::new();
    let context = shorter.context(2);
    let (mut entered, release) = arm(&shorter.admission.gate);
    let mut submission = Box::pin(shorter.service.submit_refresh(request(2), context));
    pending(submission.as_mut()).await;
    entered
        .try_recv()
        .expect("awaited admission before shorter remaining budget");
    shorter.clock.advance(Duration::from_millis(400));
    release.send(()).expect("admit before original deadline");
    let operation = submission
        .await
        .expect("remaining original deadline still valid");
    assert_eq!(
        shorter
            .refresh
            .seen
            .lock()
            .expect("captured exact submission")[0]
            .0,
        context
    );
    let envelope = shorter
        .receiver
        .try_recv()
        .expect("real remaining-window command");
    assert_eq!(
        envelope.context(),
        context,
        "deadline and expected revision remain byte-for-byte original"
    );
    assert_eq!(envelope.command().deadline_millis(), 1000);
    assert_eq!(
        context.deadline().duration_since(shorter.clock.now()),
        Duration::from_millis(100)
    );
    drop(operation);
}

#[tokio::test]
async fn refresh_refuses_foreign_submission_receipt_or_outcome() {
    let mut foreign = Harness::new();
    *foreign.refresh.mode.lock().expect("fixture") = Some(SubmitMode::ForeignAccepted(
        RequestId::new(2).expect("foreign ID"),
    ));
    let operation = ready(
        foreign
            .service
            .submit_refresh(request(1), foreign.context(1)),
    )
    .await
    .expect("correlated invalid submission operation");
    assert_eq!(operation.request_id().get(), 1);
    assert_failed(
        ready(operation.receipt()).await,
        1,
        SafeErrorCode::AvailabilityInvalidInput,
    );
    assert_eq!(
        foreign.refresh.mailbox.available_capacity(),
        0,
        "foreign ticket rejection does not cancel its queued envelope"
    );
    let (_, submitted, reply) = foreign
        .receiver
        .try_recv()
        .expect("foreign work still queued")
        .into_parts();
    assert!(
        reply
            .send(CommandReceipt::new(
                RequestId::new(2).expect("foreign ID"),
                CommandResult::Completed(outcome(submitted))
            ))
            .is_err()
    );

    for result in [
        CommandResult::Closed,
        CommandResult::Failed(SafeError::from(AvailabilityFailure::Unavailable)),
        CommandResult::Rejected(CommandRejection::MailboxSaturated),
    ] {
        let h = Harness::new();
        *h.refresh.mode.lock().expect("fixture") = Some(SubmitMode::Receipt(Box::new(
            CommandReceipt::new(RequestId::new(2).expect("foreign ID"), result),
        )));
        let operation = h
            .service
            .submit_refresh(request(1), h.context(1))
            .await
            .expect("foreign rejected submission correlated");
        assert_failed(
            ready(operation.receipt()).await,
            1,
            SafeErrorCode::AvailabilityInvalidInput,
        );
    }

    let mut h = Harness::new();
    let operation = h
        .service
        .submit_refresh(request(1), h.context(1))
        .await
        .expect("valid initial ticket");
    let (_, submitted, reply) = h.receiver.try_recv().expect("original ticket").into_parts();
    reply
        .send(CommandReceipt::new(
            RequestId::new(2).expect("foreign outer ID"),
            CommandResult::Completed(outcome(submitted)),
        ))
        .expect("malformed outer receipt delivered");
    assert_failed(
        operation.receipt().await,
        1,
        SafeErrorCode::AvailabilityInvalidInput,
    );

    let mut malformed = vec![
        (
            request_at(2, scope(), &[1], 1000),
            SafeErrorCode::AvailabilityInvalidInput,
        ),
        (
            request_at(1, scope(), &[2], 1000),
            SafeErrorCode::AvailabilityScopeMismatch,
        ),
        (
            request_at(1, scope(), &[1], 999),
            SafeErrorCode::AvailabilityInvalidInput,
        ),
        (
            request_at(1, scope(), &[], 1000),
            SafeErrorCode::AvailabilityScopeMismatch,
        ),
    ];
    malformed.extend(
        changes()
            .into_iter()
            .map(|(binding, code)| (request_at(1, binding, &[1], 1000), code)),
    );
    for (returned_request, code) in malformed {
        let mut h = Harness::new();
        let operation = h
            .service
            .submit_refresh(request(1), h.context(1))
            .await
            .expect("original accepted ticket");
        let (_, _, reply) = h
            .receiver
            .try_recv()
            .expect("original real command")
            .into_parts();
        reply
            .send(CommandReceipt::new(
                RequestId::new(1).expect("original outer ID"),
                CommandResult::Completed(outcome(returned_request)),
            ))
            .expect("independently constructed malformed completion");
        assert_failed(operation.receipt().await, 1, code);
    }
}

#[test]
fn scope_validation_preserves_all_original_query_comparisons() {
    let original = scope();
    let query = query(1);
    assert_eq!(original.validate_current(&original), Ok(()));
    assert_eq!(query.validate_scope(&original), Ok(()));
    for (current, expected) in changes() {
        let expected = match expected {
            SafeErrorCode::AvailabilityScopeMismatch => AvailabilityQueryError::ScopeMismatch,
            SafeErrorCode::AvailabilityStaleQuery => AvailabilityQueryError::StaleQuery,
            _ => panic!("frozen structural policy"),
        };
        assert_eq!(original.validate_current(&current), Err(expected));
        assert_eq!(query.validate_scope(&current), Err(expected));
        assert_eq!(
            original,
            scope(),
            "structural comparison leaves original immutable"
        );
    }
    for current in [
        scope_with(OTHER_AUTHOR, 1, 9, 6, 8, 4),
        scope_with(AUTHOR, 9, 9, 6, 8, 4),
    ] {
        assert_eq!(
            original.validate_current(&current),
            Err(AvailabilityQueryError::ScopeMismatch),
            "identity mismatch keeps original precedence over changed generations"
        );
        assert_eq!(
            query.validate_scope(&current),
            Err(AvailabilityQueryError::ScopeMismatch)
        );
    }
}

#[test]
fn head_coordinate_accessor_preserves_missing_selected_and_broad_unsupported_evidence() {
    let coordinate = coordinate("missing");
    let missing = AvailabilityHeadView::missing(coordinate.clone());
    let original = missing.clone();
    assert_eq!(missing.listing_coordinate(), Some(&coordinate));
    assert_eq!(missing.state(), AvailabilityHeadState::Missing);
    assert_eq!(missing.version(), None);
    assert_eq!(missing.suppression(), None);
    assert_eq!(missing, original);

    let long_identifier = "d".repeat(4026);
    let specs = [
        ("deleted", true),
        ("focused", true),
        ("generic", false),
        ("", true),
        (long_identifier.as_str(), true),
    ];
    let heads = public_heads(&specs, true);
    let expected = [
        AvailabilityHeadState::Deleted,
        AvailabilityHeadState::Focused,
        AvailabilityHeadState::Unsupported,
        AvailabilityHeadState::Unsupported,
        AvailabilityHeadState::Unsupported,
    ];
    for (index, head) in heads.iter().enumerate() {
        let before = head.clone();
        assert_eq!(head.state(), expected[index]);
        let view = head
            .version()
            .expect("selected unsupported/deleted evidence remains retained");
        if index < 3 {
            let coordinate = head
                .listing_coordinate()
                .expect("bounded selected coordinate");
            assert_eq!(coordinate.identifier(), specs[index].0);
            assert_eq!(Some(coordinate), view.listing_coordinate());
        } else {
            assert_eq!(
                head.listing_coordinate(),
                None,
                "broader raw evidence does not become a bounded application coordinate"
            );
            assert_eq!(view.listing_coordinate(), None);
        }
        match view.raw_coordinate() {
            radroots_event::envelope::event_head::EventHeadCoordinate::Addressable {
                kind,
                d_tag,
                ..
            } => {
                assert_eq!(*kind, 30402);
                assert_eq!(d_tag, specs[index].0);
            }
            _ => panic!("original raw public coordinate preserved"),
        }
        assert_eq!(head.version(), before.version());
        assert_eq!(head.suppression(), before.suppression());
        assert_eq!(
            head, &before,
            "accessor leaves full selected/suppression/history evidence immutable"
        );
        if index == 0 {
            assert_eq!(head.suppression().expect("real shared deletion decision").outcome(), radroots_event_codec::admission::deletion::RadrootsNip09SuppressionOutcome::Suppressed);
            assert!(head.focused().is_none());
            assert!(
                view.focused().is_some(),
                "deleted focused data remains historical evidence"
            );
        }
    }
}
