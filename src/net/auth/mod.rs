//! Who somebody is, on this server. Two halves, and [`person`] says which is
//! which and why.
//!
//! Here in [`crate::net`] rather than in `server` because both ends of the
//! wire have to agree what a person is, and it is the *client* that keeps the
//! half that matters.

pub mod person;

pub use person::{PersonId, Secret};
