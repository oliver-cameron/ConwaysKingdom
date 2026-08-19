//! The client: what a player sees and does.
//!
//! Split from `render`, which is generic wgpu and winit, and from `sim`, which
//! is the rules. This is where policy lives — tick rate, which world to open,
//! where the camera points, what a click means.

pub mod views;

pub use views::battle::{set_connection, BattleApp, GENERATION_SPAN};
