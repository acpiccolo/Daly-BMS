/// Represents the possible errors that can occur in the `dalybms_lib`.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    /// Error indicating an invalid checksum in the received data.
    #[error("Invalid checksum")]
    CheckSumError,
    /// Error indicating an invalid size of the reply received from the BMS.
    #[error("Invalid reply size")]
    ReplySizeError,
    /// Error indicating that a frame was received out of order.
    #[error("Frame out of order")]
    FrameNoError,
}
