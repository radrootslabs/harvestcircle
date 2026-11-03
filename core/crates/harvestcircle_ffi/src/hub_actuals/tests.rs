use super::*;
use crate::hub_stock::{HubOccurrenceDto, HubOccurrenceKindDto};
fn unknown() -> HubOccurrenceDto {
    HubOccurrenceDto {
        kind: HubOccurrenceKindDto::Unknown,
        start_unix_s: None,
        end_unix_s: None,
    }
}
fn report() -> HubActualReportInputDto {
    HubActualReportInputDto {
        variant_id: None,
        recipient_id: None,
        whole_packs: None,
        source_kind: HubActualSourceKindDto::Unknown,
        source_claims: vec![],
        occurrence: unknown(),
        observation: "Already-taken fact; no invented source or quantity".to_owned(),
    }
}
fn claim(n: u8) -> HubActualSourceClaimDto {
    HubActualSourceClaimDto {
        trace_id: format!("{n:02x}").repeat(16),
        group_id: None,
        location_id: None,
    }
}
#[test]
fn actual_report_transport_preserves_unknowns_future_clocks_and_closed_source_claim_shapes() {
    let original = report();
    let decoded = original.clone().command().unwrap();
    assert_eq!(HubActualReportInputDto::from(&decoded), original);
    let mut future = original.clone();
    future.occurrence = HubOccurrenceDto {
        kind: HubOccurrenceKindDto::Known,
        start_unix_s: Some(500),
        end_unix_s: None,
    };
    assert_eq!(
        HubActualReportInputDto::from(&future.clone().command().unwrap()),
        future
    );
    for kind in [
        HubActualSourceKindDto::Unknown,
        HubActualSourceKindDto::Possible,
        HubActualSourceKindDto::EstablishedClaim,
    ] {
        for count in [0, 1, 2, 64, 65] {
            let mut input = original.clone();
            input.source_kind = kind;
            input.source_claims = (1..=count).map(claim).collect();
            let valid = match kind {
                HubActualSourceKindDto::Unknown => count == 0,
                HubActualSourceKindDto::Possible => (1..=64).contains(&count),
                HubActualSourceKindDto::EstablishedClaim => count == 1,
            };
            let decoded = input.clone().command();
            assert_eq!(decoded.is_ok(), valid);
            if let Ok(decoded) = decoded {
                assert_eq!(HubActualReportInputDto::from(&decoded), input);
            }
        }
    }
    let mut duplicate = original.clone();
    duplicate.source_kind = HubActualSourceKindDto::Possible;
    duplicate.source_claims = vec![claim(1), claim(1)];
    assert!(duplicate.command().is_err());
    let mut zero = original.clone();
    zero.whole_packs = Some(0);
    assert!(zero.command().is_err());
    let mut invalid = original.clone();
    invalid.variant_id = Some("00".repeat(16));
    assert!(invalid.command().is_err());
    let mut invalid = original.clone();
    invalid.occurrence.end_unix_s = Some(1);
    assert!(invalid.command().is_err());
    let mut invalid = original;
    invalid.observation.clear();
    assert!(invalid.command().is_err());
}
#[test]
fn actual_report_pages_require_finite_bounds_and_both_native_revisions_after_first_page() {
    let query = HubActualReportQueryDto {
        expected_stock_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
        filter: HubActualReportFilterDto::Unresolved,
    };
    assert!(query.clone().decode().is_ok());
    for (offset, limit, stock, catalog) in [
        (0, 0, None, None),
        (0, 65, None, None),
        (4097, 64, Some(1), Some(1)),
        (1, 64, None, None),
        (1, 64, Some(1), None),
        (1, 64, None, Some(1)),
        (1, 64, Some(0), Some(1)),
    ] {
        assert!(
            HubActualReportQueryDto {
                offset,
                limit,
                expected_stock_revision: stock,
                expected_catalog_revision: catalog,
                ..query.clone()
            }
            .decode()
            .is_err()
        );
    }
    assert!(
        HubActualReportQueryDto {
            offset: 64,
            expected_stock_revision: Some(1),
            expected_catalog_revision: Some(2),
            ..query
        }
        .decode()
        .is_ok()
    );
}
