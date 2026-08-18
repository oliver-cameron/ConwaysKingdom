use crate::app::App;
use crate::conwayHandler::{self, GameState};
use crate::frame::{Draw, DrawCall};
use crate::gpu::GpuState;
use crate::pipeline::{PipelineDescriptor, create_pipeline};

pub struct BattleApp {
    pipelines: PipelineList,
    ticker: f32,
    game_state: conwayHandler::GameState,
}
pub struct PipelineList {
    grid_pipeline: wgpu::RenderPipeline,
}
impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let pipeline = create_pipeline(
            gpu,
            &PipelineDescriptor {
                label: "triangle pipeline",
                shader_source: include_str!("shaders/triangle.wgsl"),
                ..Default::default()
            },
        );
        Self {
            pipelines: PipelineList {
                grid_pipeline: pipeline,
            },
            ticker: 0.0,
            game_state: GameState { chunks: vec![] },
        }
    }

    fn draw_calls(&self) -> Vec<DrawCall<'_>> {
        vec![DrawCall {
            pipeline: &self.pipelines.grid_pipeline,
            bind_groups: &[],
            vertex_buffers: &[],
            index_buffer: None,
            draw: Draw::Vertices {
                vertices: 0..3,
                instances: 0..1,
            },
        }]
    }
    fn update(&mut self, _gpu: &GpuState, _dt: f32) {
        self.ticker += _dt;
    }
}
