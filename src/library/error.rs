//! Errors from the library, settings, or save files.

/// Something the library or its files could not do.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Database(#[from] diesel::result::Error),

    #[error("database connection error: {0}")]
    Connection(#[from] diesel::result::ConnectionError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("image error: {0}")]
    Image(String),
}
