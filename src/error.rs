#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid checksum")]
    CheckSumError,
    #[error("Invalid reply size")]
    ReplySizeError,
    #[error("Frame out of order")]
    FrameNoError,
}
