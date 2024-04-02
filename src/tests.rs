use std::{path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    http::{self, Request},
    Router,
};
use dmds::{mem_io_handle::MemStorage, StreamExt};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::{paper, question, Config, Global};

fn router() -> (Global<MemStorage>, Router) {
    use axum::routing::{get, post};

    let config = Config {
        db_path: PathBuf::new(),
        port: 8080,
        static_path: PathBuf::new(),
        mng_secret: "secret".to_owned(),
        mng_get_papers_secret: "get_papers".to_owned(),
        mng_approve_papers_secret: "approve_papers".to_owned(),
        mng_reject_papers_secret: "reject_papers".to_owned(),
    };

    let state = Global {
        config: Arc::new(config),
        papers: Arc::new(dmds::world! {
            // 32 chunks, 2 chunk
            MemStorage::new(), 576460752303423488u64 | ..=u64::MAX, 1 | ..=1
        }),
        questions: Arc::new(dmds::world! {
            // 32 chunks
            MemStorage::new(), 1152921504606846976u64 | ..=u64::MAX
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
    let (state, route) = router();
    let question = question::In {
        name: "Yjn024".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };

    assert!(route
        .oneshot(
            Request::builder()
                .uri("/questions/new")
                .method(http::Method::POST)
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(serde_json::to_string(&question).unwrap())
                .unwrap()
        )
        .await
        .unwrap()
        .status()
        .is_success());

    let select = state.questions.select_all();
    let mut iter = select.iter();

    while let Some(Ok(lazy)) = iter.next().await {
        if let Ok(question::Question {
            name, info, email, ..
        }) = lazy.get().await
        {
            assert_eq!(name, "Yjn024");
            assert_eq!(info, "Hello, world!");
            assert!(email.is_none());

            return;
        }
    }
    unreachable!("data not inserted");
}

#[tokio::test]
async fn post_paper() {
    let (state, route) = router();
    let paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };

    assert!(route
        .oneshot(
            Request::builder()
                .uri("/paper/post")
                .method(http::Method::POST)
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(serde_json::to_string(&paper).unwrap())
                .unwrap()
        )
        .await
        .unwrap()
        .status()
        .is_success());

    let select = state.papers.select(1, paper::Status::Pending as u8 as u64);
    let mut iter = select.iter();

    while let Some(Ok(lazy)) = iter.next().await {
        if let Ok(paper::Paper {
            name,
            info,
            email,
            status,
            ..
        }) = lazy.get().await
        {
            assert_eq!(name, "Yjn024");
            assert_eq!(info, "Hello, world!");
            assert!(email.is_none());
            assert_eq!(*status, paper::Status::Pending);

            return;
        }
    }
    unreachable!("data not inserted");
}

#[tokio::test]
async fn get_paper() {
    let (state, route) = router();
    let paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };
    state.papers.insert(paper.into()).await.unwrap();

    assert!(!route
        .clone()
        .oneshot(
            Request::builder()
                .uri("/paper/get")
                .method(http::Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .is_success());

    let paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Genshine Impact".to_owned(),
        email: None,
    };
    let mut paper: paper::Paper = paper.into();
    paper.status = paper::Status::Approved;
    state.papers.insert(paper.into()).await.unwrap();

    let res = route
        .oneshot(
            Request::builder()
                .uri("/paper/get")
                .method(http::Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success());
    let paper::Out {
        name, info, email, ..
    } = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(name, "Yjn024");
    assert_eq!(info, "Genshine Impact");
    assert!(email.is_none());
}

#[tokio::test]
async fn unprocessed_papers() {
    let (state, route) = router();
    let paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Genshine Impact".to_owned(),
        email: None,
    };
    let mut paper: paper::Paper = paper.into();
    paper.status = paper::Status::Approved;
    state.papers.insert(paper.into()).await.unwrap();

    let paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };
    state.papers.insert(paper.into()).await.unwrap();

    let paper = paper::In {
        name: "c191239".to_owned(),
        info: "Hello, world!".to_owned(),
        email: None,
    };
    state.papers.insert(paper.into()).await.unwrap();

    let res = route
        .oneshot(
            Request::builder()
                .uri("/secret/get_papers")
                .method(http::Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success());
    let res: Vec<paper::Out> =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(res.len(), 2)
}

#[tokio::test]
async fn approve_paper() {
    let (state, route) = router();
    let paper: paper::Paper = paper::In {
        name: "Yjn024".to_owned(),
        info: "Genshine Impact".to_owned(),
        email: None,
    }
    .into();
    let pid = paper.pid;
    state.papers.insert(paper).await.unwrap();

    assert!(route
        .oneshot(
            Request::builder()
                .uri("/secret/get_papers")
                .method(http::Method::GET)
                .body(serde_json::to_string(&paper::ApprRejReq { pid }).unwrap())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .is_success());

    let select = state
        .papers
        .select(0, pid)
        .and(1, paper::Status::Approved as u8 as u64);
    let mut iter = select.iter();

    while let Some(Ok(lazy)) = iter.next().await {
        if let Ok(paper) = lazy.get().await {
            assert_eq!(paper.pid, pid);
            assert_eq!(paper.status, paper::Status::Approved);
        }
    }
}
