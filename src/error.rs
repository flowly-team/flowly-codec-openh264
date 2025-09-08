use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("OpenH264Error: {0}")]
    OpenH264Error(#[from] openh264::Error),

    #[error("OpenH264 Decoder Failed (worker dead, cannot send)")]
    TrySendError,
}
