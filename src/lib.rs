//! # dalybms_lib
//!
//! This crate provides a library for interacting with Daly BMS (Battery Management System) devices.
//! It offers both synchronous and asynchronous clients for communication.
//!
//! ## Features
//!
//! - `serialport`: Enables the synchronous client using the `serialport` crate.
//! - `tokio-serial-async`: Enables the asynchronous client using the `tokio-serial` crate.
//!
//! By default, both features are enabled.

/// Contains error types for the library.
mod error;
/// Defines the communication protocol for Daly BMS.
pub mod protocol;

pub use error::Error;

/// Synchronous client for Daly BMS communication.
#[cfg(feature = "serialport")]
pub mod serialport;

/// Asynchronous client for Daly BMS communication.
#[cfg(feature = "tokio-serial-async")]
pub mod tokio_serial_async;
