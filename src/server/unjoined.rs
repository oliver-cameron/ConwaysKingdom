//! What a connection that has joined nothing is held to.
//!
//! Two limits on the connection rather than the player, because there is no
//! player yet: a **deadline** for sitting silent in no room, and a **cap** on
//! how many one address may hold open at once. The numbers are [`ws`]'s, beside
//! its frame cap; the decisions are here, where `cargo test` reaches them.
//! See [known-bugs.md].
//!
//! [`ws`]: crate::server::ws
//! [known-bugs.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/known-bugs.md#fixed

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// When a connection last heard from at `last_heard` is closed for saying
/// nothing: `after` that, and never while it is in a room — a seated or
/// watching connection is written to every generation, and a dead one is
/// found by the write.
pub fn deadline(last_heard: Instant, in_room: bool, after: Duration) -> Option<Instant> {
    (!in_room).then(|| last_heard + after)
}

/// Who a connection is from. `CF-Connecting-IP` is the tunnel saying, and is
/// believed because nothing but the tunnel can reach the socket — see
/// [server.md]; without it, or with one that is not an address, the socket's
/// own.
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#deploying
pub fn remote(cf_connecting_ip: Option<&str>, socket: IpAddr) -> IpAddr {
    cf_connecting_ip.and_then(|ip| ip.trim().parse().ok()).unwrap_or(socket)
}

/// How many unjoined connections each address holds.
#[derive(Debug, Default)]
pub struct PerAddress(HashMap<IpAddr, usize>);

impl PerAddress {
    /// One more from `addr`, unless it already holds `cap`.
    pub fn admit(&mut self, addr: IpAddr, cap: usize) -> bool {
        let held = self.0.get(&addr).copied().unwrap_or(0);
        if held >= cap {
            return false;
        }
        self.0.insert(addr, held + 1);
        true
    }

    /// One fewer from `addr`: it joined, or it closed. An address holding none
    /// is forgotten, so the table is the addresses waiting now and not every
    /// address ever seen.
    pub fn release(&mut self, addr: IpAddr) {
        if let Some(held) = self.0.get_mut(&addr) {
            *held -= 1;
            if *held == 0 {
                self.0.remove(&addr);
            }
        }
    }

    /// How many addresses hold at least one.
    pub fn addresses(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    /// A connection in no room is due `after` the last thing it said, and one
    /// in a room is never due at all.
    #[test]
    fn a_silent_connection_is_due_unless_it_is_in_a_room() {
        let heard = Instant::now();
        let after = Duration::from_secs(120);
        assert_eq!(deadline(heard, false, after), Some(heard + after));
        assert_eq!(deadline(heard, true, after), None);
    }

    /// Hearing from it again moves the deadline out by the same amount.
    #[test]
    fn a_word_from_it_pushes_the_deadline_out() {
        let heard = Instant::now();
        let after = Duration::from_secs(120);
        let later = heard + Duration::from_secs(30);
        assert_eq!(deadline(later, false, after), Some(later + after));
    }

    /// The tunnel's header names the client; without one, or with one that is
    /// not an address, the socket's own address stands.
    #[test]
    fn the_tunnel_names_the_client_and_garbage_falls_back_to_the_socket() {
        let socket = IpAddr::from([127, 0, 0, 1]);
        assert_eq!(remote(Some("198.51.100.7"), socket), IpAddr::from([198, 51, 100, 7]));
        assert_eq!(remote(Some(" 2001:db8::1 "), socket), "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(remote(None, socket), socket);
        assert_eq!(remote(Some("not an address"), socket), socket);
    }

    /// An address may hold the cap and no more, and another address is not
    /// counted against it.
    #[test]
    fn the_cap_is_per_address() {
        let mut table = PerAddress::default();
        assert!(table.admit(ip(1), 2));
        assert!(table.admit(ip(1), 2));
        assert!(!table.admit(ip(1), 2));
        assert!(table.admit(ip(2), 2));
        assert_eq!(table.addresses(), 2);
    }

    /// A release makes room for one more, and an address holding nothing is
    /// forgotten rather than kept at nought forever.
    #[test]
    fn a_release_makes_room_and_forgets_an_empty_address() {
        let mut table = PerAddress::default();
        assert!(table.admit(ip(1), 1));
        assert!(!table.admit(ip(1), 1));
        table.release(ip(1));
        assert_eq!(table.addresses(), 0);
        assert!(table.admit(ip(1), 1));
    }

    /// Releasing an address that holds nothing is harmless.
    #[test]
    fn releasing_a_stranger_changes_nothing() {
        let mut table = PerAddress::default();
        table.release(ip(9));
        assert_eq!(table.addresses(), 0);
        assert!(table.admit(ip(9), 1));
    }
}
