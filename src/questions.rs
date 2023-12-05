use serde::Serialize;

/// Question from frontend
#[derive(Debug, Clone, Serialize)]
pub struct Simple {
    /// Name of question's author
    name: String,
    /// Question content
    info: String,
    /// Email of question's author
    email: Option<lettre::Address>,
}

/// Question to frontend
pub struct Full {
    pid: u64,
    info: String,
    time: chrono::NaiveDateTime,
    name: String,
    email: Option<lettre::Address>,
}
