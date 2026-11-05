//! Bounded committed count descriptions. Decoding supplies no admission or
//! execution authority; full originals must independently authenticate capture.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{CountClassificationCommand, CountExplanationCommand, CountExplanationFacts},
    reference::{SourceProvenance, WholePacks},
    stock::Occurrence,
};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum Description {
    Explanation {
        schema: u8,
        report: String,
        reconciliation: String,
        stock_revision: u64,
        catalog_revision: u64,
        previous: Option<String>,
        packs: u32,
        recipient: String,
        occurrence: Clock,
        source_operation: String,
        source_group: String,
        source_evidence: String,
        quantity_evidence: String,
        recipient_evidence: String,
        timing_evidence: String,
    },
    Classification {
        schema: u8,
        observation: String,
        stock_revision: u64,
        catalog_revision: u64,
        account_previous: Option<String>,
        previous: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum Clock {
    Unknown,
    Known { at: i64 },
    Interval { earliest: i64, latest: i64 },
}
impl Clock {
    fn from_domain(value: Occurrence) -> Self {
        match value {
            Occurrence::Unknown => Self::Unknown,
            Occurrence::Known(at) => Self::Known {
                at: at.as_seconds(),
            },
            Occurrence::Interval { earliest, latest } => Self::Interval {
                earliest: earliest.as_seconds(),
                latest: latest.as_seconds(),
            },
        }
    }
    fn domain(self) -> Result<Occurrence, SafeError> {
        let at = |n| UnixTimestamp::from_seconds(n).ok_or_else(corrupt_storage);
        match self {
            Self::Unknown => Ok(Occurrence::Unknown),
            Self::Known { at: n } => Ok(Occurrence::Known(at(n)?)),
            Self::Interval { earliest, latest } if earliest <= latest => Ok(Occurrence::Interval {
                earliest: at(earliest)?,
                latest: at(latest)?,
            }),
            _ => Err(corrupt_storage()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Command {
    Explanation(CountExplanationCommand),
    Classification(CountClassificationCommand),
}
impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Explanation(_) => "correction_count_explain",
            Self::Classification(_) => "correction_count_classify",
        }
    }
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, own: HubRecordId) -> [u8; 32] {
        match self {
            Self::Explanation(command) => command.fingerprint(hub, actor, own),
            Self::Classification(command) => command.fingerprint(hub, actor, own),
        }
    }
    pub fn cuts(&self) -> (HubRevision, HubRevision) {
        match self {
            Self::Explanation(command) => (
                command.facts.expected_stock_revision,
                command.facts.expected_catalog_revision,
            ),
            Self::Classification(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
        }
    }
    pub fn request(&self) -> harvestcircle_application::hub_submission::HubSubmittedRequest<'_> {
        use harvestcircle_application::hub_submission::HubSubmittedRequest;
        match self {
            Self::Explanation(command) => HubSubmittedRequest::CountExplanation(command),
            Self::Classification(command) => HubSubmittedRequest::CountClassification(command),
        }
    }
}
pub(super) fn encode(command: &Command) -> Result<Vec<u8>, SafeError> {
    let data = match command {
        Command::Explanation(command) => {
            let f = &command.facts;
            Description::Explanation {
                schema: 1,
                report: f.report.to_hex(),
                reconciliation: f.count_reconciliation.to_hex(),
                stock_revision: f.expected_stock_revision.value(),
                catalog_revision: f.expected_catalog_revision.value(),
                previous: f.expected_prior.map(HubRecordId::to_hex),
                packs: f.packs.value(),
                recipient: f.recipient.to_hex(),
                occurrence: Clock::from_domain(f.occurred),
                source_operation: command.source_operation.to_hex(),
                source_group: command.source_group.to_hex(),
                source_evidence: f.source_evidence.as_str().into(),
                quantity_evidence: f.quantity_evidence.as_str().into(),
                recipient_evidence: f.recipient_evidence.as_str().into(),
                timing_evidence: f.timing_evidence.as_str().into(),
            }
        }
        Command::Classification(command) => Description::Classification {
            schema: 1,
            observation: command.observation_operation.to_hex(),
            stock_revision: command.expected_stock_revision.value(),
            catalog_revision: command.expected_catalog_revision.value(),
            account_previous: command.expected_account_prior.map(HubRecordId::to_hex),
            previous: command.expected_prior.map(HubRecordId::to_hex),
        },
    };
    let bytes = serde_json::to_vec(&data).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(stock::capacity());
    }
    Ok(bytes)
}
pub(super) fn decode(bytes: &[u8]) -> Result<Command, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let data: Description = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    let id = super::additional_payload::id;
    let revision = |n| HubRevision::from_value(n).map_err(|_| corrupt_storage());
    let note = |s| SourceProvenance::new(s).map_err(|_| corrupt_storage());
    let command = match data {
        Description::Explanation {
            schema: 1,
            report,
            reconciliation,
            stock_revision,
            catalog_revision,
            previous,
            packs,
            recipient,
            occurrence,
            source_operation,
            source_group,
            source_evidence,
            quantity_evidence,
            recipient_evidence,
            timing_evidence,
        } => Command::Explanation(CountExplanationCommand {
            facts: CountExplanationFacts {
                report: id(&report)?,
                count_reconciliation: id(&reconciliation)?,
                expected_stock_revision: revision(stock_revision)?,
                expected_catalog_revision: revision(catalog_revision)?,
                expected_prior: previous.as_deref().map(id).transpose()?,
                packs: WholePacks::from_value(packs).map_err(|_| corrupt_storage())?,
                recipient: id(&recipient)?,
                occurred: occurrence.domain()?,
                source_evidence: note(source_evidence)?,
                quantity_evidence: note(quantity_evidence)?,
                recipient_evidence: note(recipient_evidence)?,
                timing_evidence: note(timing_evidence)?,
            },
            source_operation: id(&source_operation)?,
            source_group: id(&source_group)?,
        }),
        Description::Classification {
            schema: 1,
            observation,
            stock_revision,
            catalog_revision,
            account_previous,
            previous,
        } => Command::Classification(CountClassificationCommand {
            observation_operation: id(&observation)?,
            expected_stock_revision: revision(stock_revision)?,
            expected_catalog_revision: revision(catalog_revision)?,
            expected_account_prior: account_previous.as_deref().map(id).transpose()?,
            expected_prior: previous.as_deref().map(id).transpose()?,
        }),
        _ => return Err(corrupt_storage()),
    };
    if encode(&command)?.as_slice() != bytes {
        return Err(corrupt_storage());
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u8) -> HubRecordId {
        HubRecordId::from_bytes([n; 16]).unwrap()
    }
    fn commands() -> Vec<Command> {
        let note =
            || SourceProvenance::new("Independent fictional committed count evidence").unwrap();
        vec![
            Command::Explanation(CountExplanationCommand {
                facts: CountExplanationFacts {
                    report: id(1),
                    count_reconciliation: id(2),
                    expected_stock_revision: HubRevision::from_value(9).unwrap(),
                    expected_catalog_revision: HubRevision::from_value(8).unwrap(),
                    expected_prior: Some(id(3)),
                    packs: WholePacks::from_value(2).unwrap(),
                    recipient: id(4),
                    occurred: Occurrence::Interval {
                        earliest: UnixTimestamp::from_seconds(100).unwrap(),
                        latest: UnixTimestamp::from_seconds(110).unwrap(),
                    },
                    source_evidence: note(),
                    quantity_evidence: note(),
                    recipient_evidence: note(),
                    timing_evidence: note(),
                },
                source_operation: id(5),
                source_group: id(6),
            }),
            Command::Classification(CountClassificationCommand {
                observation_operation: id(7),
                expected_stock_revision: HubRevision::from_value(12).unwrap(),
                expected_catalog_revision: HubRevision::from_value(8).unwrap(),
                expected_account_prior: Some(id(8)),
                expected_prior: Some(id(9)),
            }),
        ]
    }
    #[test]
    fn count_descriptions_round_trip_closed_primitives_and_preserve_independent_heads() {
        for command in commands() {
            let bytes = encode(&command).unwrap();
            assert_eq!(decode(&bytes).unwrap(), command);
            let hub = HubId::from_bytes([10; 16]).unwrap();
            let actor = crate::hub::stock::tests::owner();
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded.kind(), command.kind());
            assert_eq!(decoded.cuts(), command.cuts());
            assert_eq!(
                decoded.fingerprint(hub, actor, id(11)),
                command.fingerprint(hub, actor, id(11))
            );
            assert_eq!(
                decoded.request().archive().unwrap().as_bytes(),
                command.request().archive().unwrap().as_bytes()
            );
        }
    }
    #[test]
    fn count_descriptions_reject_alternate_unknown_oversize_and_invalid_primitive_forms() {
        for command in commands() {
            let bytes = encode(&command).unwrap();
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(decode(text.replace("\"schema\":1", "\"schema\":2").as_bytes()).is_err());
            let changed = format!("{},\"future\":false}}", &text[..text.len() - 1]);
            assert!(decode(changed.as_bytes()).is_err());
            assert!(
                decode(
                    &serde_json::to_vec_pretty(
                        &serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
                    )
                    .unwrap()
                )
                .is_err()
            );
            assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        }
        let original = String::from_utf8(encode(&commands()[0]).unwrap()).unwrap();
        for (before, after) in [
            ("\"packs\":2".to_owned(), "\"packs\":1000001".to_owned()),
            (
                format!("\"recipient\":\"{}\"", id(4).to_hex()),
                "\"recipient\":\"00000000000000000000000000000000\"".to_owned(),
            ),
            (
                "\"earliest\":100,\"latest\":110".to_owned(),
                "\"earliest\":110,\"latest\":100".to_owned(),
            ),
            (
                "\"stock_revision\":9".to_owned(),
                "\"stock_revision\":0".to_owned(),
            ),
        ] {
            assert_eq!(original.matches(&before).count(), 1);
            // Preserve canonical field order/spacing so rejection exercises
            // the changed primitive, independently of the encoding guard.
            assert!(decode(original.replacen(&before, &after, 1).as_bytes()).is_err());
        }
        assert!(decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
}
