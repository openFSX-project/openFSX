//! Deterministic aircraft dynamics and contact runtime; no window or renderer.
use fsx_vehicle::aircraft;
#[cfg(test)]
use fsx_vehicle::mass;
pub mod air_data;
pub mod clock;
pub mod electrical;
pub mod electrical_distribution;
pub mod electrical_sources;
pub mod flight;
pub mod ground;
pub use flight::AircraftDynamics;
