use crate::cell;
use std::cell::Cell;
use std::rc::Rc;
pub struct GameState {
    pub chunks: Vec<Rc<Cell<cell::Neighbor>>>,
}
