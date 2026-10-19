//! Real loopback response sequences for the CLI's finite inbox-read phase.
use super::*;
use std::{io::Read, net::TcpListener, thread};

#[derive(Debug)]
struct Seen {
    path: String,
    body: Value,
    instance: String,
    activity: String,
    printed: usize,
}
#[derive(Clone, Default)]
struct Output(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn response_sequence(
    responses: Vec<(u16, String)>,
    out: Output,
) -> (Endpoint, thread::JoinHandle<Vec<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        for (status, response) in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing expected HTTP request");
                        thread::park_timeout(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n\r\n") {
                    break bytes.len();
                }
                assert!(bytes.len() < 16_384);
            };
            let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            let header = |name: &str| {
                headers
                    .lines()
                    .skip(1)
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case(name)
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap()
            };
            let length: usize = header("content-length").parse().unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            seen.push(Seen {
                path: headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .into(),
                body: serde_json::from_slice(&body).unwrap(),
                instance: header("x-towermd-instance"),
                activity: header("x-towermd-inbox-activity"),
                printed: out.0.lock().unwrap().len(),
            });
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            stream.flush().unwrap();
        }
        seen
    });
    (
        Endpoint {
            address: "127.0.0.1".into(),
            port,
            pid: 0,
            instance: "same-inbox-instance".into(),
            host: "host".into(),
        },
        worker,
    )
}
fn changed() -> (u16, String) {
    (409, json!({"code":"inbox_read_changed","error":"head changed before continuation","request_id":null}).to_string())
}
fn empty() -> (u16, String) {
    (200, json!({"messages":[],"delivery_token":""}).to_string())
}
fn http() -> reqwest::blocking::Client {
    client(Duration::from_secs(3)).unwrap()
}
fn directory() -> PathBuf {
    PathBuf::from("/source-bound-inbox")
}

#[test]
fn dedicated_read_code_repeats_only_the_same_read_before_any_output_or_ack() {
    for recovery in [false, true] {
        let mut out = Output::default();
        let (endpoint, worker) =
            response_sequence(vec![changed(), changed(), empty()], out.clone());
        deliver_inbox_at(
            &http(),
            &endpoint,
            directory(),
            Some("R-original".into()),
            recovery,
            &mut out,
        )
        .unwrap();
        let seen = worker.join().unwrap();
        assert_eq!(seen.len(), 3);
        for request in &seen {
            assert_eq!(request.path, "/inbox");
            assert_eq!(
                request.body,
                json!({"directory":directory(),"run":"R-original"})
            );
            assert_eq!(request.instance, endpoint.instance);
            assert_eq!(
                request.activity,
                if recovery { "recovery" } else { "project" }
            );
            assert_eq!(request.printed, 0);
        }
        assert!(out.0.lock().unwrap().is_empty());
    }
}

#[test]
fn read_retry_budget_is_three_total_requests_without_output_or_ack() {
    let mut out = Output::default();
    let (endpoint, worker) = response_sequence(vec![changed(), changed(), changed()], out.clone());
    let error =
        deliver_inbox_at(&http(), &endpoint, directory(), None, false, &mut out).unwrap_err();
    assert_eq!(error, "head changed before continuation");
    assert_eq!(worker.join().unwrap().len(), 3);
    assert!(out.0.lock().unwrap().is_empty());
}

#[test]
fn after_coordination_unknown_uuid_and_other_source_errors_never_repeat_the_read() {
    let cases = [
        (
            400,
            json!({"code":"request_refused","error":"head changed before continuation","request_id":null}),
            "head changed before continuation",
        ),
        (
            503,
            json!({"code":"request_uncertain","error":"request_id=original: uncertain","request_id":"original"}),
            "request_id=original: uncertain",
        ),
        (
            409,
            json!({"code":"inbox_read_changed","error":"head changed before continuation","request_id":"original"}),
            "head changed before continuation",
        ),
        (
            409,
            json!({"code":"inbox_read_changed","error":"authority changed","request_id":null}),
            "authority changed",
        ),
        (
            409,
            json!({"code":"instance_changed","error":"daemon instance changed"}),
            "daemon instance changed",
        ),
    ];
    for (status, body, expected) in cases {
        let mut out = Output::default();
        let (endpoint, worker) = response_sequence(vec![(status, body.to_string())], out.clone());
        let error =
            deliver_inbox_at(&http(), &endpoint, directory(), None, false, &mut out).unwrap_err();
        assert_eq!(error, expected);
        let seen = worker.join().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].path, "/inbox");
        assert!(out.0.lock().unwrap().is_empty());
    }
}

#[test]
fn invalid_response_is_not_retried_or_printed() {
    let mut out = Output::default();
    let (endpoint, worker) = response_sequence(vec![(409, "not json".into())], out.clone());
    let error =
        deliver_inbox_at(&http(), &endpoint, directory(), None, false, &mut out).unwrap_err();
    assert!(error.contains("invalid inbox response"));
    assert_eq!(worker.join().unwrap().len(), 1);
    assert!(out.0.lock().unwrap().is_empty());
}

#[test]
fn after_print_even_the_read_changed_code_cannot_repeat_ack_or_read() {
    let mut out = Output::default();
    let message = json!({"id":"H/A/R","hold":"H","run":"R","text":"actual answer","kind":"reply","actionable":false});
    let (endpoint, worker) = response_sequence(
        vec![
            changed(),
            (
                200,
                json!({"messages":[message],"delivery_token":"exact-token"}).to_string(),
            ),
            changed(),
        ],
        out.clone(),
    );
    let error = deliver_inbox_at(
        &http(),
        &endpoint,
        directory(),
        Some("R".into()),
        false,
        &mut out,
    )
    .unwrap_err();
    assert!(error.contains("acknowledgement uncertain"));
    assert!(error.contains("request_id="));
    let seen = worker.join().unwrap();
    assert_eq!(
        seen.iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/inbox", "/inbox", "/inbox/delivered"]
    );
    assert_eq!(seen[0].printed, 0);
    assert_eq!(seen[1].printed, 0);
    assert!(seen[2].printed > 0);
    assert_eq!(seen[2].body["delivery_token"], "exact-token");
    assert_eq!(seen[2].body["message_ids"], json!(["H/A/R"]));
    let bytes = out.0.lock().unwrap();
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
}
