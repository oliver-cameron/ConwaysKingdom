mod app;
mod cell;
mod conwayHandler;
mod frame;
mod game;
mod gpu;
mod pipeline;
pub use app::{App, run};
pub use frame::{Draw, DrawCall, Frame, FrameAcquire, IndexBufferBinding};
pub use gpu::GpuState;
pub use pipeline::{PipelineDescriptor, create_pipeline};

// Swap this for your own `App` impl once you've moved past the demo.
pub use game::BattleApp;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).expect("could not init logger");
    wasm_bindgen_futures::spawn_local(run::<TriangleApp>());
}
