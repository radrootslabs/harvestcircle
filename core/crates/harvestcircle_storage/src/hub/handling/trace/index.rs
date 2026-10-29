use super::*;
use harvestcircle_domain::hub::reference::{HubLabel, SourceCertainty, SourceCode};
use harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY;
use harvestcircle_domain::hub::trace::{TraceScope, TraceSourceEvidence};
use sqlx::{Row, sqlite::SqliteRow};

impl Database {
    pub async fn hub_trace_page(
        &self,
        actor: PublicKey,
        query: HubTraceQuery,
    ) -> Result<HubTracePage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let (hub, state, catalog) = context_on(
                        tx,
                        actor,
                        query.expected_revision,
                        query.expected_catalog_revision,
                    )
                    .await?;
                    let selected = candidates_on(tx, hub, &query.selection).await?;
                    let total = u32::try_from(selected.len()).map_err(|_| corrupt_storage())?;
                    if query.offset > total {
                        return Err(invalid_input());
                    }
                    let mut rows = Vec::new();
                    for id in selected
                        .into_iter()
                        .skip(query.offset as usize)
                        .take(query.limit as usize)
                    {
                        let snapshot =
                            super::snapshot::load_on(tx, hub, &state, &catalog, id).await?;
                        if !query
                            .selection
                            .includes(&snapshot.trace)
                            .map_err(|_| corrupt_storage())?
                        {
                            return Err(corrupt_storage());
                        }
                        rows.push(HubTraceRow {
                            balance: snapshot.balance().map_err(|_| corrupt_storage())?,
                            trace: snapshot.trace,
                        });
                    }
                    Ok(HubTracePage {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        selection: query.selection,
                        offset: query.offset,
                        total,
                        more: query.offset
                            + u32::try_from(rows.len()).map_err(|_| corrupt_storage())?
                            < total,
                        rows,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
async fn candidates_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    selection: &TraceSelection,
) -> Result<Vec<HubRecordId>, SafeError> {
    if let TraceScope::SourceImpact(id) = selection.scope {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM hub_source_references WHERE hub_id=? AND source_id=?",
        )
        .bind(hub.as_bytes().as_slice())
        .bind(id.as_bytes().as_slice())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
        if n != 1 {
            return Err(invalid_input());
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_receipts")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if !(0..=HUB_TRACE_CAPACITY as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    // One bounded original-field scan; no N+1 deep-reader traversal merely to
    // filter candidates. Selected populations use the complete original readers.
    let candidates=sqlx::query(concat!(
        "SELECT substr(r.trace_id,1,17) AS trace_id,substr(r.source_id,1,17) AS source_id,substr(r.source_certainty,1,17) AS certainty,substr(s.issuer_id,1,17) AS issuer_id,",
        "substr(CAST(p.label AS BLOB),1,121) AS product_label,substr(CAST(v.label AS BLOB),1,121) AS variant_label,substr(CAST(su.label AS BLOB),1,121) AS supplier_label,",
        "substr(CAST(pr.label AS BLOB),1,121) AS producer_label,substr(CAST(si.label AS BLOB),1,121) AS issuer_label,substr(CAST(s.source_code AS BLOB),1,129) AS source_code,",
        "CASE WHEN o.operation_id IS NULL OR o.hub_id<>? OR o.kind<>'receipt' OR p.record_id IS NULL OR p.kind<>'product' OR p.hub_id<>o.hub_id OR v.record_id IS NULL OR v.kind<>'pack' OR v.hub_id<>o.hub_id OR ",
        "(r.supplier_id IS NOT NULL AND (su.record_id IS NULL OR su.kind<>'party' OR su.hub_id<>o.hub_id)) OR (r.producer_id IS NOT NULL AND (pr.record_id IS NULL OR pr.kind<>'party' OR pr.hub_id<>o.hub_id)) OR ",
        "(r.source_id IS NOT NULL AND (s.source_id IS NULL OR s.hub_id<>o.hub_id OR (s.issuer_id IS NOT NULL AND (si.record_id IS NULL OR si.kind<>'party' OR si.hub_id<>o.hub_id)))) THEN 1 ELSE 0 END AS invalid ",
        "FROM hub_receipts r LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id LEFT JOIN hub_source_references s ON s.source_id=r.source_id ",
        "LEFT JOIN hub_reference_versions p ON p.record_id=r.product_id AND p.record_revision=r.product_revision LEFT JOIN hub_reference_versions v ON v.record_id=r.variant_id AND v.record_revision=r.variant_revision ",
        "LEFT JOIN hub_reference_versions su ON su.record_id=r.supplier_id AND su.record_revision=r.supplier_revision LEFT JOIN hub_reference_versions pr ON pr.record_id=r.producer_id AND pr.record_revision=r.producer_revision ",
        "LEFT JOIN hub_reference_versions si ON si.record_id=s.issuer_id AND si.record_revision=r.source_issuer_revision ORDER BY r.trace_id LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if candidates.len() != count as usize {
        return Err(corrupt_storage());
    }
    let mut selected = Vec::new();
    let mut found_exact = false;
    for row in candidates {
        if row
            .try_get::<i64, _>("invalid")
            .map_err(|_| corrupt_storage())?
            != 0
        {
            return Err(corrupt_storage());
        }
        let trace = stock::read::record_id(&row, "trace_id")?;
        if selection.scope == TraceScope::Receipt(trace) {
            found_exact = true;
        }
        let source_id = stock::read::optional_id(&row, "source_id")?;
        let issuer = stock::read::optional_id(&row, "issuer_id")?;
        let certainty: Option<String> = row.try_get("certainty").map_err(|_| corrupt_storage())?;
        let code: Option<String> = optional_text(&row, "source_code")?;
        let source = match (source_id, certainty.as_deref()) {
            (None, None) if issuer.is_none() && code.is_none() => TraceSourceEvidence::Unknown,
            (Some(source), Some(certainty)) => {
                let certainty = match certainty {
                    "possible" => SourceCertainty::ReportedPossible,
                    "established" => SourceCertainty::Established,
                    _ => return Err(corrupt_storage()),
                };
                SourceCode::new(code.clone().ok_or_else(corrupt_storage)?)
                    .map_err(|_| corrupt_storage())?;
                TraceSourceEvidence::Referenced {
                    source,
                    certainty,
                    issuer,
                }
            }
            _ => return Err(corrupt_storage()),
        };
        let mut fields = vec![trace.to_hex()];
        for field in [
            "product_label",
            "variant_label",
            "supplier_label",
            "producer_label",
            "issuer_label",
        ] {
            if let Some(text) = optional_text(&row, field)? {
                HubLabel::new(text.clone()).map_err(|_| corrupt_storage())?;
                fields.push(text);
            } else if matches!(field, "product_label" | "variant_label") {
                return Err(corrupt_storage());
            }
        }
        if let Some(code) = code {
            fields.push(code);
        }
        if let Some(source) = source_id {
            fields.push(source.to_hex());
        }
        let fields = fields.iter().map(String::as_str).collect::<Vec<_>>();
        if selection
            .includes_fields(trace, source, &fields)
            .map_err(|_| corrupt_storage())?
        {
            selected.push(trace);
        }
    }
    if matches!(selection.scope, TraceScope::Receipt(_)) && !found_exact {
        return Err(invalid_input());
    }
    Ok(selected)
}
fn optional_text(row: &SqliteRow, field: &str) -> Result<Option<String>, SafeError> {
    let bytes: Option<Vec<u8>> = row.try_get(field).map_err(|_| corrupt_storage())?;
    bytes
        .map(|v| String::from_utf8(v).map_err(|_| corrupt_storage()))
        .transpose()
}
