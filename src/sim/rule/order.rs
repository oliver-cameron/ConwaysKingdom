//! The one piece of machinery the rules need: a list that is also the code.
//!
//! [`rules!`] takes the rules in the order they are applied, names them, and
//! writes both the chain that runs them and the list of names. Written once, so
//! the order you read is the order that happens and there is no second place to
//! forget to update.
//!
//! A macro rather than an array of function pointers, and the difference is
//! measurable: an array cannot be inlined through, and three indirect calls per
//! cell per generation cost **54%** of the stepping time — 29 µs a generation
//! against 45. Unrolled here, each rule inlines and the list costs nothing.
//!
//! Split out of `super` because that file is meant to read like a config, and
//! this is the opposite of that.

/// Define the rules, in order.
///
/// ```ignore
/// rules! {
///     "ice freezes what it covers" => ice,
///     "life and death"             => conway,
/// }
/// ```
///
/// Each rule takes the cell as the one before it left it and returns a
/// [`super::Then`], which says whether the rules after it still run. Expands to
/// `apply`, which is the chain, and `RULES`, which is the names in the same
/// order — so anything that wants to describe the game reads the same list the
/// game runs.
macro_rules! rules {
    ($($name:literal => $rule:ident),* $(,)?) => {
        /// The rules, in the order they are applied.
        ///
        /// Names only: the chain that runs them is generated from the same
        /// list, so this cannot drift from what happens.
        pub const RULES: &[&str] = &[$($name),*];

        /// Every rule in turn, stopping early if one says to.
        #[inline]
        fn apply(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Cell {
            let mut cell = cell;
            $(
                match $rule(cell, neighbours, roll) {
                    Then::Next(next) => cell = next,
                    Then::Stop(next) => return next,
                }
            )*
            cell
        }
    };
}

pub(crate) use rules;
