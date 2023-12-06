use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::Global;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr,
)]
#[repr(u8)]
pub enum Status {
    Pending,
    Approved,
}

#[derive(Debug, Clone, Serialize)]
pub struct Paper {
    /// Paper author's name.
    pub name: String,
    /// Paper content.
    pub info: String,
    /// Paper author's email.
    pub email: Option<lettre::Address>,

    /// Only identifier of this paper.
    pub pid: u64,
    /// Post time
    time: DateTime<Utc>,

    pub status: Status,
}

/// Paper from frontend.
#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct In {
    pub name: String,
    pub info: String,
    pub email: Option<lettre::Address>,
}

/// Paper to frontend.
#[derive(Debug, Serialize, Deserialize)]
pub struct Out {
    pub name: String,
    pub info: String,
    pub email: Option<lettre::Address>,
    pub pid: u64,
    time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Store {
    name: String,
    info: String,
    email: Option<lettre::Address>,
    time: DateTime<Utc>,
}

impl Paper {
    #[inline]
    fn approve(&mut self) {
        self.status = Status::Approved;
    }

    fn to_out(&self) -> Out {
        Out {
            name: self.name.clone(),
            info: self.info.clone(),
            email: self.email.clone(),
            pid: self.pid,
            time: self.time,
        }
    }

    fn to_store(&self) -> Store {
        Store {
            name: self.name.clone(),
            info: self.info.clone(),
            email: self.email.clone(),
            time: self.time,
        }
    }
}

impl From<In> for Paper {
    fn from(value: In) -> Self {
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
            status: Status::Pending,
        }
    }
}

impl dmds::Data for Paper {
    const DIMS: usize = 2;

    #[inline]
    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.pid,
            1 => self.status as u8 as u64,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(dims: &[u64], buf: B) -> std::io::Result<Self> {
        let inner: Store = bincode::deserialize_from(buf.reader())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;

        Ok(Self {
            name: inner.name,
            info: inner.info,
            email: inner.email,
            time: inner.time,
            pid: dims[0],
            status: if dims[1] as u8 == Status::Pending as u8 {
                Status::Pending
            } else {
                Status::Approved
            },
        })
    }

    fn encode<B: bytes::BufMut>(&self, buf: B) -> std::io::Result<()> {
        bincode::serialize_into(buf.writer(), &self.to_store())
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
    #[error("requiring paper not found")]
    NotFound,
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
                Error::NoPaper | Error::NotFound => StatusCode::NOT_FOUND,
            },
            Json(JErr {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub async fn post<Io: IoHandle>(
    State(Global { papers, .. }): State<Global<Io>>,
    Json(paper): Json<In>,
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
) -> Result<Json<Paper>, Error> {
    let select = papers.select(1, Status::Approved as u8 as u64);
    let pid = fastrand::choice(
        select
            .iter()
            .filter_map(|e| e.ok().map(|lazy| lazy.id()))
            .collect::<Vec<u64>>()
            .await
            .into_iter(),
    )
    .ok_or(Error::NoPaper)?;

    let select = papers.select(0, pid).hint(pid);
    let mut papers_iter = select.iter();
    while let Some(Ok(lazy)) = papers_iter.next().await {
        if lazy.id() == pid {
            if let Ok(val) = lazy.get().await {
                return Ok(Json(val.clone()));
            }
        }
    }

    Err(Error::Db)
}

pub async fn unprocessed<Io: IoHandle>(
    State(Global { papers, .. }): State<Global<Io>>,
) -> Json<Vec<Out>> {
    let select = papers.select(1, Status::Pending as u8 as u64);
    let mut papers_iter = select.iter();

    let mut ret = Vec::new();
    while let Some(Ok(lazy)) = papers_iter.next().await {
        if let Ok(val) = lazy.get().await {
            ret.push(val.to_out());
        }
    }
    Json(ret)
}

#[derive(Deserialize, Debug)]
pub struct ApprRejReq {
    pid: u64,
}

pub async fn approve<Io: IoHandle>(
    State(Global { papers, .. }): State<Global<Io>>,
    Json(ApprRejReq { pid }): Json<ApprRejReq>,
) -> Result<(), Error> {
    let select = papers
        .select(0, pid)
        .and(1, Status::Pending as u8 as u64)
        .hint(pid);
    let mut papers_iter = select.iter();

    while let Some(Ok(mut lazy)) = papers_iter.next().await {
        if lazy.id() == pid {
            if let Ok(paper) = lazy.get_mut().await {
                info!("approving paper {pid}");
                paper.approve();
                return lazy.close().await.map_err(|err| {
                    error!("failed to approve paper: {err}");
                    Error::Db
                });
            }
        }
    }

    Err(Error::NotFound)
}

pub async fn reject<Io: IoHandle>(
    State(Global { papers, .. }): State<Global<Io>>,
    Json(ApprRejReq { pid }): Json<ApprRejReq>,
) -> Result<(), Error> {
    let select = papers
        .select(0, pid)
        .and(1, Status::Pending as u8 as u64)
        .hint(pid);
    let mut papers_iter = select.iter();

    while let Some(Ok(lazy)) = papers_iter.next().await {
        if lazy.id() == pid {
            info!("rejecting paper {pid}");
            return lazy.destroy().await.map_err(|err| {
                error!("failed to remove paper: {err}");
                Error::Db
            });
        }
    }

    Err(Error::NotFound)
}
