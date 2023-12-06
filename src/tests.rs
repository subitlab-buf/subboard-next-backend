use std::{path::PathBuf, sync::Arc};

use axum::{
    http::{self, Request},
    Router,
};
use dmds::mem_io_handle::MemStorage;
use tower::ServiceExt;

use crate::{paper, question, Config, Global};

fn router() -> (Global<MemStorage>, Router) {
    use axum::routing::{get, post};

    let config = Config {
        db_path: PathBuf::new(),
        port: 8080,
        mng_secret: "secret".to_owned(),
        mng_get_papers_secret: "get_papers".to_owned(),
        mng_approve_papers_secret: "approve_papers".to_owned(),
        mng_reject_papers_secret: "reject_papers".to_owned(),
    };

    let state = Global {
        config: Arc::new(config),
        papers: Arc::new(dmds::world! {
            // 32 chunks, 2 chunk
            MemStorage::new(), 576460752303423488 | ..=u64::MAX, 1 | ..=1
        }),
        questions: Arc::new(dmds::world! {
            // 32 chunks
            MemStorage::new(), 1152921504606846976 | ..=u64::MAX
        }),
    };

    (
        state.clone(),
        Router::new()
            .route("/questions/new", post(question::new::<MemStorage>))
            .route("/paper/post", post(paper::post::<MemStorage>))
            .route("/paper/get", get(paper::get::<MemStorage>))
            .route("/secret/get_papers", get(paper::unprocessed::<MemStorage>))
            .route("/secret/approve_papers", post(paper::approve::<MemStorage>))
            .route("/secret/reject_papers", post(paper::reject::<MemStorage>))
            .with_state(state),
    )
}

#[tokio::test]
async fn new_question() {
    let (_, route) = router();
    let question = question::In {
        name: "Yjn024".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };

    assert!(route
        .oneshot(
            Request::builder()
                .uri("/questions/new")
                .method("POST")
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(serde_json::to_string(&question).unwrap())
                .unwrap()
        )
        .await
        .unwrap()
        .status()
        .is_success());
}
