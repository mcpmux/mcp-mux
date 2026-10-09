//! Choosing the master key the stored data is encrypted with.
//!
//! On macOS and Linux the key lives in the OS keychain, or in
//! `<data_dir>/keys/master.key` where no keychain is available. Picking the
//! wrong one is not recoverable in practice: a *new* key makes every stored
//! credential unreadable, and whatever is saved next is encrypted with a
//! key the rest of the data doesn't use. So the choice is never "whichever
//! works right now":
//!
//! 1. `<data_dir>/master-key.json` records the source and a one-way
//!    fingerprint of the key in use. When present, only that key is used.
//! 2. Without a record (installs from before it existed), the key that
//!    decrypts the stored ciphertexts wins.
//! 3. Only when nothing encrypted is stored yet may a key be created — in
//!    the keychain when it works, otherwise in a file.
//!
//! When the keychain is unavailable and the data needs the key kept there,
//! startup fails with an explanation instead of creating a new key.

use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use mcpmux_storage::Database;
use mcpmux_storage::{key_fingerprint, EncryptedSample, FieldEncryptor, KEY_SIZE};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// A master key.
pub type Key = Zeroizing<[u8; KEY_SIZE]>;

/// File (in the data directory) recording which key the data uses.
pub const KEY_RECORD_FILE: &str = "master-key.json";

/// How many stored ciphertexts to try candidate keys against.
pub(crate) const SAMPLE_LIMIT: usize = 64;

/// Where a master key is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Keychain,
    File,
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KeySource::Keychain => "OS keychain",
            KeySource::File => "key file",
        })
    }
}

/// Contents of [`KEY_RECORD_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRecord {
    pub source: KeySource,
    pub fingerprint: String,
}

/// Result of asking the keychain for an existing key.
pub enum KeychainLookup {
    Found(Key),
    /// The keychain works but holds no McpMux key.
    Empty,
    /// The keychain can't be used right now (no Secret Service, locked,
    /// prompt dismissed, D-Bus timeout, ...).
    Unavailable(String),
}

/// What to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChoice {
    Use(KeySource),
    Create(KeySource),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyChoiceError {
    #[error(
        "the OS keychain is unavailable ({0}) and holds the key McpMux's data is \
         encrypted with. Unlock the keyring (or start a Secret Service provider) and \
         try again. McpMux will not create a new key: that would make the stored \
         credentials unreadable"
    )]
    KeychainUnavailable(String),
    #[error(
        "the {kept_in} no longer holds the key McpMux's data is encrypted with (see \
         master-key.json in the data directory). Restore it (unlock the keyring, or put \
         back keys/master.key) and try again. McpMux will not create a new key: that \
         would make the stored credentials unreadable. To start over on purpose, move \
         the data directory aside"
    )]
    RecordedKeyMissing { kept_in: KeySource },
    #[error(
        "none of the available master keys ({available}) is the one McpMux's data is \
         encrypted with. If the key was removed on purpose, move the data directory \
         aside to start over"
    )]
    NoMatchingKey { available: String },
    #[error(
        "the master key file (keys/master.key in the data directory) is missing, but \
         McpMux's data is encrypted with it. Restore it from a backup and try again. \
         McpMux will not create a new key: that would make the stored credentials \
         unreadable. To start over on purpose, move the data directory aside"
    )]
    KeyFileMissing,
}

/// How many of `samples` `key` decrypts.
fn decrypt_count(key: &Key, samples: &[EncryptedSample]) -> usize {
    match FieldEncryptor::new(key) {
        Ok(encryptor) => samples
            .iter()
            .filter(|s| encryptor.decrypt_bound(&s.value, &s.context).is_ok())
            .count(),
        Err(_) => 0,
    }
}

/// How many of `samples` `key` can't decrypt: stored secrets written with
/// another key, which stay unreadable.
pub fn unreadable_samples(key: &Key, samples: &[EncryptedSample]) -> usize {
    samples.len() - decrypt_count(key, samples)
}

/// Warn when some stored secrets are encrypted with a different key than
/// the one in use (e.g. a key was replaced at some point).
pub(crate) fn warn_if_partly_unreadable(key: &Key, samples: &[EncryptedSample]) {
    let unreadable = unreadable_samples(key, samples);
    if unreadable > 0 {
        tracing::warn!(
            decrypted = samples.len() - unreadable,
            total = samples.len(),
            "some stored secrets are encrypted with a different key and can't be read; \
             sign in to or reconfigure the affected servers"
        );
    }
}

/// Where there is a single key and nothing to choose between (DPAPI on
/// Windows): use the key when it reads the stored data, and create one only
/// when nothing encrypted is stored yet. `existing` is the key already
/// kept, `create` makes a new one.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn guard_single_key(
    existing: Option<Key>,
    samples: &[EncryptedSample],
    create: impl FnOnce() -> anyhow::Result<Key>,
) -> anyhow::Result<Key> {
    match existing {
        Some(key) if !samples.is_empty() && decrypt_count(&key, samples) == 0 => {
            Err(KeyChoiceError::NoMatchingKey {
                available: "the DPAPI key file".to_string(),
            }
            .into())
        }
        Some(key) => Ok(key),
        None if !samples.is_empty() => Err(KeyChoiceError::KeyFileMissing.into()),
        None => create(),
    }
}

/// Decide which master key to use. Pure: every input is passed in, so it
/// can be tested without touching a real keychain.
///
/// `allowed` lists the sources the key-provider policy permits, in order of
/// preference. `samples` are stored ciphertexts (see
/// [`Database::encrypted_samples`]).
pub fn choose_master_key(
    allowed: &[KeySource],
    record: Option<&KeyRecord>,
    keychain: &KeychainLookup,
    file_key: Option<&Key>,
    samples: &[EncryptedSample],
) -> Result<KeyChoice, KeyChoiceError> {
    let candidates: Vec<(KeySource, &Key)> = allowed
        .iter()
        .filter_map(|source| match source {
            KeySource::Keychain => match keychain {
                KeychainLookup::Found(key) => Some((KeySource::Keychain, key)),
                _ => None,
            },
            KeySource::File => file_key.map(|key| (KeySource::File, key)),
        })
        .collect();
    let keychain_error = match keychain {
        KeychainLookup::Unavailable(e) if allowed.contains(&KeySource::Keychain) => Some(e),
        _ => None,
    };
    let refuse = || match keychain_error {
        Some(e) => KeyChoiceError::KeychainUnavailable(e.clone()),
        None => KeyChoiceError::NoMatchingKey {
            available: if candidates.is_empty() {
                "none".to_string()
            } else {
                candidates
                    .iter()
                    .map(|(source, _)| source.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        },
    };

    // 1. A recorded key is the only acceptable one.
    if let Some(record) = record {
        if let Some((source, _)) = candidates
            .iter()
            .find(|(_, key)| key_fingerprint(key) == record.fingerprint)
        {
            return Ok(KeyChoice::Use(*source));
        }
        return Err(match (record.source, keychain_error) {
            (KeySource::Keychain, Some(e)) => KeyChoiceError::KeychainUnavailable(e.clone()),
            (kept_in, _) => KeyChoiceError::RecordedKeyMissing { kept_in },
        });
    }

    // 2. No record: the key that decrypts the stored data.
    if !samples.is_empty() {
        let mut best: Option<(KeySource, usize)> = None;
        for (source, key) in &candidates {
            let count = decrypt_count(key, samples);
            if count > 0 && best.is_none_or(|(_, b)| count > b) {
                best = Some((*source, count));
            }
        }
        return best
            .map(|(source, _)| KeyChoice::Use(source))
            .ok_or_else(refuse);
    }

    // 3. Nothing encrypted yet: an existing key, else a new one.
    if let Some((source, _)) = candidates.first() {
        return Ok(KeyChoice::Use(*source));
    }
    for source in allowed {
        match (source, keychain) {
            (KeySource::Keychain, KeychainLookup::Empty) => {
                return Ok(KeyChoice::Create(KeySource::Keychain))
            }
            (KeySource::Keychain, _) => continue,
            (KeySource::File, _) => return Ok(KeyChoice::Create(KeySource::File)),
        }
    }
    Err(refuse())
}

/// Access to the OS keychain, injectable so tests never touch the user's
/// real keyring (whose McpMux entry is shared by every data directory).
pub trait KeychainAccess {
    fn lookup(&self) -> KeychainLookup;
    fn create(&self) -> anyhow::Result<Key>;
}

/// The real OS keychain.
pub struct OsKeychain;

impl KeychainAccess for OsKeychain {
    fn lookup(&self) -> KeychainLookup {
        match mcpmux_storage::KeychainKeyProvider::new()
            .and_then(|provider| provider.get_existing_key())
        {
            Ok(Some(key)) => KeychainLookup::Found(key),
            Ok(None) => KeychainLookup::Empty,
            Err(e) => KeychainLookup::Unavailable(e.to_string()),
        }
    }

    fn create(&self) -> anyhow::Result<Key> {
        use mcpmux_storage::MasterKeyProvider;
        mcpmux_storage::KeychainKeyProvider::new()?.get_or_create_key()
    }
}

fn record_path(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_RECORD_FILE)
}

/// Read the key record. A record that exists but can't be read is an error:
/// ignoring it would silently drop the protection it gives.
pub fn read_key_record(data_dir: &Path) -> anyhow::Result<Option<KeyRecord>> {
    let path = record_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            anyhow::anyhow!(
                "{} is unreadable ({e}). Restore it from a backup; removing it makes \
                 McpMux re-detect the key from the stored data",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::anyhow!("cannot read {}: {e}", path.display())),
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn write_key_record(data_dir: &Path, record: &KeyRecord) -> anyhow::Result<()> {
    use std::io::Write;
    let path = record_path(data_dir);
    let tmp = data_dir.join(format!(".{KEY_RECORD_FILE}.tmp"));
    // On disk before the rename: a crash must not leave an empty record,
    // which would stop McpMux from starting.
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(record)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Resolve the master key for `data_dir` under the given policy (`allowed`
/// sources in preference order), creating one only when nothing encrypted
/// is stored yet, and record which key is in use. Not used on Windows,
/// where DPAPI keeps the key next to the data.
#[cfg(not(windows))]
pub fn resolve_master_key(
    data_dir: &Path,
    database: &Database,
    allowed: &[KeySource],
    keychain: &dyn KeychainAccess,
) -> anyhow::Result<(Key, KeySource)> {
    let record = read_key_record(data_dir)?;
    let keychain_lookup = if allowed.contains(&KeySource::Keychain) {
        keychain.lookup()
    } else {
        KeychainLookup::Empty
    };
    let file_key = if allowed.contains(&KeySource::File)
        && data_dir.join("keys").join("master.key").exists()
    {
        match mcpmux_storage::FileKeyProvider::new(data_dir)
            .and_then(|provider| provider.get_existing_key())
        {
            Ok(key) => key,
            // The data uses the keychain key; a leftover key file that
            // can't be read doesn't matter.
            Err(e) if record.as_ref().map(|r| r.source) == Some(KeySource::Keychain) => {
                tracing::warn!("ignoring an unreadable stale keys/master.key: {e}");
                None
            }
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    let samples = database.encrypted_samples(SAMPLE_LIMIT)?;

    let choice = choose_master_key(
        allowed,
        record.as_ref(),
        &keychain_lookup,
        file_key.as_ref(),
        &samples,
    )?;
    let (key, source) = match choice {
        KeyChoice::Use(KeySource::Keychain) => match keychain_lookup {
            KeychainLookup::Found(key) => (key, KeySource::Keychain),
            _ => unreachable!("chose the keychain key without one"),
        },
        KeyChoice::Use(KeySource::File) => (
            file_key.expect("chose the file key without one"),
            KeySource::File,
        ),
        KeyChoice::Create(KeySource::Keychain) => match keychain.create() {
            Ok(key) => (key, KeySource::Keychain),
            // Nothing is encrypted yet, so a file key loses nothing.
            Err(e) if allowed.contains(&KeySource::File) => {
                tracing::warn!(
                    "OS keychain unavailable ({e}), using file-based key storage. \
                     For better security, install gnome-keyring or another Secret Service provider."
                );
                (create_file_key(data_dir)?, KeySource::File)
            }
            Err(e) => return Err(e),
        },
        KeyChoice::Create(KeySource::File) => (create_file_key(data_dir)?, KeySource::File),
    };

    warn_if_partly_unreadable(&key, &samples);
    let current = KeyRecord {
        source,
        fingerprint: key_fingerprint(&key),
    };
    if record.as_ref() != Some(&current) {
        write_key_record(data_dir, &current)?;
    }
    tracing::info!("[runtime] master key: {source}");
    Ok((key, source))
}

#[cfg(not(windows))]
fn create_file_key(data_dir: &Path) -> anyhow::Result<Key> {
    use mcpmux_storage::MasterKeyProvider;
    mcpmux_storage::FileKeyProvider::new(data_dir)?.get_or_create_key()
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use std::cell::Cell;

    const AUTO: &[KeySource] = &[KeySource::Keychain, KeySource::File];

    fn key(byte: u8) -> Key {
        Zeroizing::new([byte; KEY_SIZE])
    }

    /// Bound (`v2:`) ciphertexts, as current builds store them.
    fn ciphertexts(k: &Key, n: usize) -> Vec<EncryptedSample> {
        let enc = FieldEncryptor::new(k).unwrap();
        (0..n)
            .map(|i| {
                let context = format!("credentials|credential_value|s|srv{i}|access_token");
                EncryptedSample {
                    value: enc.encrypt_bound(&format!("secret-{i}"), &context).unwrap(),
                    context,
                }
            })
            .collect()
    }

    /// Unbound ciphertexts, as earlier builds stored them.
    fn legacy_ciphertexts(k: &Key, n: usize) -> Vec<EncryptedSample> {
        let enc = FieldEncryptor::new(k).unwrap();
        (0..n)
            .map(|i| EncryptedSample {
                value: enc.encrypt(&format!("secret-{i}")).unwrap(),
                context: "ignored".to_string(),
            })
            .collect()
    }

    #[test]
    fn legacy_unbound_data_still_identifies_its_key() {
        let (kc, stale) = (key(1), key(2));
        let choice = choose_master_key(
            AUTO,
            None,
            &KeychainLookup::Found(kc.clone()),
            Some(&stale),
            &legacy_ciphertexts(&kc, 3),
        );
        assert_eq!(choice, Ok(KeyChoice::Use(KeySource::Keychain)));
    }

    fn record(source: KeySource, k: &Key) -> KeyRecord {
        KeyRecord {
            source,
            fingerprint: key_fingerprint(k),
        }
    }

    fn unavailable() -> KeychainLookup {
        KeychainLookup::Unavailable("no Secret Service".into())
    }

    #[test]
    fn recorded_keychain_key_is_required_when_the_keychain_is_down() {
        let kc = key(1);
        let choice = choose_master_key(
            AUTO,
            Some(&record(KeySource::Keychain, &kc)),
            &unavailable(),
            None,
            &[],
        );
        assert!(matches!(
            choice,
            Err(KeyChoiceError::KeychainUnavailable(_))
        ));
    }

    #[test]
    fn recorded_file_key_wins_over_a_different_keychain_key() {
        let (kc, file) = (key(1), key(2));
        let choice = choose_master_key(
            AUTO,
            Some(&record(KeySource::File, &file)),
            &KeychainLookup::Found(kc),
            Some(&file),
            &[],
        );
        assert_eq!(choice, Ok(KeyChoice::Use(KeySource::File)));
    }

    #[test]
    fn without_a_record_the_key_that_decrypts_the_data_wins() {
        // A stale file key left by an earlier transient keychain error, while
        // the data is encrypted with the keychain key.
        let (kc, stale) = (key(1), key(2));
        let mut samples = ciphertexts(&kc, 5);
        samples.extend(ciphertexts(&stale, 1));
        let choice = choose_master_key(
            &[KeySource::File, KeySource::Keychain],
            None,
            &KeychainLookup::Found(kc),
            Some(&stale),
            &samples,
        );
        assert_eq!(choice, Ok(KeyChoice::Use(KeySource::Keychain)));
    }

    #[test]
    fn existing_data_and_a_down_keychain_never_create_a_key() {
        let kc = key(1);
        let choice = choose_master_key(AUTO, None, &unavailable(), None, &ciphertexts(&kc, 2));
        assert!(matches!(
            choice,
            Err(KeyChoiceError::KeychainUnavailable(_))
        ));
    }

    #[test]
    fn a_fresh_install_without_a_keychain_gets_a_file_key() {
        assert_eq!(
            choose_master_key(AUTO, None, &unavailable(), None, &[]),
            Ok(KeyChoice::Create(KeySource::File))
        );
        assert_eq!(
            choose_master_key(AUTO, None, &KeychainLookup::Empty, None, &[]),
            Ok(KeyChoice::Create(KeySource::Keychain))
        );
    }

    #[test]
    fn explicit_file_policy_is_checked_against_the_record() {
        let (kc, file) = (key(1), key(2));
        let choice = choose_master_key(
            &[KeySource::File],
            Some(&record(KeySource::Keychain, &kc)),
            &KeychainLookup::Empty,
            Some(&file),
            &[],
        );
        assert_eq!(
            choice,
            Err(KeyChoiceError::RecordedKeyMissing {
                kept_in: KeySource::Keychain
            })
        );
    }

    struct FakeKeychain {
        lookup: fn() -> KeychainLookup,
        created: Cell<bool>,
    }

    impl KeychainAccess for FakeKeychain {
        fn lookup(&self) -> KeychainLookup {
            (self.lookup)()
        }
        fn create(&self) -> anyhow::Result<Key> {
            self.created.set(true);
            Ok(key(9))
        }
    }

    /// A database holding one credential, encrypted (bound) with `k`.
    fn database_with_ciphertext(dir: &Path, k: &Key) -> Database {
        let db = Database::open(&dir.join("mcpmux.db")).unwrap();
        let space: String = db
            .connection()
            .query_row("SELECT id FROM spaces LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let context = mcpmux_storage::binding::credential(&space, "srv", "access_token");
        let value = FieldEncryptor::new(k)
            .unwrap()
            .encrypt_bound("token", &context)
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO credentials (id, space_id, server_id, credential_type, credential_value, created_at, updated_at)
                 VALUES ('c1', (SELECT id FROM spaces LIMIT 1), 'srv', 'access_token', ?1, 'now', 'now')",
                [value],
            )
            .unwrap();
        db
    }

    #[test]
    fn a_database_of_bound_ciphertexts_identifies_its_key() {
        let tmp = tempfile::tempdir().unwrap();
        let file = key(3);
        let db = database_with_ciphertext(tmp.path(), &file);
        std::fs::create_dir_all(tmp.path().join("keys")).unwrap();
        std::fs::write(tmp.path().join("keys/master.key"), *file).unwrap();
        let keychain = FakeKeychain {
            lookup: || KeychainLookup::Found(key(4)),
            created: Cell::new(false),
        };
        // No record yet: the stored (bound) data decides, not preference.
        let (chosen, source) = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap();
        assert_eq!(source, KeySource::File);
        assert_eq!(*chosen, *file);
    }

    #[test]
    fn keychain_error_with_a_recorded_keychain_key_writes_no_key_file() {
        let tmp = tempfile::tempdir().unwrap();
        let kc = key(1);
        let db = database_with_ciphertext(tmp.path(), &kc);
        write_key_record(tmp.path(), &record(KeySource::Keychain, &kc)).unwrap();
        let keychain = FakeKeychain {
            lookup: unavailable,
            created: Cell::new(false),
        };

        let err = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap_err();
        assert!(err.to_string().contains("keychain is unavailable"), "{err}");
        assert!(!tmp.path().join("keys/master.key").exists());
        assert!(!keychain.created.get());
    }

    #[test]
    fn fresh_install_without_keychain_creates_and_records_a_file_key() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open(&tmp.path().join("mcpmux.db")).unwrap();
        let keychain = FakeKeychain {
            lookup: unavailable,
            created: Cell::new(false),
        };

        let (k, source) = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap();
        assert_eq!(source, KeySource::File);
        assert!(tmp.path().join("keys/master.key").exists());
        assert_eq!(
            read_key_record(tmp.path()).unwrap(),
            Some(record(KeySource::File, &k))
        );

        // Next start: the same key, even once a keychain shows up.
        let keychain = FakeKeychain {
            lookup: || KeychainLookup::Found(key(7)),
            created: Cell::new(false),
        };
        let (again, source) = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap();
        assert_eq!(source, KeySource::File);
        assert_eq!(*again, *k);
    }

    #[test]
    fn a_deleted_key_file_is_not_silently_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open(&tmp.path().join("mcpmux.db")).unwrap();
        let keychain = FakeKeychain {
            lookup: unavailable,
            created: Cell::new(false),
        };
        let (k, _) = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap();
        let value = FieldEncryptor::new(&k).unwrap().encrypt("token").unwrap();
        db.connection()
            .execute(
                "INSERT INTO credentials (id, space_id, server_id, credential_type, credential_value, created_at, updated_at)
                 VALUES ('c1', (SELECT id FROM spaces LIMIT 1), 'srv', 'access_token', ?1, 'now', 'now')",
                [value],
            )
            .unwrap();
        std::fs::remove_file(tmp.path().join("keys/master.key")).unwrap();

        let err = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap_err();
        assert!(
            err.to_string().starts_with("the key file no longer holds"),
            "{err}"
        );
        assert!(!tmp.path().join("keys/master.key").exists());
    }

    #[test]
    fn a_single_key_is_only_created_when_nothing_is_encrypted() {
        let kept = key(1);
        let samples = ciphertexts(&kept, 3);
        let created = Cell::new(false);
        let create = || {
            created.set(true);
            Ok(key(9))
        };

        // No key file, but data encrypted with one: refuse.
        let err = guard_single_key(None, &samples, create).unwrap_err();
        assert!(err.to_string().contains("is missing"), "{err}");
        assert!(!created.get());

        // A key file that reads none of the data: refuse.
        let err = guard_single_key(Some(key(2)), &samples, || Ok(key(9))).unwrap_err();
        assert!(err.to_string().contains("none of the available"), "{err}");

        // The right key, or a fresh install.
        assert_eq!(
            *guard_single_key(Some(key(1)), &samples, || Ok(key(9))).unwrap(),
            *kept
        );
        let created = Cell::new(false);
        let fresh = guard_single_key(None, &[], || {
            created.set(true);
            Ok(key(9))
        })
        .unwrap();
        assert!(created.get());
        assert_eq!(*fresh, *key(9));
    }

    #[test]
    fn secrets_written_with_another_key_are_counted() {
        let (kept, other) = (key(1), key(2));
        let mut samples = ciphertexts(&kept, 4);
        samples.extend(ciphertexts(&other, 2));
        assert_eq!(unreadable_samples(&kept, &samples), 2);
        assert_eq!(unreadable_samples(&kept, &ciphertexts(&kept, 3)), 0);
    }

    #[test]
    fn an_unreadable_leftover_key_file_is_ignored_when_the_keychain_key_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let kc = key(1);
        let db = database_with_ciphertext(tmp.path(), &kc);
        write_key_record(tmp.path(), &record(KeySource::Keychain, &kc)).unwrap();
        std::fs::create_dir_all(tmp.path().join("keys")).unwrap();
        std::fs::write(tmp.path().join("keys/master.key"), b"corrupt").unwrap();
        let keychain = FakeKeychain {
            lookup: || KeychainLookup::Found(key(1)),
            created: Cell::new(false),
        };

        let (k, source) = resolve_master_key(tmp.path(), &db, AUTO, &keychain).unwrap();
        assert_eq!(source, KeySource::Keychain);
        assert_eq!(*k, *kc);
    }

    #[test]
    fn the_record_is_replaced_whole() {
        let tmp = tempfile::tempdir().unwrap();
        write_key_record(tmp.path(), &record(KeySource::File, &key(1))).unwrap();
        write_key_record(tmp.path(), &record(KeySource::Keychain, &key(2))).unwrap();
        assert_eq!(
            read_key_record(tmp.path()).unwrap(),
            Some(record(KeySource::Keychain, &key(2)))
        );
        assert!(!tmp.path().join(format!(".{KEY_RECORD_FILE}.tmp")).exists());
    }

    #[test]
    fn an_unreadable_record_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(KEY_RECORD_FILE), b"not json").unwrap();
        assert!(read_key_record(tmp.path()).is_err());
    }
}
