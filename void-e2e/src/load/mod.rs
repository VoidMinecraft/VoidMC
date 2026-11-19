//! Functional and load runs with full-protocol clients.
//!
//! A run first walks one client through the smoke checks a player relies
//! on, then starts a [`Workload`]'s load clients, the same [`Bot`]s the
//! scenarios use, and records connection success, round-trip latency, tick
//! rate, errors and, for an in-process server, resource use. The [`Report`]
//! compares each measurement with the workload's thresholds and decides
//! PASS or FAIL.
//!
//! Results apply only to the workload, thresholds and host recorded in the
//! report; they are not a capacity claim.
//!
//! [`Bot`]: crate::Bot

pub mod cli;
mod decision;
mod measure;
mod report;
mod runner;
mod system;
mod workload;

pub use decision::{Decision, Verdict, VerdictOutcome, decide};
pub use measure::{
    CheckOutcome, Connections, Distribution, Errors, Latency, Measurements, Resources, SmokeCheck,
    TickRate,
};
pub use report::{Conditions, REPORT_FORMAT, Report, SCOPE, ServerSettings};
pub use runner::{Target, run, run_endpoint, run_in_process};
pub use workload::{Thresholds, Workload};
