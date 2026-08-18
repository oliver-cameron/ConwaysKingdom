//! The eight directions a cell or chunk has neighbours in.

/// The eight neighbours of a chunk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
    Nw,
}

impl Dir {
    pub const ALL: [Dir; 8] = [
        Dir::N,
        Dir::Ne,
        Dir::E,
        Dir::Se,
        Dir::S,
        Dir::Sw,
        Dir::W,
        Dir::Nw,
    ];

    #[inline]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (-1, 0),
            Dir::Ne => (-1, 1),
            Dir::E => (0, 1),
            Dir::Se => (1, 1),
            Dir::S => (1, 0),
            Dir::Sw => (1, -1),
            Dir::W => (0, -1),
            Dir::Nw => (-1, -1),
        }
    }
}

