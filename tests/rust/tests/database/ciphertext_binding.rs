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

/// Once every stored value is bound, legacy-looking values are refused: a
/// plaintext override or an unbound ciphertext put into the database later
/// wasn't written by McpMux.
#[tokio::test]
async fn once_everything_is_bound_unbound_values_are_refused() {
    let s = setup().await;
    let creds = SqliteCredentialRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let servers = SqliteInstalledServerRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    creds
        .save(&Credential::api_key(s.space, "github", "ghp_bound"))
        .await
        .unwrap();
    let mut server = fixtures::test_installed_server(&s.space.to_string(), "github");
    server.env_overrides.insert("TOKEN".into(), "bound".into());
    InstalledServerRepository::install(&servers, &server)
        .await
        .unwrap();

    {
        let db = s.db.lock().await;
        assert_eq!(db.unbound_value_count().unwrap(), 0);
        assert!(!db.ciphertexts_bound());
        db.mark_ciphertexts_bound().unwrap();
        assert!(db.ciphertexts_bound());
    }
    s.enc.require_bound();

    {
        let db = s.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "UPDATE credentials SET credential_value = ?1",
            [s.enc.encrypt("planted").unwrap()],
        )
        .unwrap();
        conn.execute(
            r#"UPDATE installed_servers SET env_overrides = '{"TOKEN":"planted"}'"#,
            [],
        )
        .unwrap();
    }
    assert!(creds
        .get(&s.space, "github", &CredentialType::ApiKey)
        .await
        .is_err());
    assert!(InstalledServerRepository::get(&servers, &server.id)
        .await
        .is_err());
}

/// A value that can't be converted keeps the database from being marked
/// bound, so its legacy reads aren't switched off under it.
#[tokio::test]
async fn unconverted_values_are_counted() {
    let s = setup().await;
    let servers = SqliteInstalledServerRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let server = fixtures::test_installed_server(&s.space.to_string(), "srv");
    InstalledServerRepository::install(&servers, &server)
        .await
        .unwrap();
    s.db.lock()
        .await
        .connection()
        .execute("UPDATE installed_servers SET extra_headers = 'garbage'", [])
        .unwrap();
    servers.encrypt_plaintext_rows().await.unwrap();
    assert_eq!(s.db.lock().await.unbound_value_count().unwrap(), 1);
}

/// Each column of a row has its own binding: a ciphertext copied from one
/// settings column into another of the same server doesn't decrypt.
#[tokio::test]
async fn a_value_copied_between_columns_of_one_row_does_not_decrypt() {
    let s = setup().await;
    let servers = SqliteInstalledServerRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let mut server = fixtures::test_installed_server(&s.space.to_string(), "srv");
    server.env_overrides.insert("A".into(), "1".into());
    server.extra_headers.insert("B".into(), "2".into());
    InstalledServerRepository::install(&servers, &server)
        .await
        .unwrap();
    s.db.lock()
        .await
        .connection()
        .execute(
            "UPDATE installed_servers SET extra_headers = env_overrides",
            [],
        )
        .unwrap();
    assert!(InstalledServerRepository::get(&servers, &server.id)
        .await
        .is_err());
}

/// Outbound OAuth client secrets are bound too, and an unbound one from an
/// earlier build is converted and still read.
#[tokio::test]
async fn client_secrets_are_bound_and_legacy_ones_converted() {
    use mcpmux_core::{OutboundOAuthRegistration, OutboundOAuthRepository};
    use mcpmux_storage::SqliteOutboundOAuthRepository;

    let s = setup().await;
    let repo = SqliteOutboundOAuthRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let registration = OutboundOAuthRegistration::new(
        s.space,
        "srv",
        "https://mcp.example.com/mcp",
        "client-1",
        "http://127.0.0.1:45819/oauth2redirect",
    )
    .with_client_secret(Some("client-secret-value".into()));
    repo.save(&registration).await.unwrap();
    let stored = || async {
        s.db.lock()
            .await
            .connection()
            .query_row(
                "SELECT client_secret_encrypted FROM outbound_oauth_clients",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
    };
    assert!(FieldEncryptor::is_bound(&stored().await));

    // As an earlier build stored it.
    s.db.lock()
        .await
        .connection()
        .execute(
            "UPDATE outbound_oauth_clients SET client_secret_encrypted = ?1",
            [s.enc.encrypt("client-secret-value").unwrap()],
        )
        .unwrap();
    assert_eq!(s.db.lock().await.unbound_value_count().unwrap(), 1);
    assert_eq!(
        s.db.lock().await.bind_legacy_ciphertexts(&s.enc).unwrap(),
        1
    );
    assert!(FieldEncryptor::is_bound(&stored().await));
    let loaded = repo.get(&s.space, "srv").await.unwrap().unwrap();
    assert_eq!(loaded.client_secret.as_deref(), Some("client-secret-value"));
}

/// A row with one column as an unbound ciphertext and another as plaintext
/// JSON (both from earlier builds) is converted in one pass.
#[tokio::test]
async fn a_row_mixing_plaintext_and_unbound_columns_is_converted() {
    let s = setup().await;
    let servers = SqliteInstalledServerRepository::new(Arc::clone(&s.db), Arc::clone(&s.enc));
    let server = fixtures::test_installed_server(&s.space.to_string(), "mixed");
    InstalledServerRepository::install(&servers, &server)
        .await
        .unwrap();
    s.db.lock()
        .await
        .connection()
        .execute(
            r#"UPDATE installed_servers SET input_values = ?1, env_overrides = '{"TOKEN":"plain"}'"#,
            [s.enc.encrypt(r#"{"KEY":"unbound"}"#).unwrap()],
        )
        .unwrap();

    assert_eq!(servers.encrypt_plaintext_rows().await.unwrap(), 1);
    assert_eq!(s.db.lock().await.unbound_value_count().unwrap(), 0);
    let loaded = InstalledServerRepository::get(&servers, &server.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.input_values["KEY"], "unbound");
    assert_eq!(loaded.env_overrides["TOKEN"], "plain");
}
