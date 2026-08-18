fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    pollster::block_on(conwayskingdom::run::<conwayskingdom::BattleApp>());
}
