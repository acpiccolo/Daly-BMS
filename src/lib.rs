mod error;
pub mod protocol;

pub use error::Error;

#[cfg(feature = "serialport")]
pub mod serialport;

#[cfg(feature = "tokio-serial-async")]
pub mod tokio_serial_async;
