//! What can go wrong, and the exit code each kind of failure gives.

use std::path::PathBuf;

/// Every failure `tc` reports. The message is shown as it is, so it says what
/// happened and what to do in plain words.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The command was used wrongly: a bad argument, an unknown entry number,
    /// a command that needs a connection while none is set up.
    #[error("{0}")]
    Usage(String),
    /// The service answered with an error.
    #[error("the service answered {status}: {message}")]
    Service { status: u16, message: String },
    /// The service could not be reached at all.
    #[error("could not reach {url}: {reason}")]
    Network { url: String, reason: String },
    /// A file on this machine could not be read or written.
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The local database failed.
    #[error("the local database failed: {0}")]
    Store(#[from] rusqlite::Error),
    /// JSON from the service or the classifier did not have the expected shape.
    #[error("unexpected data: {0}")]
    Data(String),
    /// Money did not add up, which only happens with absurd amounts.
    #[error(transparent)]
    Money(#[from] tokencontroller_core::MoneyError),
    /// Writing the output failed, such as a closed pipe.
    #[error("could not write the output: {0}")]
    Output(#[from] std::io::Error),
}

impl Error {
    /// The process exit code: 1 for a usage error, 2 when the service is at
    /// fault or out of reach, 3 for a failure on this machine.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 1,
            Self::Service { .. } | Self::Network { .. } => 2,
            Self::File { .. }
            | Self::Store(_)
            | Self::Data(_)
            | Self::Money(_)
            | Self::Output(_) => 3,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub fn file(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::File {
            path: path.into(),
            source,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Data(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_tell_the_caller_who_is_at_fault() {
        assert_eq!(Error::usage("no").exit_code(), 1);
        let service = Error::Service {
            status: 500,
            message: "down".into(),
        };
        assert_eq!(service.exit_code(), 2);
        let network = Error::Network {
            url: "https://x".into(),
            reason: "timeout".into(),
        };
        assert_eq!(network.exit_code(), 2);
        assert_eq!(Error::Data("x".into()).exit_code(), 3);
        let file = Error::file("/x", std::io::Error::other("gone"));
        assert_eq!(file.exit_code(), 3);
        assert_eq!(file.to_string(), "/x: gone");
    }
}
