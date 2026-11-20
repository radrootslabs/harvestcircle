//! Closed bounded native descriptions, never a decoder for archived permission.
use super::*;
use harvestcircle_domain::hub::{
    corrections::CountReturnAssociationCommand, reference::SourceProvenance,
};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Description {
    schema: u8,
    return_operation: String,
    explanation_operation: String,
    stock_revision: u64,
    catalog_revision: u64,
    account_previous: Option<String>,
    returns_previous: Option<String>,
    source_operation: String,
    source_group: String,
    source_evidence: String,
    quantity_evidence: String,
    timing_evidence: String,
}

pub(super) fn encode(command: &CountReturnAssociationCommand) -> Result<Vec<u8>, SafeError> {
    let data = Description {
        schema: 1,
        return_operation: command.return_operation.to_hex(),
        explanation_operation: command.explanation_operation.to_hex(),
        stock_revision: command.expected_stock_revision.value(),
        catalog_revision: command.expected_catalog_revision.value(),
        account_previous: command.expected_account_prior.map(HubRecordId::to_hex),
        returns_previous: command.expected_returns_prior.map(HubRecordId::to_hex),
        source_operation: command.source_operation.to_hex(),
        source_group: command.source_group.to_hex(),
        source_evidence: command.source_evidence.as_str().into(),
        quantity_evidence: command.quantity_evidence.as_str().into(),
        timing_evidence: command.timing_evidence.as_str().into(),
    };
    let bytes = serde_json::to_vec(&data).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(stock::capacity());
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> Result<CountReturnAssociationCommand, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let data: Description = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if data.schema != 1 {
        return Err(corrupt_storage());
    }
    let id = |s: &str| HubRecordId::from_hex(s).map_err(|_| corrupt_storage());
    let optional = |s: Option<String>| s.as_deref().map(id).transpose();
    let evidence = |s: String| SourceProvenance::new(s).map_err(|_| corrupt_storage());
    let command = CountReturnAssociationCommand {
        return_operation: id(&data.return_operation)?,
        explanation_operation: id(&data.explanation_operation)?,
        expected_stock_revision: HubRevision::from_value(data.stock_revision)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: HubRevision::from_value(data.catalog_revision)
            .map_err(|_| corrupt_storage())?,
        expected_account_prior: optional(data.account_previous)?,
        expected_returns_prior: optional(data.returns_previous)?,
        source_operation: id(&data.source_operation)?,
        source_group: id(&data.source_group)?,
        source_evidence: evidence(data.source_evidence)?,
        quantity_evidence: evidence(data.quantity_evidence)?,
        timing_evidence: evidence(data.timing_evidence)?,
    };
    if encode(&command)?.as_slice() != bytes {
        return Err(corrupt_storage());
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command() -> CountReturnAssociationCommand {
        let id = |n| HubRecordId::from_bytes([n; 16]).unwrap();
        let evidence = |s| SourceProvenance::new(s).unwrap();
        CountReturnAssociationCommand {
            return_operation: id(1),
            explanation_operation: id(2),
            expected_stock_revision: HubRevision::from_value(12).unwrap(),
            expected_catalog_revision: HubRevision::from_value(8).unwrap(),
            expected_account_prior: Some(id(3)),
            expected_returns_prior: Some(id(4)),
            source_operation: id(5),
            source_group: id(6),
            source_evidence: evidence("Independent fictional source evidence"),
            quantity_evidence: evidence("Whole original restricted intake independently counted"),
            timing_evidence: evidence("Occurrence remains separately known or uncertain"),
        }
    }

    #[test]
    fn native_description_preserves_every_command_field_and_explicit_missing_heads() {
        let mut c = command();
        assert_eq!(decode(&encode(&c).unwrap()).unwrap(), c);
        c.expected_account_prior = None;
        c.expected_returns_prior = None;
        assert_eq!(decode(&encode(&c).unwrap()).unwrap(), c);
    }

    #[test]
    fn noncanonical_unknown_fields_schema_duplicate_fields_and_invalid_bounds_fail_closed() {
        let bytes = encode(&command()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (name, replacement) in [
            ("schema", serde_json::json!(2)),
            ("stock_revision", serde_json::json!(0)),
            ("catalog_revision", serde_json::json!(0)),
            (
                "return_operation",
                serde_json::json!("00000000000000000000000000000000"),
            ),
            ("source_evidence", serde_json::json!("")),
            ("timing_evidence", serde_json::json!("x".repeat(513))),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut bad = value.clone();
            bad[name] = replacement;
            assert!(
                decode(&serde_json::to_vec(&bad).unwrap()).is_err(),
                "{name}"
            );
        }
        let text = String::from_utf8(bytes.clone()).unwrap();
        let duplicate = text.replacen("{", "{\"schema\":1,", 1);
        assert!(decode(duplicate.as_bytes()).is_err());
        assert!(decode(&serde_json::to_vec_pretty(&value).unwrap()).is_err());
        assert!(decode(&[]).is_err());
        assert!(decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
}
