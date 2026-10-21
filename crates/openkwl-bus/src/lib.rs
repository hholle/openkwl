//! The Helios module bus: RS-485 at 57600 8N1 with the controller as Modbus RTU master
//! (as in the stock firmware). [`modbus`] builds and checks the frames; the module
//! protocol on top of it follows the stock firmware byte for byte.

#![no_std]

pub mod knx;
pub mod modbus;
