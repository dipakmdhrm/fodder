//! Fetch-layer tests against a loopback HTTP server.

use fodder_core::fetch::{self, FetchOutcome};

const FEED_BODY: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>t</title><item><guid>1</guid><title>a</title></item></channel></rss>"#;

const ETAG: &str = "\"v1\"";

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

/// Serve `handler` on a random loopback port until the returned server is dropped.
fn serve(
    handler: impl Fn(&tiny_http::Request) -> tiny_http::Response<std::io::Cursor<Vec<u8>>>
        + Send
        + 'static,
) -> (std::sync::Arc<tiny_http::Server>, String) {
    let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let srv = server.clone();
    std::thread::spawn(move || {
        for request in srv.incoming_requests() {
            let response = handler(&request);
            let _ = request.respond(response);
        }
    });
    (server, base)
}

fn if_none_match(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv("If-None-Match"))
        .map(|h| h.value.to_string())
}

#[test]
fn fetch_200_with_etag_then_304() {
    let (server, base) = serve(|req| {
        if if_none_match(req).as_deref() == Some(ETAG) {
            tiny_http::Response::from_string("").with_status_code(304)
        } else {
            tiny_http::Response::from_string(FEED_BODY).with_header(header("ETag", ETAG))
        }
    });
    let agent = fetch::agent();

    let outcome = fetch::fetch_feed(&agent, &base, None, None).unwrap();
    let fetched = match outcome {
        FetchOutcome::Fetched(f) => f,
        other => panic!("expected Fetched, got {other:?}"),
    };
    assert_eq!(fetched.body, FEED_BODY.as_bytes());
    assert_eq!(fetched.etag.as_deref(), Some(ETAG));
    assert!(fetched.permanent_url.is_none());

    let outcome = fetch::fetch_feed(&agent, &base, Some(ETAG), None).unwrap();
    assert!(matches!(outcome, FetchOutcome::NotModified));
    drop(server);
}

#[test]
fn permanent_redirect_reports_new_url() {
    let (server, base) = serve(|req| {
        if req.url() == "/old" {
            tiny_http::Response::from_string("")
                .with_status_code(301)
                .with_header(header("Location", "/new"))
        } else {
            tiny_http::Response::from_string(FEED_BODY)
        }
    });
    let agent = fetch::agent();
    let outcome = fetch::fetch_feed(&agent, &format!("{base}/old"), None, None).unwrap();
    match outcome {
        FetchOutcome::Fetched(f) => {
            assert_eq!(
                f.permanent_url.as_deref(),
                Some(format!("{base}/new").as_str())
            );
        }
        other => panic!("expected Fetched, got {other:?}"),
    }
    drop(server);
}

#[test]
fn temporary_redirect_keeps_original_url() {
    let (server, base) = serve(|req| {
        if req.url() == "/old" {
            tiny_http::Response::from_string("")
                .with_status_code(302)
                .with_header(header("Location", "/new"))
        } else {
            tiny_http::Response::from_string(FEED_BODY)
        }
    });
    let agent = fetch::agent();
    let outcome = fetch::fetch_feed(&agent, &format!("{base}/old"), None, None).unwrap();
    match outcome {
        FetchOutcome::Fetched(f) => assert!(f.permanent_url.is_none()),
        other => panic!("expected Fetched, got {other:?}"),
    }
    drop(server);
}

#[test]
fn redirect_loop_errors_out() {
    let (server, base) = serve(|_| {
        tiny_http::Response::from_string("")
            .with_status_code(301)
            .with_header(header("Location", "/loop"))
    });
    let agent = fetch::agent();
    assert!(fetch::fetch_feed(&agent, &format!("{base}/loop"), None, None).is_err());
    drop(server);
}

#[test]
fn server_error_is_reported() {
    let (server, base) = serve(|_| tiny_http::Response::from_string("boom").with_status_code(500));
    let agent = fetch::agent();
    assert!(fetch::fetch_feed(&agent, &base, None, None).is_err());
    drop(server);
}
