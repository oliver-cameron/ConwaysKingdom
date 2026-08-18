use conwayskingdom::sim::World;
fn main() {
    let mut w = World::demo();
    println!("{:>5} {:>7} {:>5}  {}", "gen", "chunks", "live", "digest");
    for g in 0..=300 {
        if g % 30 == 0 {
            println!("{g:>5} {:>7} {:>5}  {:x}", w.stored_count(), w.live_cells().len(), w.digest());
        }
        w.step();
    }
}
