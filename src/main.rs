use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{
    routing::{get, post},
    Router,
};
use dmds::IoHandle;
use dmds_tokio_fs::FsHandle;
use paper::Paper;
use question::Question;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

mod paper;
mod question;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct Global<Io: IoHandle> {
    config: Arc<Config>,
    papers: Arc<dmds::World<Paper, 2, Io>>,
    questions: Arc<dmds::World<Question, 1, Io>>,
}

impl<Io: IoHandle> Clone for Global<Io> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            papers: self.papers.clone(),
            questions: self.questions.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    db_path: PathBuf,
    port: u32,

    /// Root secret mapping.
    mng_secret: String,
    /// Secret mapping for management clients to get
    /// all unprocessed papers.
    mng_get_papers_secret: String,
    /// Secret mapping for management clients to approve papers.
    mng_approve_papers_secret: String,
    /// Secret mapping for management clients to reject papers.
    mng_reject_papers_secret: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config: Config = toml::from_str(&{
        use std::io::Read;
        let mut str = String::new();
        std::fs::File::open("config.toml")
            .unwrap()
            .read_to_string(&mut str)
            .unwrap();
        str
    })
    .unwrap();
    let port = config.port;

    let mut paper_path = config.db_path.clone();
    paper_path.push("papers");

    let mut questions_path = config.db_path.clone();
    questions_path.push("questions");

    let config = Arc::new(config);

    let state = Global {
        config: config.clone(),
        papers: Arc::new(dmds::world! {
            // 32 chunks, 1 chunk
            dmds_tokio_fs::FsHandle::new(paper_path, false), 576460752303423488u64 | ..=u64::MAX, 1 | ..=1
        }),
        questions: Arc::new(dmds::world! {
            // 32 chunks
            dmds_tokio_fs::FsHandle::new(questions_path, true), 1152921504606846976u64 | ..=u64::MAX
        }),
    };

    let router: Router<()> = Router::new()
        .layer(CorsLayer::permissive())
        .route("/questions/new", post(question::new::<FsHandle>))
        .route("/paper/post", post(paper::post::<FsHandle>))
        .route("/paper/get", get(paper::get::<FsHandle>))
        .route(
            &format!("/{}/{}", config.mng_secret, config.mng_get_papers_secret),
            get(paper::unprocessed::<FsHandle>),
        )
        .route(
            &format!(
                "/{}/{}",
                config.mng_secret, config.mng_approve_papers_secret
            ),
            post(paper::approve::<FsHandle>),
        )
        .route(
            &format!("/{}/{}", config.mng_secret, config.mng_reject_papers_secret),
            post(paper::reject::<FsHandle>),
        )
        .with_state(state.clone());

    tokio::spawn(dmds_tokio_fs::daemon(
        state.papers.clone(),
        Duration::from_secs(45),
    ));
    tokio::spawn(dmds_tokio_fs::daemon(
        state.questions.clone(),
        Duration::from_secs(120),
    ));

    axum::serve(
        tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .unwrap(),
        router,
    )
    .await
    .unwrap();
}
