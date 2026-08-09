use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::compatibility::{DatabasePreflight, preflight};
use crate::db::{CURRENT_SCHEMA_VERSION, restrict_file_permissions};

type HmacSha256 = Hmac<Sha256>;
const EXPORT_DOMAIN: &[u8] = b"radroots-studio-quarantine-export-v1";
const REPAIR_DOMAIN: &[u8] = b"radroots-studio-repair-candidate-v1";

pub struct RepairAuthorization(Zeroizing<[u8; 32]>);

impl RepairAuthorization {
    /// Moves an exact 256-bit caller authorization secret into zeroizing storage.
    ///
    /// # Errors
    ///
    /// Returns a safe authorization error for every other input length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, SafeError> {
        let bytes = Zeroizing::new(bytes);
        let value = <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| unauthorized())?;
        Ok(Self(Zeroizing::new(value)))
    }

    fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

pub struct QuarantineExportReceipt {
    path: PathBuf,
    sha256: String,
    authentication_tag: String,
}

impl QuarantineExportReceipt {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[must_use]
    pub fn authentication_tag(&self) -> &str {
        &self.authentication_tag
    }
}

pub struct RepairCandidate {
    path: PathBuf,
    sha256: String,
    authentication_tag: String,
}

impl RepairCandidate {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn export_quarantined(
    source: &Path,
    destination: &Path,
    authorization: &RepairAuthorization,
) -> Result<QuarantineExportReceipt, SafeError> {
    if !matches!(preflight(source)?, DatabasePreflight::Quarantined { .. }) {
        return Err(not_quarantined());
    }
    ensure_new_destination(destination)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(source, flags).map_err(|_| storage_error())?;
    if connection.backup(MAIN_DB, destination, None).is_err() {
        let _ = fs::remove_file(destination);
        return Err(storage_error());
    }
    restrict_file_permissions(destination)?;
    File::open(destination)
        .and_then(|file| file.sync_all())
        .map_err(|_| storage_error())?;
    let sha256 = digest_file(destination)?;
    let authentication_tag = authenticate(authorization, EXPORT_DOMAIN, &sha256)?;
    Ok(QuarantineExportReceipt {
        path: destination.to_path_buf(),
        sha256,
        authentication_tag,
    })
}

pub(crate) fn authenticate_candidate(
    path: &Path,
    authorization: &RepairAuthorization,
) -> Result<RepairCandidate, SafeError> {
    if !matches!(
        preflight(path)?,
        DatabasePreflight::Ready { schema_version } if schema_version <= CURRENT_SCHEMA_VERSION
    ) {
        return Err(storage_error());
    }
    let sha256 = digest_file(path)?;
    let authentication_tag = authenticate(authorization, REPAIR_DOMAIN, &sha256)?;
    Ok(RepairCandidate {
        path: path.to_path_buf(),
        sha256,
        authentication_tag,
    })
}

pub(crate) fn install_candidate(
    target: &Path,
    candidate: &RepairCandidate,
    authorization: &RepairAuthorization,
) -> Result<(), SafeError> {
    if !matches!(preflight(target)?, DatabasePreflight::Quarantined { .. }) {
        return Err(not_quarantined());
    }
    let digest = digest_file(&candidate.path)?;
    if digest != candidate.sha256
        || authenticate(authorization, REPAIR_DOMAIN, &digest)? != candidate.authentication_tag
    {
        return Err(unauthorized());
    }
    if !matches!(preflight(&candidate.path)?, DatabasePreflight::Ready { .. }) {
        return Err(storage_error());
    }
    let parent = target.parent().ok_or_else(storage_error)?;
    let replacement = parent.join(".authenticated-repair.tmp");
    if replacement.try_exists().map_err(|_| storage_error())? {
        return Err(storage_error());
    }
    copy_secure(&candidate.path, &replacement)?;
    let retained = parent.join("studio.sqlite3.quarantined-evidence");
    if retained.try_exists().map_err(|_| storage_error())? {
        let _ = fs::remove_file(&replacement);
        return Err(storage_error());
    }
    fs::rename(target, &retained).map_err(|_| storage_error())?;
    if fs::rename(&replacement, target).is_err() {
        let _ = fs::rename(&retained, target);
        let _ = fs::remove_file(&replacement);
        return Err(storage_error());
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| storage_error())
}

fn ensure_new_destination(path: &Path) -> Result<(), SafeError> {
    if path.try_exists().map_err(|_| storage_error())? {
        return Err(storage_error());
    }
    let parent = path.parent().ok_or_else(storage_error)?;
    let metadata = fs::symlink_metadata(parent).map_err(|_| storage_error())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(storage_error());
    }
    Ok(())
}

fn copy_secure(source: &Path, destination_path: &Path) -> Result<(), SafeError> {
    let mut source = secure_read(source)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits())
                .map_err(|_| storage_error())?,
        );
    }
    let mut destination = options
        .open(destination_path)
        .map_err(|_| storage_error())?;
    std::io::copy(&mut source, &mut destination).map_err(|_| storage_error())?;
    destination.sync_all().map_err(|_| storage_error())?;
    restrict_file_permissions(destination_path)
}

fn secure_read(path: &Path) -> Result<File, SafeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| storage_error())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(storage_error());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits())
                .map_err(|_| storage_error())?,
        );
    }
    options.open(path).map_err(|_| storage_error())
}

fn digest_file(path: &Path) -> Result<String, SafeError> {
    let mut file = secure_read(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| storage_error())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex(&digest.finalize()))
}

fn authenticate(
    authorization: &RepairAuthorization,
    domain: &[u8],
    digest: &str,
) -> Result<String, SafeError> {
    let mut hmac =
        HmacSha256::new_from_slice(authorization.expose()).map_err(|_| unauthorized())?;
    hmac.update(domain);
    hmac.update(digest.as_bytes());
    Ok(hex(&hmac.finalize().into_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn unauthorized() -> SafeError {
    SafeError::new(
        SafeErrorCode::RepairUnauthorized,
        SafeMessage::new("The database repair authorization is invalid."),
    )
}

const fn not_quarantined() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The database is not in quarantine."),
    )
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The database repair operation could not be completed."),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{
        REPAIR_DOMAIN, RepairAuthorization, RepairCandidate, authenticate, authenticate_candidate,
        copy_secure, digest_file, ensure_new_destination, export_quarantined, hex,
        install_candidate, secure_read,
    };
    use crate::Database;

    fn quarantined_database(path: &std::path::Path) {
        drop(Database::open(path).expect("current database"));
        let connection = Connection::open(path).expect("open database");
        connection
            .execute(
                "INSERT INTO account_identities (public_key, npub, created_at) VALUES (?1, ?2, 1)",
                [
                    "00".repeat(32),
                    "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7".to_owned(),
                ],
            )
            .expect("invalid identity fixture");
        connection
            .execute(
                "INSERT INTO local_signer_bindings (account_public_key, binding_public_key, binding_kind, availability) VALUES (?1, ?1, 'local_secret', 'available')",
                ["00".repeat(32)],
            )
            .expect("binding fixture");
    }

    #[test]
    fn repair_authority_and_candidate_reject_invalid_states() {
        assert!(RepairAuthorization::from_bytes(vec![0_u8; 31]).is_err());
        assert!(RepairAuthorization::from_bytes(vec![0_u8; 33]).is_err());
        let authorization = RepairAuthorization::from_bytes(vec![0x41; 32]).expect("authorization");
        assert_eq!(
            authenticate(&authorization, b"domain", "digest")
                .expect("authentication tag")
                .len(),
            64
        );
        assert_eq!(hex(&[0, 15, 255]), "000fff");

        let directory = tempdir().expect("temporary directory");
        let ready = directory.path().join("ready.sqlite3");
        drop(Database::open(&ready).expect("ready database"));
        let candidate = authenticate_candidate(&ready, &authorization).expect("candidate");
        assert_eq!(candidate.path(), ready);

        let missing = directory.path().join("missing.sqlite3");
        assert!(authenticate_candidate(&missing, &authorization).is_err());
        let export = directory.path().join("export.sqlite3");
        assert!(export_quarantined(&ready, &export, &authorization).is_err());
        assert!(install_candidate(&ready, &candidate, &authorization).is_err());
    }

    #[test]
    fn repair_file_boundaries_reject_existing_non_file_and_missing_parent_paths() {
        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"repair material").expect("write regular file");
        let child = directory.path().join("child");
        fs::create_dir(&child).expect("create child directory");

        assert!(ensure_new_destination(&regular).is_err());
        assert!(ensure_new_destination(&regular.join("nested")).is_err());
        assert!(secure_read(&child).is_err());
        assert_eq!(digest_file(&regular).expect("digest").len(), 64);

        let copied = directory.path().join("copied");
        copy_secure(&regular, &copied).expect("secure copy");
        assert_eq!(fs::read(&copied).expect("copied bytes"), b"repair material");
        assert!(copy_secure(&regular, &copied).is_err());
        assert!(copy_secure(&child, &directory.path().join("invalid-copy")).is_err());
    }

    #[test]
    fn repair_installation_rejects_tampering_quarantined_candidates_and_staging_collisions() {
        let directory = tempdir().expect("temporary directory");
        let authorization = RepairAuthorization::from_bytes(vec![0x41; 32]).expect("authorization");
        let target = directory.path().join("studio.sqlite3");
        quarantined_database(&target);
        let candidate_path = directory.path().join("candidate.sqlite3");
        drop(Database::open(&candidate_path).expect("candidate database"));
        let candidate = authenticate_candidate(&candidate_path, &authorization).expect("candidate");

        fs::OpenOptions::new()
            .append(true)
            .open(&candidate_path)
            .expect("open candidate")
            .write_all(b"tamper")
            .expect("tamper candidate");
        assert!(install_candidate(&target, &candidate, &authorization).is_err());

        let quarantined_candidate_path = directory.path().join("quarantined-candidate.sqlite3");
        quarantined_database(&quarantined_candidate_path);
        let digest = digest_file(&quarantined_candidate_path).expect("candidate digest");
        let quarantined_candidate = RepairCandidate {
            path: quarantined_candidate_path,
            sha256: digest.clone(),
            authentication_tag: authenticate(&authorization, REPAIR_DOMAIN, &digest)
                .expect("candidate tag"),
        };
        assert!(install_candidate(&target, &quarantined_candidate, &authorization).is_err());

        let candidate_path = directory.path().join("candidate-two.sqlite3");
        drop(Database::open(&candidate_path).expect("candidate database"));
        let candidate = authenticate_candidate(&candidate_path, &authorization).expect("candidate");
        let replacement = directory.path().join(".authenticated-repair.tmp");
        fs::write(&replacement, b"occupied").expect("occupied replacement");
        assert!(install_candidate(&target, &candidate, &authorization).is_err());
        fs::remove_file(&replacement).expect("remove occupied replacement");

        let retained = directory.path().join("studio.sqlite3.quarantined-evidence");
        fs::write(&retained, b"occupied").expect("occupied retained evidence");
        assert!(install_candidate(&target, &candidate, &authorization).is_err());
        assert!(!replacement.exists());
    }

    #[cfg(unix)]
    #[test]
    fn repair_file_boundaries_reject_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"repair material").expect("write regular file");
        let link = directory.path().join("link");
        symlink(&regular, &link).expect("create file symlink");
        assert!(secure_read(&link).is_err());
        assert!(digest_file(&link).is_err());

        let directory_link = directory.path().join("directory-link");
        symlink(directory.path(), &directory_link).expect("create directory symlink");
        assert!(ensure_new_destination(&directory_link.join("export")).is_err());
    }
}
