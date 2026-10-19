//! What can go wrong, and the exit code each kind of failure gives.

use std::path::PathBuf;

/// Every failure `tc` reports. The message is shown as it is, so it says what
/// happened and what to do in plain words.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The command was used wrongly: a bad argument, an unknown session
    /// number, a command that needs a connection while none is set up.
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
    /// JSON from the service or the log on this machine did not have the
    /// expected shape.
    #[error("unexpected data: {0}")]
    Data(String),
    /// Money did not add up, which only happens with absurd amounts.
    #[error(transparent)]
    Money(#[from] tokencontroller_core::MoneyError),
    /// The event log on this machine cannot be folded.
    #[error("the log on this machine cannot be read: {0}")]
    Fold(#[from] tokencontroller_core::FoldError),
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
            | Self::Fold(_)
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

impl From<tokencontroller_core::TagError> for Error {
    fn from(error: tokencontroller_core::TagError) -> Self {
        Self::Usage(error.to_string())
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
        let tag: Error = tokencontroller_core::TagError::InvalidTag("x".into()).into();
        assert_eq!(tag.exit_code(), 1);
    }

    #[test]
    fn broken_json_is_a_failure_on_this_machine() {
        let broken = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let error: Error = broken.into();
        assert_eq!(error.exit_code(), 3);
        assert!(error.to_string().starts_with("unexpected data: "));
        let output: Error = std::io::Error::other("closed pipe").into();
        assert_eq!(output.exit_code(), 3);
        assert_eq!(
            output.to_string(),
            "could not write the output: closed pipe"
        );
    }

    #[test]
    fn messages_name_the_service_and_the_address() {
        let service = Error::Service {
            status: 422,
            message: "no such feature".into(),
        };
        assert_eq!(
            service.to_string(),
            "the service answered 422: no such feature"
        );
        let network = Error::Network {
            url: "https://tc.example.com".into(),
            reason: "timeout".into(),
        };
        assert_eq!(
            network.to_string(),
            "could not reach https://tc.example.com: timeout"
        );
        assert_eq!(
            Error::usage("say which session").to_string(),
            "say which session"
        );
    }
}
