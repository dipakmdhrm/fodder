//! End-to-end daemon test: real binary, temp data dir, loopback feed server.
//! Run under `dbus-run-session` in CI for an isolated session bus.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fodder_core::{ipc, Db, Settings};

const FEED_BODY: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>IT Feed</title>
<item><guid>a</guid><title>one</title><link>https://example.org/1</link></item>
<item><guid>b</guid><title>two</title><link>https://example.org/2</link></item>
</channel></rss>"#;

#[test]
fn daemon_polls_feed_into_db_and_quits() {
    let dir = tempfile::tempdir().unwrap();

    let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    {
        let server = server.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let _ = request.respond(tiny_http::Response::from_string(FEED_BODY));
            }
        });
    }

    let db_path = dir.path().join("fodder.db");
    {
        let db = Db::open(&db_path).unwrap();
        db.add_feed(&format!("http://127.0.0.1:{port}/feed"), 0)
            .unwrap();
        // No tray/notifications in a test environment.
        db.save_settings(&Settings {
            run_in_background: false,
            notifications: false,
            ..Settings::default()
        })
        .unwrap();
    }

    let bus_name = format!("io.github.dipakmdhrm.FodderTest{}", std::process::id());
    let mut child = Command::new(env!("CARGO_BIN_EXE_fodder-daemon"))
        .env("FODDER_DATA_DIR", dir.path())
        .env("FODDER_BUS_NAME", &bus_name)
        .env("RUST_LOG", "debug")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // The feed has never been polled, so the daemon must fetch it right away.
    let deadline = Instant::now() + Duration::from_secs(20);
    let feed_id = loop {
        assert!(Instant::now() < deadline, "daemon never stored the items");
        let db = Db::open(&db_path).unwrap();
        let feeds = db.list_feeds().unwrap();
        if let Some(feed) = feeds.first() {
            if db.list_items(feed.id).unwrap().len() == 2 {
                break feed.id;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    {
        let db = Db::open(&db_path).unwrap();
        let feed = db.get_feed(feed_id).unwrap().unwrap();
        assert_eq!(feed.title, "IT Feed");
        assert_eq!(feed.last_status.as_deref(), Some("ok"));
        assert_eq!(db.unread_total().unwrap(), 2);
    }

    // Prefer a graceful Quit over D-Bus; fall back to kill without a bus.
    let quit_sent = zbus::blocking::Connection::session()
        .and_then(|conn| {
            conn.call_method(
                Some(bus_name.as_str()),
                ipc::DAEMON_OBJECT_PATH,
                Some(ipc::DAEMON_INTERFACE),
                "Quit",
                &(),
            )
        })
        .is_ok();

    if quit_sent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                panic!("daemon did not exit after Quit");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    } else {
        child.kill().unwrap();
        let _ = child.wait();
    }
}
