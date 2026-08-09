use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::db::{restrict_directory_permissions, restrict_file_permissions};

type HmacSha256 = Hmac<Sha256>;

const RECOVERY_DIRECTORY_SUFFIX: &str = "recovery";
const AUTHENTICATION_KEY_FILENAME: &str = "authentication-key-v1";
const MANIFEST_FORMAT: &str = "radroots-studio-migration-recovery-v1";

pub(crate) struct MigrationRecovery {
    directory: PathBuf,
    backup: PathBuf,
    marker: PathBuf,
    source_schema: u32,
    target_schema: u32,
    digest: String,
    tag: String,
    state: String,
}

impl MigrationRecovery {
    pub(crate) fn prepare(
        database_path: &Path,
        source_schema: u32,
        target_schema: u32,
    ) -> Result<Self, SafeError> {
        let directory = recovery_directory(database_path)?;
        create_recovery_directory(&directory)?;
        let key = load_or_create_authentication_key(&directory)?;
        let stem = format!("migration-v{source_schema}-to-v{target_schema}");
        let backup = directory.join(format!("{stem}.sqlite3"));
        let marker = directory.join(format!("{stem}.marker"));

        if marker.try_exists().map_err(|_| storage_error())? {
            let mut recovery = Self::load_existing(
                directory,
                backup,
                marker,
                source_schema,
                target_schema,
                &key,
            )?;
            if recovery.state == "complete" {
                recovery.tag = authentication_tag(
                    &key,
                    source_schema,
                    target_schema,
                    &recovery.digest,
                    "prepared",
                )?;
                recovery.state = "prepared".to_owned();
                recovery.write_marker("prepared", &key)?;
            }
            return Ok(recovery);
        }
        if backup.try_exists().map_err(|_| storage_error())? {
            return Err(backup_invalid());
        }

        create_verified_backup(database_path, &backup)?;
        let digest = file_digest(&backup)?;
        let tag = authentication_tag(&key, source_schema, target_schema, &digest, "prepared")?;
        let recovery = Self {
            directory,
            backup,
            marker,
            source_schema,
            target_schema,
            digest,
            tag,
            state: "prepared".to_owned(),
        };
        recovery.write_marker("prepared", &key)?;
        recovery.verify_backup(&key, "prepared")?;
        Ok(recovery)
    }

    pub(crate) fn finish(self, current_schema: u32) -> Result<(), SafeError> {
        if current_schema != self.target_schema {
            return Err(backup_invalid());
        }
        let key = load_authentication_key(&self.directory)?;
        self.verify_backup(&key, "prepared")?;
        self.write_marker("complete", &key)
    }

    pub(crate) fn verify_evidence(
        database_path: &Path,
        source_schema: u32,
        target_schema: u32,
    ) -> Result<(), SafeError> {
        let directory = recovery_directory(database_path)?;
        let key = load_authentication_key(&directory)?;
        let stem = format!("migration-v{source_schema}-to-v{target_schema}");
        Self::load_existing(
            directory.clone(),
            directory.join(format!("{stem}.sqlite3")),
            directory.join(format!("{stem}.marker")),
            source_schema,
            target_schema,
            &key,
        )
        .map(|_| ())
    }

    pub(crate) fn restore(
        database_path: &Path,
        source_schema: u32,
        target_schema: u32,
    ) -> Result<(), SafeError> {
        let directory = recovery_directory(database_path)?;
        let key = load_authentication_key(&directory)?;
        let stem = format!("migration-v{source_schema}-to-v{target_schema}");
        let recovery = Self::load_existing(
            directory.clone(),
            directory.join(format!("{stem}.sqlite3")),
            directory.join(format!("{stem}.marker")),
            source_schema,
            target_schema,
            &key,
        )?;
        recovery.verify_backup(&key, &recovery.state)?;
        replace_with_backup(database_path, &recovery.backup)
    }

    fn load_existing(
        directory: PathBuf,
        backup: PathBuf,
        marker: PathBuf,
        source_schema: u32,
        target_schema: u32,
        key: &[u8],
    ) -> Result<Self, SafeError> {
        let manifest = read_bounded_file(&marker, 4_096)?;
        let manifest = std::str::from_utf8(&manifest).map_err(|_| backup_invalid())?;
        let mut lines = manifest.lines();
        if lines.next() != Some(MANIFEST_FORMAT)
            || parse_field(&mut lines, "source_schema")? != source_schema.to_string()
            || parse_field(&mut lines, "target_schema")? != target_schema.to_string()
            || parse_field(&mut lines, "backup")?
                != backup
                    .file_name()
                    .ok_or_else(backup_invalid)?
                    .to_string_lossy()
            || lines.clone().count() != 3
        {
            return Err(backup_invalid());
        }
        let digest = parse_field(&mut lines, "sha256")?;
        let state = parse_field(&mut lines, "state")?;
        let tag = parse_field(&mut lines, "hmac_sha256")?;
        if !matches!(state.as_str(), "prepared" | "complete") {
            return Err(backup_invalid());
        }
        let recovery = Self {
            directory,
            backup,
            marker,
            source_schema,
            target_schema,
            digest,
            tag,
            state,
        };
        recovery.verify_backup(key, &recovery.state)?;
        Ok(recovery)
    }

    fn verify_backup(&self, key: &[u8], state: &str) -> Result<(), SafeError> {
        if file_digest(&self.backup)? != self.digest {
            return Err(backup_invalid());
        }
        let expected = authentication_tag(
            key,
            self.source_schema,
            self.target_schema,
            &self.digest,
            state,
        )?;
        let expected = decode_hex_32(&expected)?;
        let actual = decode_hex_32(&self.tag)?;
        if !constant_time_eq(&expected, &actual) {
            return Err(backup_invalid());
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let connection =
            Connection::open_with_flags(&self.backup, flags).map_err(|_| backup_invalid())?;
        let integrity: String = connection
            .pragma_query_value(None, "quick_check", |row| row.get(0))
            .map_err(|_| backup_invalid())?;
        if integrity != "ok" {
            return Err(backup_invalid());
        }
        Ok(())
    }

    fn write_marker(&self, state: &str, key: &[u8]) -> Result<(), SafeError> {
        let tag = authentication_tag(
            key,
            self.source_schema,
            self.target_schema,
            &self.digest,
            state,
        )?;
        let content = format!(
            "{MANIFEST_FORMAT}\nsource_schema={}\ntarget_schema={}\nbackup={}\nsha256={}\nstate={state}\nhmac_sha256={tag}\n",
            self.source_schema,
            self.target_schema,
            self.backup
                .file_name()
                .ok_or_else(backup_invalid)?
                .to_string_lossy(),
            self.digest,
        );
        atomic_secure_write(&self.marker, content.as_bytes())
    }
}

fn recovery_directory(database_path: &Path) -> Result<PathBuf, SafeError> {
    let filename = database_path
        .file_name()
        .ok_or_else(storage_error)?
        .to_string_lossy();
    Ok(database_path.with_file_name(format!("{filename}.{RECOVERY_DIRECTORY_SUFFIX}")))
}

fn create_recovery_directory(directory: &Path) -> Result<(), SafeError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(storage_error());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(directory).map_err(|_| storage_error())?;
        }
        Err(_) => return Err(storage_error()),
    }
    restrict_directory_permissions(directory)
}

fn load_or_create_authentication_key(directory: &Path) -> Result<Zeroizing<Vec<u8>>, SafeError> {
    let path = directory.join(AUTHENTICATION_KEY_FILENAME);
    if path.try_exists().map_err(|_| storage_error())? {
        return load_authentication_key(directory);
    }
    let mut key = Zeroizing::new(vec![0_u8; 32]);
    getrandom::getrandom(&mut key).map_err(|_| storage_error())?;
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
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(&key).map_err(|_| storage_error())?;
            file.sync_all().map_err(|_| storage_error())?;
            restrict_file_permissions(&path)?;
            Ok(key)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            load_authentication_key(directory)
        }
        Err(_) => Err(storage_error()),
    }
}

fn load_authentication_key(directory: &Path) -> Result<Zeroizing<Vec<u8>>, SafeError> {
    let path = directory.join(AUTHENTICATION_KEY_FILENAME);
    let key = read_bounded_file(&path, 32)?;
    if key.len() != 32 {
        return Err(backup_invalid());
    }
    Ok(Zeroizing::new(key))
}

fn create_verified_backup(source: &Path, destination: &Path) -> Result<(), SafeError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(source, flags).map_err(|_| backup_invalid())?;
    connection
        .backup(MAIN_DB, destination, None)
        .map_err(|_| backup_invalid())?;
    restrict_file_permissions(destination)?;
    File::open(destination)
        .and_then(|file| file.sync_all())
        .map_err(|_| backup_invalid())
}

fn replace_with_backup(database_path: &Path, backup: &Path) -> Result<(), SafeError> {
    let parent = database_path.parent().ok_or_else(storage_error)?;
    let mut suffix = [0_u8; 8];
    getrandom::getrandom(&mut suffix).map_err(|_| storage_error())?;
    let replacement = parent.join(format!(".database-restore-{}.tmp", hex(&suffix)));
    let displaced = parent.join(format!(".database-displaced-{}.sqlite3", hex(&suffix)));
    let mut source = secure_read(backup)?;
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
    let result = (|| {
        let mut destination = options.open(&replacement).map_err(|_| storage_error())?;
        std::io::copy(&mut source, &mut destination).map_err(|_| storage_error())?;
        destination.sync_all().map_err(|_| storage_error())?;
        restrict_file_permissions(&replacement)?;
        fs::rename(database_path, &displaced).map_err(|_| storage_error())?;
        if fs::rename(&replacement, database_path).is_err() {
            let _ = fs::rename(&displaced, database_path);
            return Err(storage_error());
        }
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| storage_error())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&replacement);
    }
    result
}

fn secure_read(path: &Path) -> Result<File, SafeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| backup_invalid())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(backup_invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits())
                .map_err(|_| backup_invalid())?,
        );
    }
    options.open(path).map_err(|_| backup_invalid())
}

fn file_digest(path: &Path) -> Result<String, SafeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| backup_invalid())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(backup_invalid());
    }
    let mut file = secure_read(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| backup_invalid())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex(&digest.finalize()))
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, SafeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| backup_invalid())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || usize::try_from(metadata.len()).map_err(|_| backup_invalid())? > limit
    {
        return Err(backup_invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits())
                .map_err(|_| backup_invalid())?,
        );
    }
    let file = options.open(path).map_err(|_| backup_invalid())?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(u64::try_from(limit).map_err(|_| backup_invalid())? + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| backup_invalid())?;
    if bytes.len() > limit {
        return Err(backup_invalid());
    }
    Ok(bytes)
}

fn atomic_secure_write(path: &Path, bytes: &[u8]) -> Result<(), SafeError> {
    let parent = path.parent().ok_or_else(storage_error)?;
    let mut suffix = [0_u8; 8];
    getrandom::getrandom(&mut suffix).map_err(|_| storage_error())?;
    let temporary = parent.join(format!(".marker-{}.tmp", hex(&suffix)));
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
    let result = (|| {
        let mut file = options.open(&temporary).map_err(|_| storage_error())?;
        file.write_all(bytes).map_err(|_| storage_error())?;
        file.sync_all().map_err(|_| storage_error())?;
        restrict_file_permissions(&temporary)?;
        fs::rename(&temporary, path).map_err(|_| storage_error())?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| storage_error())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn authentication_tag(
    key: &[u8],
    source_schema: u32,
    target_schema: u32,
    digest: &str,
    state: &str,
) -> Result<String, SafeError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| backup_invalid())?;
    mac.update(MANIFEST_FORMAT.as_bytes());
    mac.update(&source_schema.to_be_bytes());
    mac.update(&target_schema.to_be_bytes());
    mac.update(digest.as_bytes());
    mac.update(state.as_bytes());
    Ok(hex(&mac.finalize().into_bytes()))
}

fn parse_field<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<String, SafeError> {
    lines
        .next()
        .and_then(|line| line.strip_prefix(name))
        .and_then(|value| value.strip_prefix('='))
        .map(str::to_owned)
        .ok_or_else(backup_invalid)
}

fn decode_hex_32(value: &str) -> Result<[u8; 32], SafeError> {
    if value.len() != 64 {
        return Err(backup_invalid());
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or_else(backup_invalid)?;
        let low = hex_nibble(pair[1]).ok_or_else(backup_invalid)?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The application database recovery path is unavailable."),
    )
}

const fn backup_invalid() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageBackupInvalid,
        SafeMessage::new("The application database recovery backup is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{
        AUTHENTICATION_KEY_FILENAME, MANIFEST_FORMAT, MigrationRecovery, atomic_secure_write,
        constant_time_eq, create_recovery_directory, decode_hex_32, file_digest, hex, hex_nibble,
        load_authentication_key, load_or_create_authentication_key, parse_field, read_bounded_file,
        recovery_directory, replace_with_backup, secure_read,
    };

    fn sqlite_database(path: &Path) {
        let connection = Connection::open(path).expect("open sqlite database");
        connection
            .execute("CREATE TABLE durable_probe (value INTEGER NOT NULL)", [])
            .expect("create probe table");
        connection
            .execute("INSERT INTO durable_probe (value) VALUES (7)", [])
            .expect("insert probe row");
    }

    #[test]
    fn migration_recovery_authenticates_finishes_reopens_and_restores() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("studio.sqlite3");
        sqlite_database(&database);

        let recovery = MigrationRecovery::prepare(&database, 5, 10).expect("prepare recovery");
        MigrationRecovery::verify_evidence(&database, 5, 10).expect("prepared evidence");
        assert!(recovery.finish(9).is_err());

        MigrationRecovery::prepare(&database, 5, 10)
            .expect("reopen prepared recovery")
            .finish(10)
            .expect("finish recovery");
        MigrationRecovery::verify_evidence(&database, 5, 10).expect("complete evidence");

        MigrationRecovery::prepare(&database, 5, 10)
            .expect("reopen complete recovery")
            .finish(10)
            .expect("finish reopened recovery");
        fs::write(&database, b"not sqlite").expect("corrupt active database");
        MigrationRecovery::restore(&database, 5, 10).expect("restore authenticated backup");
        let connection = Connection::open(&database).expect("open restored database");
        let value: i64 = connection
            .query_row("SELECT value FROM durable_probe", [], |row| row.get(0))
            .expect("restored row");
        assert_eq!(value, 7);
    }

    #[test]
    fn recovery_manifest_rejects_every_tampered_authority_field() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("studio.sqlite3");
        sqlite_database(&database);
        let recovery = MigrationRecovery::prepare(&database, 5, 10).expect("prepare recovery");
        let original = fs::read_to_string(&recovery.marker).expect("read marker");
        let backup_name = recovery
            .backup
            .file_name()
            .expect("backup name")
            .to_string_lossy();
        let cases = [
            original.replacen(MANIFEST_FORMAT, "wrong-format", 1),
            original.replacen("source_schema=5", "source_schema=4", 1),
            original.replacen("target_schema=10", "target_schema=11", 1),
            original.replacen(&format!("backup={backup_name}"), "backup=other.sqlite3", 1),
            original.replacen("sha256=", "unexpected=value\nsha256=", 1),
            original.replacen("state=prepared", "state=invalid", 1),
            original.replacen("sha256=", "sha256=00", 1),
            original.replacen("hmac_sha256=", "hmac_sha256=gg", 1),
            {
                let mut lines = original.lines().map(str::to_owned).collect::<Vec<_>>();
                let tag = lines
                    .iter_mut()
                    .find(|line| line.starts_with("hmac_sha256="))
                    .expect("tag field");
                let replacement = if tag.ends_with('0') { '1' } else { '0' };
                tag.pop();
                tag.push(replacement);
                format!("{}\n", lines.join("\n"))
            },
        ];
        for tampered in cases {
            fs::write(&recovery.marker, tampered).expect("write tampered marker");
            assert!(MigrationRecovery::verify_evidence(&database, 5, 10).is_err());
        }
        fs::write(&recovery.marker, original).expect("restore marker");
        MigrationRecovery::verify_evidence(&database, 5, 10).expect("restored evidence");
    }

    #[test]
    fn recovery_helpers_reject_invalid_paths_sizes_and_encodings() {
        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"abc").expect("write regular file");
        let child = directory.path().join("child");
        fs::create_dir(&child).expect("create child directory");

        assert!(recovery_directory(Path::new("/")).is_err());
        assert!(create_recovery_directory(&regular).is_err());
        assert!(create_recovery_directory(&regular.join("nested")).is_err());
        assert!(secure_read(&child).is_err());
        assert!(file_digest(&child).is_err());
        assert!(read_bounded_file(&child, 4).is_err());
        assert!(read_bounded_file(&regular, 2).is_err());
        assert_eq!(
            read_bounded_file(&regular, 3).expect("bounded read"),
            b"abc"
        );

        let missing_key_dir = directory.path().join("missing-key");
        fs::create_dir(&missing_key_dir).expect("create missing key directory");
        assert!(load_authentication_key(&missing_key_dir).is_err());
        fs::write(
            missing_key_dir.join(AUTHENTICATION_KEY_FILENAME),
            [0_u8; 31],
        )
        .expect("write short key");
        assert!(load_authentication_key(&missing_key_dir).is_err());

        assert!(decode_hex_32("00").is_err());
        assert!(decode_hex_32(&format!("g0{}", "00".repeat(31))).is_err());
        assert!(decode_hex_32(&format!("0g{}", "00".repeat(31))).is_err());
        let zeros = decode_hex_32(&"00".repeat(32)).expect("decode zeros");
        assert!(constant_time_eq(&zeros, &[0_u8; 32]));
        assert!(!constant_time_eq(&zeros, &[1_u8; 32]));
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        assert_eq!(hex_nibble(b'9'), Some(9));
        assert_eq!(hex_nibble(b'f'), Some(15));
        assert_eq!(hex_nibble(b'G'), None);

        let mut valid = ["field=value"].into_iter();
        assert_eq!(parse_field(&mut valid, "field").expect("field"), "value");
        let mut invalid = ["other=value"].into_iter();
        assert!(parse_field(&mut invalid, "field").is_err());
        let mut missing = std::iter::empty();
        assert!(parse_field(&mut missing, "field").is_err());

        let absent_parent = directory.path().join("absent").join("marker");
        assert!(atomic_secure_write(&absent_parent, b"marker").is_err());

        let orphan_database = directory.path().join("orphan.sqlite3");
        sqlite_database(&orphan_database);
        let orphan_directory = recovery_directory(&orphan_database).expect("recovery directory");
        create_recovery_directory(&orphan_directory).expect("create recovery directory");
        load_or_create_authentication_key(&orphan_directory).expect("authentication key");
        fs::write(
            orphan_directory.join("migration-v5-to-v10.sqlite3"),
            b"orphan backup",
        )
        .expect("orphan backup");
        assert!(MigrationRecovery::prepare(&orphan_database, 5, 10).is_err());

        let missing_database = directory.path().join("missing-database.sqlite3");
        assert!(replace_with_backup(&missing_database, &regular).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_helpers_reject_symlink_inputs() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"abc").expect("write regular file");
        let link = directory.path().join("link");
        symlink(&regular, &link).expect("create symlink");
        assert!(secure_read(&link).is_err());
        assert!(file_digest(&link).is_err());
        assert!(read_bounded_file(&link, 3).is_err());
        assert!(create_recovery_directory(&link).is_err());
    }
}
