//! Exact verified public listing evidence under the sole governed SQLite host.

use harvestcircle_domain::{
    AvailabilityEventVersion, AvailabilityObservation, AvailabilityUnsupportedReason,
    AvailabilityVersionView, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp,
};
use radroots_event::envelope::event_head::EventHeadCoordinate;
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::wire::{DEFAULT_RAW_JSON_MAX_BYTES, Nip01EventWire};
use radroots_event_codec::verify::verify_nip01_event;
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};

const VERSION_CAPACITY: i64 = 4096;
const TOTAL_PAYLOAD_BYTES: i64 = 134_217_728;
const ORDINARY_PAYLOAD_BYTES: i64 = 125_829_120;
const FIXED_VERSION_BYTES: usize = 92;
const RAW_IDENTIFIER_BYTES: usize = 4096;
const SOURCE_BYTES: usize = 2048;
const ADMISSION_BYTES: usize = 64;
const REJECTION_BYTES: usize = 64;

// Every byte-bearing column is projected before any Rust hydration or decoding.
// The retained full length distinguishes a bounded prefix from complete evidence.
pub(crate) const SELECT_AVAILABILITY_VERSION_SQL: &str = "SELECT \
    substr(event_id, 1, 33) AS event_id, length(event_id) AS event_id_bytes, \
    substr(author, 1, 33) AS author, length(author) AS author_bytes, kind, \
    substr(CAST(raw_d AS BLOB), 1, 4097) AS raw_d, \
    length(CAST(raw_d AS BLOB)) AS raw_d_bytes, \
    substr(signed_at, 1, 9) AS signed_at, length(signed_at) AS signed_at_bytes, \
    substr(published_at, 1, 9) AS published_at, length(published_at) AS published_at_bytes, \
    substr(CAST(original_json AS BLOB), 1, 262145) AS original_json, \
    length(CAST(original_json AS BLOB)) AS original_json_bytes, \
    substr(CAST(admission_label AS BLOB), 1, 65) AS admission_label, \
    length(CAST(admission_label AS BLOB)) AS admission_label_bytes, \
    substr(CAST(rejection_code AS BLOB), 1, 65) AS rejection_code, \
    length(CAST(rejection_code AS BLOB)) AS rejection_code_bytes, \
    substr(CAST(source AS BLOB), 1, 2049) AS source, \
    length(CAST(source AS BLOB)) AS source_bytes, observed_at_unix_s, payload_bytes \
    FROM availability_versions WHERE event_id = ? LIMIT 2";

impl Database {
    /// Retains the first exact signed wire and named provenance for a verified version.
    ///
    /// Public evidence does not install an account or authorize a local signer.
    /// Duplicate IDs preserve their original evidence without consuming more quota.
    pub async fn retain_availability_version(
        &self,
        view: AvailabilityVersionView,
    ) -> Result<(), SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move { retain_availability_version_on(transaction, view).await })
            })
            .await
            .map_err(map_transaction_error)
    }

    /// Loads bounded original wire and re-verifies it with the selected shared codec.
    ///
    /// Missing evidence returns `None`. Corrupt evidence or global accounting
    /// fails closed without resetting state or constructing a verified substitute.
    pub async fn load_availability_version(
        &self,
        version: AvailabilityEventVersion,
    ) -> Result<Option<AvailabilityVersionView>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    validate_public_usage(transaction).await?;
                    let row = sqlx::query(SELECT_AVAILABILITY_VERSION_SQL)
                        .bind(version.event_id().as_bytes().as_slice())
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(|_| corrupt_storage())?;
                    row.map(decode_availability_row).transpose()
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

/// The real insertion path, also exercised inside governed rollback fixtures.
pub(crate) async fn retain_availability_version_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    view: AvailabilityVersionView,
) -> Result<(), SafeError> {
    let (count, bytes) = validate_public_usage(transaction).await?;
    let existing = sqlx::query(SELECT_AVAILABILITY_VERSION_SQL)
        .bind(view.version().event_id().as_bytes().as_slice())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| corrupt_storage())?;
    if let Some(row) = existing {
        let existing = decode_availability_row(row)?;
        // Signature-verified ID correlation binds signed data. Original JSON
        // formatting, signature variants, extras and later provenance can differ.
        if existing.version() != view.version()
            || existing.publisher() != view.publisher()
            || existing.created_at() != view.created_at()
            || existing.raw_coordinate() != view.raw_coordinate()
            || existing.focused() != view.focused()
            || existing.unsupported_reason() != view.unsupported_reason()
        {
            return Err(corrupt_storage());
        }
        return Ok(());
    }

    let (label, reason) = admission(&view)?;
    let charge = version_charge(&view, label, reason)?;
    let next_bytes = bytes.checked_add(charge).ok_or_else(corrupt_storage)?;
    if count >= VERSION_CAPACITY || next_bytes > ORDINARY_PAYLOAD_BYTES {
        return Err(capacity());
    }
    let reserved = sqlx::query(
        "UPDATE public_payload_usage SET version_count = version_count + 1, \
         payload_bytes = payload_bytes + ? \
         WHERE singleton = 1 AND version_count = ? AND payload_bytes = ?",
    )
    .bind(charge)
    .bind(count)
    .bind(bytes)
    .execute(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?;
    if reserved.rows_affected() != 1 {
        return Err(corrupt_storage());
    }

    let signed_at = view.created_at().as_u64().to_be_bytes();
    let published_at = view
        .focused()
        .map(|projection| projection.published_at().as_u64().to_be_bytes());
    // Borrow the owned verified view after quota reservation: no full-wire clone.
    let inserted = sqlx::query(
        "INSERT INTO availability_versions \
         (event_id, author, kind, raw_d, signed_at, published_at, original_json, \
          admission_label, rejection_code, source, observed_at_unix_s, payload_bytes) \
         VALUES (?, ?, 30402, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(view.version().event_id().as_bytes().as_slice())
    .bind(view.publisher().public_key().as_bytes().as_slice())
    .bind(identifier(&view)?)
    .bind(signed_at.as_slice())
    .bind(published_at.as_ref().map(|bytes| bytes.as_slice()))
    .bind(view.original_json())
    .bind(label)
    .bind(reason)
    .bind(view.observation().source().as_str())
    .bind(view.observation().observed_at().as_seconds())
    .bind(charge)
    .execute(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?;
    if inserted.rows_affected() != 1 {
        return Err(corrupt_storage());
    }
    Ok(())
}

async fn validate_public_usage(
    transaction: &mut ServiceSqliteTransaction<'_>,
) -> Result<(i64, i64), SafeError> {
    let rows = sqlx::query(
        "SELECT singleton, version_count, payload_bytes FROM public_payload_usage LIMIT 2",
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| corrupt_storage())?;
    let [row] = rows.as_slice() else {
        return Err(corrupt_storage());
    };
    let singleton: i64 = row.try_get("singleton").map_err(|_| corrupt_storage())?;
    let count: i64 = row
        .try_get("version_count")
        .map_err(|_| corrupt_storage())?;
    let bytes: i64 = row
        .try_get("payload_bytes")
        .map_err(|_| corrupt_storage())?;
    if singleton != 1
        || !(0..=VERSION_CAPACITY).contains(&count)
        || !(0..=TOTAL_PAYLOAD_BYTES).contains(&bytes)
    {
        return Err(corrupt_storage());
    }
    // LIMIT bounds corruption inspection as well as normal state. The sums
    // operate on byte lengths and integers without hydrating retained wire.
    let actual = sqlx::query(
        "SELECT count(*) AS actual_count, \
         coalesce(sum(payload_bytes), 0) AS recorded_bytes, \
         coalesce(sum(actual_charge), 0) AS actual_bytes, \
         coalesce(max(CASE WHEN payload_bytes = actual_charge THEN 0 ELSE 1 END), 0) AS mismatch \
         FROM (SELECT payload_bytes, 92 + length(CAST(original_json AS BLOB)) \
             + length(CAST(raw_d AS BLOB)) + length(CAST(source AS BLOB)) \
             + length(CAST(admission_label AS BLOB)) \
             + coalesce(length(CAST(rejection_code AS BLOB)), 0) AS actual_charge \
             FROM availability_versions LIMIT 4097)",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| corrupt_storage())?;
    let actual_count: i64 = actual
        .try_get("actual_count")
        .map_err(|_| corrupt_storage())?;
    let recorded_bytes: i64 = actual
        .try_get("recorded_bytes")
        .map_err(|_| corrupt_storage())?;
    let actual_bytes: i64 = actual
        .try_get("actual_bytes")
        .map_err(|_| corrupt_storage())?;
    let mismatch: i64 = actual.try_get("mismatch").map_err(|_| corrupt_storage())?;
    if actual_count != count || recorded_bytes != bytes || actual_bytes != bytes || mismatch != 0 {
        return Err(corrupt_storage());
    }
    Ok((count, bytes))
}

pub(crate) fn decode_availability_row(
    row: SqliteRow,
) -> Result<AvailabilityVersionView, SafeError> {
    let event_id = exact_bytes::<32>(&row, "event_id", "event_id_bytes")?;
    let author = exact_bytes::<32>(&row, "author", "author_bytes")?;
    let signed_at = u64::from_be_bytes(exact_bytes::<8>(&row, "signed_at", "signed_at_bytes")?);
    let published_at = optional_bytes(&row, "published_at", "published_at_bytes", 8)?
        .map(|bytes| {
            bytes
                .try_into()
                .map(u64::from_be_bytes)
                .map_err(|_| corrupt_storage())
        })
        .transpose()?;
    let kind: i64 = row.try_get("kind").map_err(|_| corrupt_storage())?;
    let observed_at: i64 = row
        .try_get("observed_at_unix_s")
        .map_err(|_| corrupt_storage())?;
    let charge: i64 = row
        .try_get("payload_bytes")
        .map_err(|_| corrupt_storage())?;
    if kind != 30402 || observed_at < 0 {
        return Err(corrupt_storage());
    }
    let raw_d = text(&row, "raw_d", "raw_d_bytes", RAW_IDENTIFIER_BYTES)?;
    let source = text(&row, "source", "source_bytes", SOURCE_BYTES)?;
    let label = text(
        &row,
        "admission_label",
        "admission_label_bytes",
        ADMISSION_BYTES,
    )?;
    let reason = optional_bytes(
        &row,
        "rejection_code",
        "rejection_code_bytes",
        REJECTION_BYTES,
    )?
    .map(|bytes| String::from_utf8(bytes).map_err(|_| corrupt_storage()))
    .transpose()?;
    if reason
        .as_ref()
        .is_some_and(|code| code.is_empty() || !code.is_ascii())
    {
        return Err(corrupt_storage());
    }
    let original_json = text(
        &row,
        "original_json",
        "original_json_bytes",
        DEFAULT_RAW_JSON_MAX_BYTES,
    )?;
    let verified = verify_nip01_event(
        Nip01EventWire::parse_json_unverified(&original_json)
            .map_err(|_| corrupt_storage())?
            .into_unverified_envelope()
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let observation = AvailabilityObservation::parse(
        &source,
        UnixTimestamp::from_seconds(observed_at).ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let view = AvailabilityVersionView::from_verified(verified, &original_json, observation)
        .map_err(|_| corrupt_storage())?;
    let (expected_label, expected_reason) = admission(&view)?;
    let expected_publication = view
        .focused()
        .map(|projection| projection.published_at().as_u64());
    if view.version().event_id().as_bytes() != &event_id
        || view.publisher().public_key().as_bytes() != &author
        || view.created_at().as_u64() != signed_at
        || identifier(&view)? != raw_d
        || view.observation().source().as_str() != source
        || label != expected_label
        || reason.as_deref() != expected_reason
        || published_at != expected_publication
        || charge != version_charge(&view, expected_label, expected_reason)?
    {
        return Err(corrupt_storage());
    }
    Ok(view)
}

fn bounded_bytes(
    row: &SqliteRow,
    field: &str,
    length_field: &str,
    maximum: usize,
) -> Result<Vec<u8>, SafeError> {
    let length: i64 = row.try_get(length_field).map_err(|_| corrupt_storage())?;
    let length = usize::try_from(length).map_err(|_| corrupt_storage())?;
    if length > maximum {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get(field).map_err(|_| corrupt_storage())?;
    if bytes.len() != length {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}

fn exact_bytes<const N: usize>(
    row: &SqliteRow,
    field: &str,
    length_field: &str,
) -> Result<[u8; N], SafeError> {
    bounded_bytes(row, field, length_field, N)?
        .try_into()
        .map_err(|_| corrupt_storage())
}

fn optional_bytes(
    row: &SqliteRow,
    field: &str,
    length_field: &str,
    maximum: usize,
) -> Result<Option<Vec<u8>>, SafeError> {
    let length: Option<i64> = row.try_get(length_field).map_err(|_| corrupt_storage())?;
    if length.is_some() {
        return bounded_bytes(row, field, length_field, maximum).map(Some);
    }
    let bytes: Option<Vec<u8>> = row.try_get(field).map_err(|_| corrupt_storage())?;
    if bytes.is_some() {
        return Err(corrupt_storage());
    }
    Ok(None)
}

fn text(
    row: &SqliteRow,
    field: &str,
    length_field: &str,
    maximum: usize,
) -> Result<String, SafeError> {
    String::from_utf8(bounded_bytes(row, field, length_field, maximum)?)
        .map_err(|_| corrupt_storage())
}

fn identifier(view: &AvailabilityVersionView) -> Result<&str, SafeError> {
    match view.raw_coordinate() {
        EventHeadCoordinate::Addressable {
            kind: 30402, d_tag, ..
        } => Ok(d_tag),
        _ => Err(corrupt_storage()),
    }
}

fn admission(
    view: &AvailabilityVersionView,
) -> Result<(&'static str, Option<&'static str>), SafeError> {
    match (view.focused(), view.unsupported_reason().copied()) {
        (Some(_), None) => Ok(("focused", None)),
        (None, Some(AvailabilityUnsupportedReason::Excluded(partition))) => Ok((
            match partition {
                ClassifiedListingPartition::FocusedFoodAvailability => "excluded_focused",
                ClassifiedListingPartition::OperationalListing => "excluded_operational",
                ClassifiedListingPartition::GenericNip99 => "excluded_generic",
                ClassifiedListingPartition::Ambiguous => "excluded_ambiguous",
            },
            None,
        )),
        (None, Some(AvailabilityUnsupportedReason::ProjectionRejected(code)))
            if !code.is_empty() && code.len() <= REJECTION_BYTES && code.is_ascii() =>
        {
            Ok(("projection_rejected", Some(code)))
        }
        _ => Err(corrupt_storage()),
    }
}

fn version_charge(
    view: &AvailabilityVersionView,
    label: &str,
    reason: Option<&str>,
) -> Result<i64, SafeError> {
    let identifier = identifier(view)?;
    let source = view.observation().source().as_str();
    if view.original_json().len() > DEFAULT_RAW_JSON_MAX_BYTES
        || identifier.len() > RAW_IDENTIFIER_BYTES
        || source.is_empty()
        || source.len() > SOURCE_BYTES
        || label.is_empty()
        || label.len() > ADMISSION_BYTES
        || reason
            .is_some_and(|code| code.is_empty() || code.len() > REJECTION_BYTES || !code.is_ascii())
    {
        return Err(corrupt_storage());
    }
    let bytes = [
        view.original_json().len(),
        identifier.len(),
        source.len(),
        label.len(),
        reason.map_or(0, str::len),
    ]
    .into_iter()
    .try_fold(FIXED_VERSION_BYTES, usize::checked_add)
    .ok_or_else(corrupt_storage)?;
    i64::try_from(bytes).map_err(|_| corrupt_storage())
}

const fn capacity() -> SafeError {
    SafeError::new(
        SafeErrorCode::AvailabilityCapacity,
        SafeMessage::new("The retained availability evidence is at capacity."),
    )
}
