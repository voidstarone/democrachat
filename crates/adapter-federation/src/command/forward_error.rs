/// Why a forwarded write could not be completed.
#[derive(Debug)]
pub enum ForwardError {
    /// The target scope has no current owner — nobody can authorize the write.
    Unowned,
    /// The owner could not be reached (fail-closed; the write was NOT applied).
    OwnerUnreachable(String),
    /// The owner ran the use-case and refused it (a domain error, e.g. not a citizen,
    /// or the command's authenticity/freshness/replay check failed).
    Rejected(String),
}

impl std::fmt::Display for ForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForwardError::Unowned => write!(f, "scope has no reachable owner"),
            ForwardError::OwnerUnreachable(e) => write!(f, "owner unreachable: {e}"),
            ForwardError::Rejected(e) => write!(f, "owner rejected the write: {e}"),
        }
    }
}

impl std::error::Error for ForwardError {}
