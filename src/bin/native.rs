fn main() {
    pollster::block_on(conwayskingdom::run::<conwayskingdom::BattleApp>());
}
