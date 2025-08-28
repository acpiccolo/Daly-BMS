#![cfg_attr(docsrs, feature(doc_cfg))]
//! # dalybms_lib
//!
//! This crate provides a library for interacting with Daly BMS (Battery Management System) devices.
//! It offers both synchronous and asynchronous clients for communication.
//!
//! ## Features
//!
//! This crate uses a feature-based system to keep dependencies minimal.
//! You need to enable the client you want to use.
//!
//! - `default`: Enables `bin-dependencies`, which is intended for compiling the `dalybms` command-line tool and pulls in `serialport` and `serde`.
//!
//! ### Client Features
//! - `serialport`: Enables the **synchronous** client using the `serialport` crate.
//! - `tokio-serial-async`: Enables the **asynchronous** client using `tokio` and `tokio-serial`.
//!
//! ### Utility Features
//! - `serde`: Enables `serde` support for serializing/deserializing data structures.
//! - `bin-dependencies`: Enables all features required by the `dalybms` binary executable (currently `serialport` and `serde`).

/// Contains error types for the library.
mod error;
/// Defines the communication protocol for Daly BMS.
pub mod protocol;

pub use error::Error;

/// Synchronous client for Daly BMS communication.
#[cfg_attr(docsrs, doc(cfg(feature = "serialport")))]
#[cfg(feature = "serialport")]
pub mod serialport;

/// Asynchronous client for Daly BMS communication.
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-serial-async")))]
#[cfg(feature = "tokio-serial-async")]
pub mod tokio_serial_async;
