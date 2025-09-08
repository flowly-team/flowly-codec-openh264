use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error<E = flowly::Void> {
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("OpenH264Error: {0}")]
    OpenH264Error(#[from] openh264::Error),

    #[error(transparent)]
    Other(E),
}

impl Error {
    pub fn extend<E>(self) -> Error<E> {
        match self {
            Self::IoError(e) => Error::IoError(e),
            Self::OpenH264Error(e) => Error::OpenH264Error(e),
            Self::Other(_) => unreachable!(),
        }
    }
}
