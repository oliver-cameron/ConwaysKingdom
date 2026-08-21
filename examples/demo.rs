use conwayskingdom::sim::{World, CHUNK_N};
fn main() {
    let mut w = World::infinite();
    println!("{:>5} {:>7} {:>5}  {:>6} {:>6}  contested", "gen", "chunks", "live", "p1", "p2");
    for g in 0..=400 {
        if g % 50 == 0 {
            let mut p1 = 0; let mut p2 = 0; let mut other = 0;
            for ((cr, cc), chunk) in w.stored() {
                let _ = (cr, cc);
                for r in 0..CHUNK_N { for c in 0..CHUNK_N {
                    let cell = chunk[(r, c)];
                    if !cell.is_alive() { continue; }
                    match cell.player().0 { 1 => p1 += 1, 2 => p2 += 1, _ => other += 1 }
                }}
            }
            println!("{g:>5} {:>7} {:>5}  {p1:>6} {p2:>6}  {other}",
                     w.stored_count(), w.live_cells().len());
        }
        w.step();
    }
}
