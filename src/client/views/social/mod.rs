//! The screens about **people** rather than about a world.
//!
//! Who you are, who else plays here, what a server will say about any of them,
//! and the picture that tells two players with one name apart. Grouped by
//! subject rather than by where they are drawn, which is why they came from
//! two different folders: [`account`] and [`people`] were pages on the menu and
//! [`profile`] is a panel over a running board, and all three answer the same
//! kind of question.
//!
//! ## What holds them together
//!
//! **A person outlives every world on a server.** A `PlayerId` is a seat and a
//! room is one world, so everything here is filed against a
//! [`crate::net::PersonId`] instead — see [`crate::net::auth`] for what that
//! is and [`crate::server::profiles`] for what a server will vouch for about
//! one.
//!
//! And **anything another player is shown has to be the server's**, because
//! client state is self-asserted: a rating you keep is a rating you can type.
//! The exception is what a server merely holds — a pattern library and a diary
//! — which nobody else is shown at all, so there is nothing to be misled by.
//! [`crate::net::kept`] draws that line.
//!
//! ## Asking somebody for a game
//!
//! [`challenge`] is the panel, and it is a panel for the reason [`profile`] is
//! one: being asked a question should not take away whatever you were in the
//! middle of. It is opened *from* a profile, which is where somebody is when
//! they have decided they want to play a person — a lobby row, a standings
//! bar and a list of who plays here all lead there by way of wondering about
//! somebody.
//!
//! ## Why a face is here
//!
//! [`face`] is a drawing primitive rather than a screen, and it sits here
//! because what it draws is a *person*: a soup seeded from their fingerprint
//! and stepped with the game's own rule, so nobody chooses their picture and
//! nobody has to moderate one. It is used by all three screens above and by
//! the lobby, which is the one board-side thing that lists people.

pub mod account;
pub mod challenge;
pub mod face;
pub mod invite;
pub mod people;
pub mod profile;
