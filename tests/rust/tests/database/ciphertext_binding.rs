//! Stored secrets are bound to their row: a ciphertext copied elsewhere does
//! not decrypt, and values written by earlier builds are converted in place.

use mcpmux_core::repository::{CredentialRepository, InstalledServerRepository, SpaceRepository};
use mcpmux_core::{Credential, CredentialType};
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, SqliteCredentialRepository,
    SqliteInstalledServerRepository, SqliteSpaceRepository,
};
use std::sync::Arc;
use tests::fixtures;
use tokio::sync::Mutex;

struct Setup {
    db: Arc<Mutex<Database>>,
    enc: Arc<FieldEncryptor>,
    space: uuid::Uuid,
    _dir: tempfile::TempDir,
}

async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Mutex::new(
        Database::open(&dir.path().join("mcpmux.db")).unwrap(),
    ));
    let enc = Arc::new(FieldEncryptor::new(&generate_master_key().unwrap()).unwrap());
    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&SqliteSpaceRepository::new(Arc::clone(&db)), &space)
        .await
        .unwrap();
    Setup {
        db,
        enc,
        space: space.id,
        _dir: dir,
    }
}

#[tokio::test]
async fn a_credential_copied_to_another_server_does_not_decrypt() {
    let s = setup().await;
    let repo = SqliteCredentialRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    repo.save(&Credential::api_key(s.space, "github", "ghp_real_token"))
        .await
        .unwrap();
    repo.save(&Credential::api_key(s.space, "attacker", "placeholder"))
        .await
        .unwrap();

    // Copy github's ciphertext over the other server's row.
    s.db.lock()
        .await
        .connection()
        .execute(
            "UPDATE credentials SET credential_value =
               (SELECT credential_value FROM credentials WHERE server_id = 'github')
             WHERE server_id = 'attacker'",
            [],
        )
        .unwrap();

    assert!(repo
        .get(&s.space, "attacker", &CredentialType::ApiKey)
        .await
        .is_err());
    let original = repo
        .get(&s.space, "github", &CredentialType::ApiKey)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.value, "ghp_real_token");
}

#[tokio::test]
async fn values_from_earlier_builds_are_bound_and_still_read() {
    let s = setup().await;
    let creds = SqliteCredentialRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let servers = SqliteInstalledServerRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    creds
        .save(&Credential::api_key(s.space, "github", "ghp_old"))
        .await
        .unwrap();
    let mut server = fixtures::test_installed_server(&s.space.to_string(), "github");
    server.input_values.insert("KEY".into(), "input-old".into());
    InstalledServerRepository::install(&servers, &server)
        .await
        .unwrap();

    // Rewrite both as an earlier build stored them: unbound ciphertexts.
    {
        let db = s.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "UPDATE credentials SET credential_value = ?1",
            [s.enc.encrypt("ghp_old").unwrap()],
        )
        .unwrap();
        conn.execute(
            "UPDATE installed_servers SET input_values = ?1",
            [s.enc.encrypt(r#"{"KEY":"input-old"}"#).unwrap()],
        )
        .unwrap();
    }
    // Readable before conversion...
    assert_eq!(
        creds
            .get(&s.space, "github", &CredentialType::ApiKey)
            .await
            .unwrap()
            .unwrap()
            .value,
        "ghp_old"
    );

    // ...converted by the startup passes...
    assert_eq!(
        s.db.lock().await.bind_legacy_ciphertexts(&s.enc).unwrap(),
        1
    );
    assert_eq!(servers.encrypt_plaintext_rows().await.unwrap(), 1);
    let (cred, inputs): (String, String) = s
        .db
        .lock()
        .await
        .connection()
        .query_row(
            "SELECT (SELECT credential_value FROM credentials), (SELECT input_values FROM installed_servers)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(FieldEncryptor::is_bound(&cred) && FieldEncryptor::is_bound(&inputs));

    // ...and still read the same.
    assert_eq!(
        creds
            .get(&s.space, "github", &CredentialType::ApiKey)
            .await
            .unwrap()
            .unwrap()
            .value,
        "ghp_old"
    );
    let loaded = InstalledServerRepository::get(&servers, &server.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.input_values["KEY"], "input-old");
    // Nothing left to convert on the next start.
    assert_eq!(
        s.db.lock().await.bind_legacy_ciphertexts(&s.enc).unwrap(),
        0
    );
    assert_eq!(servers.encrypt_plaintext_rows().await.unwrap(), 0);
}
