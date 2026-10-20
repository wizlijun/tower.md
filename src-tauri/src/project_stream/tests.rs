use super::*;
use std::sync::atomic::AtomicUsize;
use serde_json::json;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}
fn frame(seq: u64) -> Message {
    Message::Text(json!({"kind":"state","state":{"status":"ready","seq":seq,"projection":{}}}).to_string().into())
}
fn target(port: u16, instance: &str) -> crate::backend::StreamEndpoint {
    crate::backend::StreamEndpoint { port, instance: instance.into(), handle: "project-handle".into() }
}
fn delivery() -> (Deliver, mpsc::UnboundedReceiver<StreamMessage>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Arc::new(move |message| sender.send(message).map_err(|_| ())), receiver)
}
async fn next(receiver: &mut mpsc::UnboundedReceiver<StreamMessage>) -> StreamMessage {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await.unwrap().unwrap()
}
async fn accept(listener: TcpListener, instance: &'static str) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let (socket, _) = listener.accept().await.unwrap();
    tokio_tungstenite::accept_hdr_async(socket, move |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/projects/project-handle/events");
        assert_eq!(request.headers()["x-towermd-client"], "app");
        assert_eq!(request.headers()["x-towermd-instance"], instance);
        Ok(response)
    }).await.unwrap()
}

#[test]
fn only_full_state_frames_cross_the_native_channel() {
    for status in ["not_activated", "needs_recovery"] {
        let state = json!({"status":status});
        assert_eq!(state_frame(&json!({"kind":"state","state":state}).to_string()).unwrap(), state);
    }
    for frame in [json!({"kind":"state","state":{"status":"ready","projection":{}}}),
        json!({"kind":"state","state":{"status":"ready","seq":1}}),
        json!({"kind":"error","error":"private diagnostic"}), json!({"status":"ready"})] {
        assert!(state_frame(&frame.to_string()).is_err());
    }
    assert_eq!(serde_json::to_value(message("disconnected", None)).unwrap(),
        json!({"subscription":"","token":"","status":"disconnected"}));
}

#[test]
fn real_socket_delivers_first_state_immediately_and_keeps_a_fixed_window_under_continuous_changes() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut socket = accept(listener, "fixed-window").await;
            socket.send(frame(1)).await.unwrap();
            for seq in 2..=30 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if socket.send(frame(seq)).await.is_err() { break; }
            }
        });
        let streams = Streams::default(); let (emit, mut receiver) = delivery();
        let id = uuid::Uuid::now_v7().to_string();
        streams.start("selected".into(), id.clone(), Arc::new(move || Ok(target(port, "fixed-window"))), emit).unwrap();
        let first = tokio::time::timeout(Duration::from_millis(900), receiver.recv()).await.unwrap().unwrap();
        assert_eq!(first.status, "connected"); assert_eq!(first.state.unwrap()["seq"], 1);
        assert_eq!(first.token, "selected"); assert_eq!(first.subscription, id);
        let second = tokio::time::timeout(Duration::from_millis(1500), receiver.recv()).await.unwrap().unwrap();
        let seq = second.state.unwrap()["seq"].as_u64().unwrap();
        assert!(seq > 2 && seq < 20, "continuous updates must not postpone delivery: {seq}");
        streams.unsubscribe(&id).unwrap(); server.abort();
    });
}

#[test]
fn replacing_and_cancelling_subscriptions_closes_sockets_and_old_unsubscribe_cannot_stop_new_selection() {
    runtime().block_on(async {
        let first_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_port = first_listener.local_addr().unwrap().port();
        let second_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second_port = second_listener.local_addr().unwrap().port();
        let serve = |listener, credential, seq| tokio::spawn(async move {
            let mut socket = accept(listener, credential).await;
            socket.send(frame(seq)).await.unwrap();
            // Aborting the native task must actually release its TCP connection.
            let ended = tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap();
            assert!(matches!(ended, None | Some(Err(_)) | Some(Ok(Message::Close(_)))));
        });
        let first_server = serve(first_listener, "first", 10);
        let second_server = serve(second_listener, "second", 20);
        let streams = Streams::default(); let (emit, mut receiver) = delivery();
        let old = uuid::Uuid::now_v7().to_string(); let current = uuid::Uuid::now_v7().to_string();
        streams.start("old-project".into(), old.clone(), Arc::new(move || Ok(target(first_port, "first"))), Arc::clone(&emit)).unwrap();
        assert_eq!(next(&mut receiver).await.subscription, old);
        streams.start("new-project".into(), current.clone(), Arc::new(move || Ok(target(second_port, "second"))), Arc::clone(&emit)).unwrap();
        streams.unsubscribe(&old).unwrap();
        let current_message = next(&mut receiver).await;
        assert_eq!(current_message.subscription, current); assert_eq!(current_message.token, "new-project");
        first_server.await.unwrap();
        assert!(!second_server.is_finished(), "old unsubscribe must leave the current socket alive");
        assert!(streams.start("wrong-project".into(), current.clone(), Arc::new(move || Ok(target(first_port, "first"))), emit).is_err());
        streams.cancel(); second_server.await.unwrap();
    });
}

#[test]
fn disconnect_rediscovers_endpoint_and_delivers_a_fresh_full_state_after_daemon_restart() {
    runtime().block_on(async {
        let first_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_port = first_listener.local_addr().unwrap().port();
        let second_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second_port = second_listener.local_addr().unwrap().port();
        let first_server = tokio::spawn(async move {
            let mut socket = accept(first_listener, "before-restart").await;
            socket.send(frame(99)).await.unwrap(); socket.close(None).await.unwrap();
        });
        let second_server = tokio::spawn(async move {
            let mut socket = accept(second_listener, "after-restart").await;
            socket.send(frame(5)).await.unwrap();
            let _ = socket.next().await;
        });
        let calls = Arc::new(AtomicUsize::new(0)); let resolved = Arc::clone(&calls);
        let resolver: Resolve = Arc::new(move || {
            Ok(if resolved.fetch_add(1, Ordering::SeqCst) == 0 { target(first_port, "before-restart") }
                else { target(second_port, "after-restart") })
        });
        let streams = Streams::default(); let (emit, mut receiver) = delivery();
        streams.start("same-project".into(), uuid::Uuid::now_v7().to_string(), resolver, emit).unwrap();
        assert_eq!(next(&mut receiver).await.state.unwrap()["seq"], 99);
        let lost = next(&mut receiver).await; assert_eq!(lost.status, "disconnected"); assert!(lost.state.is_none());
        let renewed = next(&mut receiver).await; assert_eq!(renewed.status, "connected"); assert_eq!(renewed.state.unwrap()["seq"], 5);
        assert!(calls.load(Ordering::SeqCst) >= 2);
        streams.cancel(); first_server.await.unwrap(); second_server.await.unwrap();
    });
}

#[test]
fn final_verified_state_is_delivered_before_disconnection_even_inside_the_window() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut socket = accept(listener, "last-alert").await;
            socket.send(frame(1)).await.unwrap();
            socket.send(Message::Text(json!({"kind":"state","state":{"status":"ready","seq":2,
                "projection":{"Alert":{"main":{"kind":"unapproved"}}}}}).to_string().into())).await.unwrap();
            socket.send(Message::Text(json!({"kind":"error","error":"stream unavailable"}).to_string().into())).await.unwrap();
        });
        let streams = Streams::default(); let (emit, mut receiver) = delivery();
        streams.start("project".into(), uuid::Uuid::now_v7().to_string(), Arc::new(move || Ok(target(port, "last-alert"))), emit).unwrap();
        assert_eq!(next(&mut receiver).await.state.unwrap()["seq"], 1);
        let final_state = next(&mut receiver).await;
        assert_eq!(final_state.status, "connected"); assert_eq!(final_state.state.unwrap()["projection"]["Alert"]["main"]["kind"], "unapproved");
        assert_eq!(next(&mut receiver).await.status, "disconnected");
        streams.cancel(); server.await.unwrap();
    });
}
