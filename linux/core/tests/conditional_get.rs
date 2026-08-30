//! Integration tests for the conditional-GET path against a mock HTTP server.
//!
//! These exercise the real `reqwest` client end-to-end: that stored validators
//! are replayed as request headers, that 304 short-circuits, that fresh
//! validators are captured, and that 429/Retry-After is honored.

use std::time::Duration;

use fodder_core::poller::http::{conditional_get, FetchResponse};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RSS_BODY: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>Mock</title>
<item><guid>1</guid><title>One</title><link>https://e.com/1</link></item>
</channel></rss>"#;

fn client() -> reqwest::Client {
    fodder_core::install_default_crypto();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn sends_conditional_headers_and_handles_304() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    let url = format!("{}/feed.xml", server.uri());
    let resp = conditional_get(
        &client(),
        &url,
        Some("\"etag-1\""),
        Some("Mon, 01 Jan 2024 00:00:00 GMT"),
    )
    .await;

    assert!(
        matches!(resp, FetchResponse::NotModified),
        "expected NotModified, got {resp:?}"
    );

    // Inspect the recorded request to confirm both validators were actually
    // replayed as conditional headers.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let headers = &requests[0].headers;
    assert_eq!(
        headers.get("if-none-match").map(|v| v.to_str().unwrap()),
        Some("\"etag-1\"")
    );
    assert_eq!(
        headers
            .get("if-modified-since")
            .map(|v| v.to_str().unwrap()),
        Some("Mon, 01 Jan 2024 00:00:00 GMT")
    );
}

#[tokio::test]
async fn captures_new_validators_on_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"new-etag\"")
                .insert_header("Last-Modified", "Wed, 10 Jan 2024 12:00:00 GMT")
                .set_body_string(RSS_BODY),
        )
        .mount(&server)
        .await;

    let url = format!("{}/feed.xml", server.uri());
    let resp = conditional_get(&client(), &url, None, None).await;

    match resp {
        FetchResponse::Modified {
            etag,
            last_modified,
            body,
        } => {
            assert_eq!(etag.as_deref(), Some("\"new-etag\""));
            assert_eq!(
                last_modified.as_deref(),
                Some("Wed, 10 Jan 2024 12:00:00 GMT")
            );
            assert!(!body.is_empty());
        }
        other => panic!("expected Modified, got {other:?}"),
    }
}

#[tokio::test]
async fn honors_retry_after_seconds_on_429() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .mount(&server)
        .await;

    let resp = conditional_get(&client(), &server.uri(), None, None).await;
    match resp {
        FetchResponse::RateLimited { retry_after } => {
            assert_eq!(retry_after, Duration::from_secs(120));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn honors_retry_after_http_date_on_429() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "Wed, 21 Oct 2099 07:28:00 GMT"),
        )
        .mount(&server)
        .await;

    let resp = conditional_get(&client(), &server.uri(), None, None).await;
    match resp {
        FetchResponse::RateLimited { retry_after } => {
            assert!(retry_after.as_secs() > 0, "future date → positive delay");
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn non_success_status_is_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let resp = conditional_get(&client(), &server.uri(), None, None).await;
    assert!(matches!(resp, FetchResponse::Error(_)), "got {resp:?}");
}
