//! The client: what a player sees and does.
//!
//! Split from `render`, which is generic wgpu and winit, and from `sim`, which
//! is the rules. This is where policy lives — tick rate, which world to open,
//! where the camera points, what a click means.

pub mod desync;
pub mod record;
pub mod route;
pub mod session;
pub mod views;

pub use views::game::GameApp;

/// Native only: the browser client learns its server from the page it came
/// from, so there is nothing to tell it.
#[cfg(not(target_arch = "wasm32"))]
pub use views::game::{set_connection, set_world, Connection};
