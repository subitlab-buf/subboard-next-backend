use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::Global;

#[derive(Debug, Clone)]
pub struct Paper {
    /// Paper author's name.
    name: String,
    /// Paper content.
    info: String,
    /// Paper author's email.
    email: Option<lettre::Address>,

    pid: u64,
    time: DateTime<Utc>,
}

/// Paper from frontend.
#[derive(Serialize, Deserialize, Hash)]
pub struct Raw {
    name: String,
    info: String,
    email: Option<lettre::Address>,
}

impl Paper {
    fn to_raw(&self) -> Raw {
        Raw {
            name: self.name.clone(),
            info: self.info.clone(),
            email: self.email.clone(),
        }
    }
}

impl From<Raw> for Paper {
    fn from(value: Raw) -> Self {
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };

        Self {
            name: value.name,
            info: value.info,
            email: value.email,
            pid: hash,
            time: Utc::now(),
        }
    }
}

impl dmds::Data for Paper {
    const DIMS: usize = 2;

    #[inline]
    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.pid,
            1 => self.time.timestamp() as u64,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(dims: &[u64], buf: B) -> std::io::Result<Self> {
        let inner: Raw = bincode::deserialize_from(buf.reader())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;

        Ok(Self {
            name: inner.name,
            info: inner.info,
            email: inner.email,
            pid: dims[0],
            time: DateTime::from_timestamp(dims[1] as i64, 0).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("unable to create a DateTime from timestamp {}", dims[1]),
                )
            })?,
        })
    }

    fn encode<B: bytes::BufMut>(&self, buf: B) -> std::io::Result<()> {
        bincode::serialize_into(buf.writer(), &self.to_raw())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("internal database error")]
    Db,
    #[error("pid conflicted")]
    PidConflict,
    #[error("paper repository is empty")]
    NoPaper,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct JErr {
            error: String,
        }

        (
            match self {
                Error::Db => StatusCode::INTERNAL_SERVER_ERROR,
                Error::PidConflict => StatusCode::CONFLICT,
                Error::NoPaper => StatusCode::NOT_FOUND,
            },
            Json(JErr {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub async fn post<Io: IoHandle>(
    Json(paper): Json<Raw>,
    State(Global { papers, .. }): State<Global<Io>>,
) -> Result<(), Error> {
    let paper: Paper = paper.into();
    let pid = paper.pid;
    info!("inserting new paper: {:?}", paper);
    papers.try_insert(paper).await.map_err(|_| {
        error!("papers with pid {pid} conflicted");
        Error::PidConflict
    })
}

pub async fn get<Io: IoHandle>(
    State(Global { papers, .. }): State<Global<Io>>,
) -> Result<Json<Raw>, Error> {
    let select = papers.select_all();
    let pids: Vec<u64> = select
        .iter()
        .filter_map(|e| e.ok().map(|lazy| lazy.id()))
        .collect()
        .await;
    let pid = pids
        .get(fastrand::usize(..pids.len()))
        .copied()
        .ok_or(Error::NoPaper)?;

    let select = papers.select(0, pid).hint(pid);
    let mut iter = select.iter();
    while let Some(Ok(lazy)) = iter.next().await {
        if lazy.id() == pid {
            if let Ok(val) = lazy.get().await {
                return Ok(Json(val.to_raw()));
            }
        }
    }

    Err(Error::Db)
}
