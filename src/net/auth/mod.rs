//! Who somebody is, on this server.
//!
//! Two halves in [`person`]: a [`Secret`] the client keeps and never shows,
//! and a [`PersonId`] the **server issues** on first sight and everybody sees.
//! The pairing is the server's to remember — see [`crate::server::people`] —
//! so an id says nothing about the secret behind it.
//!
//! Deliberately in [`crate::net`] rather than in `server`: both ends of the
//! wire have to agree on what a person is, and it is the client that keeps the
//! half that matters.
//!
//! An ed25519 keypair came first, where the client made both halves and a join
//! was a signature over a nonce. It bought one thing — a server could not be
//! you on a *different* server — which is worth nothing while there is one,
//! and cost a signature scheme, an OpenSSH parser, a round trip before every
//! join, and a dependency. The module note says what has to be revisited
//! before a second server exists.

pub mod person;

pub use person::{PersonId, Secret};
