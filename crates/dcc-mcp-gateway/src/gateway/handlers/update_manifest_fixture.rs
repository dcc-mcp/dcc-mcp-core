//! Local HTTP fixtures for gateway update endpoint tests.

use axum::{Json, body::Body, http::StatusCode};
use serde_json::Value;

pub(super) async fn spawn_update_manifest(
    manifest: Value,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/manifest.json",
            axum::routing::get(move || {
                let manifest = manifest.clone();
                async move { Json(manifest) }
            }),
        )
        .route(
            "/latest",
            axum::routing::get(|| async { axum::response::Redirect::temporary("/manifest.json") }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/latest",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (url, tx)
}

pub(super) async fn spawn_update_manifest_response(
    status: StatusCode,
    content_type: &'static str,
    body: &'static str,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new().route(
        "/manifest.json",
        axum::routing::get(move || async move {
            axum::response::Response::builder()
                .status(status)
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/manifest.json",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (url, tx)
}
