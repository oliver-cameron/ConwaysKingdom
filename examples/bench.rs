use std::time::Instant;
use conwayskingdom::World;

fn main() {
    let mut w = World::infinite();
    for _ in 0..400 { w.step(); }
    println!("after 400 gens: {} slots, {} loaded", w.stored_count(), w.active_count());

    let t = Instant::now();
    for _ in 0..1000 { w.step(); }
    let step = t.elapsed().as_secs_f64() / 1000.0;
    println!("step():        {:>9.1} us  ({} loaded)", step * 1e6, w.active_count());

    let t = Instant::now();
    for _ in 0..1000 { std::hint::black_box(w.live_cells()); }
    println!("live_cells():  {:>9.1} us", t.elapsed().as_secs_f64() / 1000.0 * 1e6);

    // What a frame's CPU-side work costs at 4 generations/sec.
    println!("\nper generation at {} loaded chunks: {:.3} ms", w.active_count(), (step) * 1e3);
    println!("budget at 60fps: 16.7 ms");
}
