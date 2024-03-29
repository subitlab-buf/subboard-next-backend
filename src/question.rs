use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use dmds::IoHandle;
use serde::{Deserialize, Serialize};
use siphasher::sip::SipHasher24;

use crate::Global;

/// Question from frontend.
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct In {
    pub name: String,
    pub info: String,
    pub email: Option<lettre::Address>,
}

/// Feedback to SubIT.
#[derive(Debug)]
pub struct Question {
    /// Name of the questioner.
    pub name: String,
    /// Content of the question.
    pub info: String,
    /// Email address of the questioner.
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

impl Question {
    fn to_store(&self) -> Store {
        Store {
            name: self.name.clone(),
            info: self.info.clone(),
            email: self.email.clone(),
            time: self.time,
        }
    }
}

impl From<In> for Question {
    fn from(value: In) -> Self {
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = SipHasher24::new();
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

impl dmds::Data for Question {
    const DIMS: usize = 1;
    const VERSION: u32 = 1;

    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.pid,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(version: u32, dims: &[u64], buf: B) -> std::io::Result<Self> {
        match version {
            1 => {
                let inner: Store = bincode::deserialize_from(buf.reader())
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
                Ok(Self {
                    name: inner.name,
                    info: inner.info,
                    email: inner.email,
                    pid: dims[0],
                    time: inner.time,
                })
            }
            _ => unreachable!(),
        }
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
            },
            Json(JErr {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub async fn new<Io: IoHandle>(
    State(Global { questions, .. }): State<Global<Io>>,
    Json(question): Json<In>,
) -> Result<(), Error> {
    let result = questions.insert(question.into()).await.map_err(|err| {
        tracing::error!("insert question failed: {}", err);
        Error::Db
    })?;
    if result.is_some() {
        Err(Error::PidConflict)
    } else {
        Ok(())
    }
}
