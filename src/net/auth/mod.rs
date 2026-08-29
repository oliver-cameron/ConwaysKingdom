//! Who somebody is, and how they prove it.
//!
//! A folder rather than a file because this is the beginning of something
//! rather than the whole of it. What is here is [`person`]: a server-minted id
//! and the bearer proof that says the id is yours, which is enough to key a
//! rating by and enough to carry between browsers. What is not here yet is a
//! keypair, which is what being the same person on *several* servers would
//! need — see [many servers] — and which would sit beside `person` rather than
//! replacing it, since a server that mints keys goes on minting them for
//! clients that have no other way to be somebody.
//!
//! Deliberately in [`crate::net`] and not in `server`: both ends of the wire
//! have to agree on what a person is, and the client is the one that keeps the
//! proof. The **table** of who exists is the server's, in
//! [`crate::server::people`], and nothing in here knows it exists.
//!
//! [many servers]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#many-servers-and-what-must-not-be-decentralised

pub mod person;

pub use person::{Person, PersonId};
