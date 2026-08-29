//! Who somebody is, and how they prove it.
//!
//! What is here is [`person`]: a keypair the client generates and never
//! sends, whose public half is the name everything else is keyed by. A
//! server-minted bearer secret came first and was replaced, because a bearer
//! secret cannot be both cross-server and safe — see the module for the two
//! ways it fails once the same key is meant to mean the same person in more
//! than one place.
//!
//! Deliberately in [`crate::net`] and not in `server`: both ends of the wire
//! have to agree on what a person is, and the client is the one that keeps the
//! proof. The **table** of who exists is the server's, in
//! [`crate::server::people`], and nothing in here knows it exists.
//!
//! [many servers]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#many-servers-and-what-must-not-be-decentralised

pub mod openssh;
pub mod person;

pub use person::{Claim, Key, PersonId};
