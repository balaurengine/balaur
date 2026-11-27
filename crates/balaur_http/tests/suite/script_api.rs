//! The http bindings called the way a game calls them: from a script, through
//! `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! One app boot per scenario, covering the callback path and the await path
//! in a single script — booting an app dominates the cost, so scenarios share
//! one.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_testkit::run_until;

/// Serve one canned HTTP/1.1 response on a fresh port, returning the url.
fn serve_one(response: impl Into<String>) -> String {
    let response = response.into();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}")
}

/// Serve one request and answer with the body it carried, returning the url.
fn echo_body() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        let body = loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            seen.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&seen).into_owned();
            let Some(end) = text.find("\r\n\r\n") else {
                if n == 0 {
                    break String::new();
                }
                continue;
            };
            let length = text[..end]
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if n == 0 || seen.len() >= end + 4 + length {
                break text[end + 4..].to_string();
            }
        };
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
    });
    format!("http://{addr}")
}

/// Serve one request and answer with the `content-type` it carried and its
/// body's bytes in hex, so bytes no string holds still read back exactly.
fn echo_body_bytes() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head, body): (String, Vec<u8>) = loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            seen.extend_from_slice(&chunk[..n]);
            let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") else {
                if n == 0 {
                    return;
                }
                continue;
            };
            let head = String::from_utf8_lossy(&seen[..end]).into_owned();
            let length = header(&head, "content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if n == 0 || seen.len() >= end + 4 + length {
                let body = seen[end + 4..].iter().take(length).copied().collect();
                break (head, body);
            }
        };
        let mut hex = String::new();
        for byte in &body {
            let _ = write!(hex, "{byte:02x}");
        }
        let content_type = header(&head, "content-type").unwrap_or_default();
        let answer = format!("{content_type} {hex}");
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
            answer.len()
        );
        let _ = stream.write_all(reply.as_bytes());
    });
    format!("http://{addr}")
}

/// The value of the header `name` in a request's head, any case.
fn header(head: &str, name: &str) -> Option<String> {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
}

#[test]
fn a_put_carries_body_bytes_as_they_are_with_its_content_type() {
    let url = echo_body_bytes();
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let headers = [http::Header {{ name: "Content-Type", value: "image/jpeg" }}];
        let options = http::RequestOptions {{ method: "PUT", headers: headers, body_bytes: [0, 255, 128, 74] }};
        let r = await task::wait(http::request("{url}", options));
        let status = r["status"];
        let body = r["body"];
        log::info("put-bytes {{status}} {{body}}");
    }}
}}
"#
    );
    run_until(&source, &["put-bytes 200 image/jpeg 00ff804a"]);
}

// The count is read in the call that starts a request: its answer lands at
// a later frame's pump, so 1 holds whenever the server answers.
#[test]
fn in_flight_counts_a_request_until_its_answer_is_woken() {
    let ok = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";
    let answered = serve_one(ok);
    let cancelled = serve_one(ok);
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let before = http::in_flight();
        let request = http::request("{answered}");
        let out = http::in_flight();
        let r = await task::wait(request);
        let status = r["status"];
        let after = http::in_flight();
        let dropped = http::request("{cancelled}");
        http::cancel(dropped);
        let untold = http::in_flight();
        let c = await task::wait(dropped);
        let kind = c["kind"];
        let told = http::in_flight();
        log::info("in-flight {{before}} {{out}} {{status}} {{after}} {{untold}} {{kind}} {{told}}");
    }}
}}
"#
    );
    run_until(&source, &["in-flight 0 1 200 0 1 cancelled 0"]);
}

#[test]
fn a_delete_carries_the_body_it_was_given() {
    let url = echo_body();
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let r = await task::wait(http::request("{url}", http::RequestOptions {{ method: "DELETE", body: "ids=7" }}));
        let status = r["status"];
        let body = r["body"];
        log::info("delete-echo {{status}} {{body}}");
    }}
}}
"#
    );
    run_until(&source, &["delete-echo 200 ids=7"]);
}

#[test]
fn a_script_awaits_one_request_after_another() {
    let awaited =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nraven");
    let handled =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nrhino");
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let r = await task::wait(http::request("{awaited}"));
        let status = r["status"];
        let body = r["body"];
        log::info("first {{status}} {{body}}");
        let r = await task::wait(http::request("{handled}"));
        let status = r["status"];
        let body = r["body"];
        log::info("second {{status}} {{body}}");
    }}
}}
"#
    );
    run_until(&source, &["first 200 raven", "second 200 rhino"]);
}

#[test]
fn answers_that_came_before_anything_waited_are_kept_until_waited_on() {
    let urls: Vec<String> = (0..20)
        .map(|i| {
            let reply =
                format!("HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{i:02}");
            serve_one(reply)
        })
        .collect();
    let list = urls
        .iter()
        .map(|url| format!("\"{url}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let urls = [{list}];
        let ids: [int] = [];
        for url in urls {{
            ids.push(http::request(url));
        }}
        while http::in_flight() > 0 {{
            await task::frames(1);
        }}
        let bodies = "";
        for id in ids {{
            let r = await task::wait(id);
            let body = r["body"].as_str();
            bodies = "{{bodies}}{{body}}";
        }}
        log::info("kept {{bodies}}");
    }}
}}
"#
    );
    let all = (0..20).fold(String::new(), |mut all, i| {
        let _ = write!(all, "{i:02}");
        all
    });
    run_until(&source, &[&format!("kept {all}")]);
}
