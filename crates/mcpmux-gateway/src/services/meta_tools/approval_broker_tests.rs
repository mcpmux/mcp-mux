use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use uuid::Uuid;

use super::super::super::MetaToolError;
use super::super::approval_types::ApprovalPayload;
use super::{ApprovalBroker, ApprovalDecision, ApprovalPublisher};

fn make_payload() -> ApprovalPayload {
    ApprovalPayload {
        tool_name: "mcpmux_pin_this_session".into(),
        summary: "test".into(),
        diff: None,
        raw_args: serde_json::json!({}),
        affects_other_clients: false,
    }
}

#[tokio::test]
async fn no_publisher_returns_no_desktop_error() {
    let broker = ApprovalBroker::new();
    let err = broker
        .request_approval(
            &Uuid::new_v4().to_string(),
            "mcpmux_pin_this_session",
            make_payload(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MetaToolError::ApprovalRequiredNoDesktop));
}

#[tokio::test]
async fn always_allow_short_circuits() {
    let broker = ApprovalBroker::new();
    let client_id = Uuid::new_v4().to_string();
    broker.insert_always_allow(&client_id, "mcpmux_pin_this_session");
    let d = broker
        .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap();
    assert_eq!(d, ApprovalDecision::AllowOnce);
}

#[tokio::test]
async fn url_client_id_works() {
    // Regression for the bug where DCR-registered clients (which use
    // a client_metadata URL as their client_id) couldn't get past the
    // approval flow because we tried to parse the URL as a UUID.
    let broker = ApprovalBroker::new();
    let url_client_id = "https://claude.ai/oauth/claude-code-client-metadata";
    broker.insert_always_allow(url_client_id, "mcpmux_pin_this_session");
    let d = broker
        .request_approval(url_client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap();
    assert_eq!(d, ApprovalDecision::AllowOnce);
}

#[tokio::test]
async fn publisher_allow_resolves() {
    let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
    let broker_clone = broker.clone();
    let client_id = Uuid::new_v4().to_string();

    // Publisher responds asynchronously with Allow.
    let publisher: ApprovalPublisher = Arc::new(move |req| {
        let b = broker_clone.clone();
        async move {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                b.respond(
                    &req.request_id,
                    &req.client_id,
                    &req.payload.tool_name,
                    ApprovalDecision::AllowOnce,
                );
            });
            true
        }
        .boxed()
    });
    broker.set_publisher(publisher).await;

    let decision = broker
        .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap();
    assert_eq!(decision, ApprovalDecision::AllowOnce);
}

#[tokio::test]
async fn publisher_deny_returns_denied_error() {
    let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
    let broker_clone = broker.clone();
    let client_id = Uuid::new_v4().to_string();

    let publisher: ApprovalPublisher = Arc::new(move |req| {
        let b = broker_clone.clone();
        async move {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                b.respond(
                    &req.request_id,
                    &req.client_id,
                    &req.payload.tool_name,
                    ApprovalDecision::Deny,
                );
            });
            true
        }
        .boxed()
    });
    broker.set_publisher(publisher).await;

    let err = broker
        .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap_err();
    assert!(matches!(err, MetaToolError::ApprovalDenied));
}

#[tokio::test]
async fn publisher_timeout() {
    let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(50)));
    // Publisher accepts delivery but never responds.
    let publisher: ApprovalPublisher = Arc::new(move |_req| async move { true }.boxed());
    broker.set_publisher(publisher).await;

    let err = broker
        .request_approval(
            &Uuid::new_v4().to_string(),
            "mcpmux_pin_this_session",
            make_payload(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MetaToolError::ApprovalTimedOut));
}

#[tokio::test]
async fn always_scope_persists_across_calls() {
    let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
    let broker_clone = broker.clone();
    let client_id = Uuid::new_v4().to_string();

    let publisher: ApprovalPublisher = Arc::new(move |req| {
        let b = broker_clone.clone();
        async move {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                b.respond(
                    &req.request_id,
                    &req.client_id,
                    &req.payload.tool_name,
                    ApprovalDecision::AlwaysForThisSessionAndClient,
                );
            });
            true
        }
        .boxed()
    });
    broker.set_publisher(publisher).await;

    // First call → dialog, returns AlwaysForThisSessionAndClient.
    let d1 = broker
        .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap();
    assert_eq!(d1, ApprovalDecision::AlwaysForThisSessionAndClient);

    // Second call → short-circuits via always-allow entry.
    let d2 = broker
        .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
        .await
        .unwrap();
    assert_eq!(d2, ApprovalDecision::AllowOnce);
}
