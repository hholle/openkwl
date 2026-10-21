//! Hardware-independent control logic for the Helios KWL EC 300 W R.
//!
//! The stock-compatible modules (`bypass`, `cwp`, `efficiency`, `fan`, `frost`, `ntc`, `tacho`,
//! `average`) reimplement the stock firmware's behaviour (same thresholds and states).
//! `clock`, `schedule`, `holiday`, `mode`, `heat`, `filter` and `sntp` are new logic for features
//! whose stock behaviour is not known in detail; their docs say where they follow observed stock
//! parameters.
//!
//! The stock-compatible modules are independent implementations written from the observed
//! behaviour of the stock easyControls 2.24 firmware (inputs, outputs, thresholds, timing and
//! settings files), for interoperability with the unit and its stock settings.
//!
//! The crate is `no_std`, allocation-free and fully testable on the host.

#![no_std]

pub mod airflow;
pub mod average;
pub mod bypass;
pub mod clock;
pub mod cwp;
pub mod efficiency;
pub mod fan;
pub mod filter;
pub mod frost;
pub mod heat;
pub mod holiday;
pub mod mdns;
pub mod mode;
pub mod ntc;
pub mod num;
pub mod schedule;
pub mod sntp;
pub mod tacho;
