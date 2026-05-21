use super::*;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[tokio::test]
async fn test_workspace_router_creates_workspace() {
    let response = router(AppState::default())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workspaces")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"tenant_id":"t1","name":"Main","provider":"p","model":"m"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_workspace_router_rejects_empty_workspace_name() {
    let response = router(AppState::default())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workspaces")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":" "}"#))
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_message_router_deduplicates_message() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let app = router(state);
    let uri = format!(
        "/api/workspaces/{}/chats/{}/messages",
        workspace.id, chat.id
    );

    let first = post_message(app.clone(), &uri).await;
    let second = post_message(app, &uri).await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("\"deduplicated\":true"));
}

#[tokio::test]
async fn test_chat_router_updates_chat_title() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "Old".into())
        .expect("chat created");
    let app = router(state);
    let uri = format!("/api/workspaces/{}/chats/{}", workspace.id, chat.id);

    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"New"}"#))
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("\"title\":\"New\""));
}

#[tokio::test]
async fn test_message_router_lists_messages_with_pagination() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "first".into(),
            "k1".into(),
        )
        .expect("first message added");
    let second = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "second".into(),
            "k2".into(),
        )
        .expect("second message added")
        .message;
    let third = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "third".into(),
            "k3".into(),
        )
        .expect("third message added")
        .message;
    let app = router(state);
    let uri = format!(
        "/api/workspaces/{}/chats/{}/messages?limit=2",
        workspace.id, chat.id
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains(&format!("\"id\":\"{}\"", third.id)));
    assert!(body.contains(&format!("\"id\":\"{}\"", second.id)));
    assert!(body.contains("\"has_more\":true"));
    assert!(body.contains(&format!("\"next_cursor\":\"{}\"", second.id)));
}

#[tokio::test]
async fn test_message_router_returns_not_found_for_unknown_chat() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let app = router(state);
    let uri = format!(
        "/api/workspaces/{}/chats/missing/messages?limit=2",
        workspace.id
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_router_analyzes_message() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let app = router(state);
    let uri = format!("/api/workspaces/{}/chats/{}/analyze", workspace.id, chat.id);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"请实现一个新功能"}"#))
                .unwrap(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("\"message_type\":\"requirement\""));
}

async fn post_message(app: Router, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"role":"user","content":"hello","idempotency_key":"k1"}"#,
            ))
            .unwrap(),
    )
    .await
    .expect("request succeeds")
}
