use std::{path::PathBuf, sync::Arc};

use axum::Router;
use dmds::IoHandle;
use paper::Paper;
use serde::{Deserialize, Serialize};

mod paper;
mod questions;

#[derive(Debug)]
pub struct Global<Io: IoHandle> {
    config: Arc<Config>,
    papers: Arc<dmds::World<Paper, 2, Io>>,
}

impl<Io: IoHandle> Clone for Global<Io> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            papers: self.papers.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    db_path: PathBuf,
    port: u32,
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

    let state = Global {
        config: Arc::new(config),
        papers: Arc::new(dmds::world! {
            // 32chunks, 1chunk
            dmds_tokio_fs::FsHandle::new(paper_path, true), 576460752303423488 | .., 18446744073709551615 | ..
        }),
    };

    let router = Router::new().with_state(state);
    axum::serve(
        tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .unwrap(),
        router,
    )
    .await
    .unwrap();
}

#[derive(Serialize)]
pub struct ResponseWithMsg {
    code: u16,
    #[serde(rename = "message")]
    msg: String,
}
