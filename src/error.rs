use std::fmt;

#[derive(Debug)]
pub enum Error {
    CheckSumError,
    ReplySizeError,
    FrameNoError,
    Io(std::io::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            // Both underlying errors already impl `Display`, so we defer to
            // their implementations.
            Error::Io(ref err) => write!(f, "IO error: {}", err),
            Error::CheckSumError => write!(f, "Invalid checksum"),
            Error::ReplySizeError => write!(f, "Invalid reply size"),
            Error::FrameNoError => write!(f, "Frame out of order"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}
