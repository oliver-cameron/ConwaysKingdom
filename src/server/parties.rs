//! **A group of people with a private set of worlds.**
//!
//! Not one room — a set — so a party is somewhere a group *lives* rather than
//! a game they are in. A room is a world and is over when it is over; a party
//! outlives every room in it, which is what makes it the first thing here
//! that is neither a world nor a fact about one person.
//!
//! ## A list of people, not a code
//!
//! A private room is reached by a code, which is a bearer credential: whoever
//! it is forwarded to gets in, and the room cannot tell. That is right for
//! reading six characters to somebody beside you and wrong for a group that
//! persists — a party somebody left should stop being a party they can
//! rejoin, and a code cannot express that. So a party is a set of
//! [`PersonId`]s, and its rooms open for those and nobody else; see
//! `Rooms::may_enter`.
//!
//! ## Keyed by today's person, on purpose
//!
//! planned.md said parties wait on identity being a keypair, because "invite
//! Alice" wants a durable name for Alice. It is built on the per-server
//! `PersonId` a secret is exchanged for, on the pattern `Rooms::challenges`
//! already uses, at the price the leaderboard already pays: when a person
//! becomes a key fingerprint every row here resets, or is claimed under
//! whatever migration ratings get. That is one line in a release note against
//! a feature people can use now, and the same reading [what to do next] gives
//! for the leaderboard.
//!
//! One JSON object per party per line, beside the other tables — see
//! [`crate::net::jsonl`].
//!
//! [what to do next]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#what-to-do-next

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::net::{jsonl, PartyId, PersonId, RoomId};

/// The format of a stored row.
const VERSION: u8 = 1;

/// How many parties a server will hold. A party costs a row and no
/// simulation, so this is a backstop against a client that makes one a second
/// rather than a budget.
pub const MAX_PARTIES: usize = 64;

/// One party as the table stores it.
#[derive(Serialize, Deserialize)]
struct Row {
    v: u8,
    id: PartyId,
    name: String,
    members: Vec<PersonId>,
    invited: Vec<PersonId>,
    rooms: Vec<RoomId>,
}

/// One party: who is in it, who has been asked, and which worlds are its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Party {
    pub name: String,
    pub members: BTreeSet<PersonId>,
    /// Asked and not yet answered. Standing rather than queued — the message
    /// is delivered once, and the right to take it stands until it is taken,
    /// which is the same split `Rooms::challenges` makes.
    pub invited: BTreeSet<PersonId>,
    pub rooms: BTreeSet<RoomId>,
}

/// Every party on this server.
#[derive(Default)]
pub struct Parties {
    known: BTreeMap<PartyId, Party>,
}

impl Parties {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    pub fn get(&self, id: &PartyId) -> Option<&Party> {
        self.known.get(id)
    }

    /// Make one, with `founder` as its first member.
    ///
    /// A name is the one thing here a client chooses, and it is clamped the
    /// way a player's is — see [`crate::net::player_name`] — because it goes
    /// on a row of a file and on a row of a screen.
    pub fn make(&mut self, name: &str, founder: &PersonId) -> Result<PartyId, String> {
        if self.known.len() >= MAX_PARTIES {
            return Err(format!(
                "this server is holding {MAX_PARTIES} parties, which is all it will"
            ));
        }
        let name = crate::net::player_name(name);
        if name.is_empty() {
            return Err("a party needs a name".into());
        }
        let id = (0..10)
            .map(|_| PartyId(format!("p-{}", &crate::server::new_token()[..8])))
            .find(|id| !self.known.contains_key(id))
            .ok_or_else(|| "could not find a free party id".to_string())?;
        let party =
            Party { name, members: BTreeSet::from([founder.clone()]), ..Default::default() };
        self.known.insert(id.clone(), party);
        Ok(id)
    }

    pub fn is_member(&self, id: &PartyId, who: &PersonId) -> bool {
        self.known.get(id).is_some_and(|p| p.members.contains(who))
    }

    /// The parties somebody is in, in id order.
    pub fn of<'a>(&'a self, who: &'a PersonId) -> impl Iterator<Item = (&'a PartyId, &'a Party)> {
        self.known.iter().filter(move |(_, p)| p.members.contains(who))
    }

    /// Ask somebody in. Only a member may, and asking somebody already in is
    /// refused rather than ignored so the asker learns it.
    pub fn invite(
        &mut self,
        id: &PartyId,
        from: &PersonId,
        who: &PersonId,
    ) -> Result<String, String> {
        let Some(party) = self.known.get_mut(id) else {
            return Err("there is no such party".into());
        };
        if !party.members.contains(from) {
            return Err("you are not in that party".into());
        }
        if party.members.contains(who) {
            return Err("they are already in it".into());
        }
        party.invited.insert(who.clone());
        Ok(party.name.clone())
    }

    /// Take a standing invitation. Nothing else gets anybody in.
    pub fn join(&mut self, id: &PartyId, who: &PersonId) -> Result<(), String> {
        let Some(party) = self.known.get_mut(id) else {
            return Err("there is no such party".into());
        };
        if party.members.contains(who) {
            return Ok(());
        }
        if !party.invited.remove(who) {
            return Err("nobody has asked you into that party".into());
        }
        party.members.insert(who.clone());
        Ok(())
    }

    /// Leave. `true` when that emptied the party, which is then gone: a party
    /// nobody is in is one nobody can see, and a row for it would stand for
    /// ever.
    pub fn leave(&mut self, id: &PartyId, who: &PersonId) -> Result<bool, String> {
        let Some(party) = self.known.get_mut(id) else {
            return Err("there is no such party".into());
        };
        if !party.members.remove(who) {
            return Err("you are not in that party".into());
        }
        let emptied = party.members.is_empty();
        if emptied {
            self.known.remove(id);
        }
        Ok(emptied)
    }

    /// Make a room one of this party's.
    pub fn attach(&mut self, id: &PartyId, room: &RoomId) {
        if let Some(party) = self.known.get_mut(id) {
            party.rooms.insert(room.clone());
        }
    }

    /// A room is gone, or is no longer anybody's.
    pub fn detach(&mut self, room: &RoomId) {
        for party in self.known.values_mut() {
            party.rooms.remove(room);
        }
    }

    /// Whose a room is, if it is a party's. A room is in at most one party.
    pub fn party_of(&self, room: &RoomId) -> Option<&PartyId> {
        self.known.iter().find(|(_, p)| p.rooms.contains(room)).map(|(id, _)| id)
    }

    /// Forget rooms that are not here, which after a restart is every match
    /// and every room since deleted.
    pub fn keep_rooms(&mut self, exists: impl Fn(&RoomId) -> bool) {
        for party in self.known.values_mut() {
            party.rooms.retain(|room| exists(room));
        }
    }

    pub fn to_lines(&self) -> String {
        // Already in id order, being a `BTreeMap`, so two saves of one table
        // are the same bytes.
        jsonl::write(self.known.iter().map(|(id, p)| Row {
            v: VERSION,
            id: id.clone(),
            name: p.name.clone(),
            members: p.members.iter().cloned().collect(),
            invited: p.invited.iter().cloned().collect(),
            rooms: p.rooms.iter().cloned().collect(),
        }))
    }

    /// Read a table back, skipping any row this build cannot make sense of.
    /// A party with nobody in it is not read: it could never be seen.
    pub fn from_lines(text: &str) -> Self {
        let known = jsonl::read::<Row>(text, "the parties file")
            .into_iter()
            .filter(|row| row.v == VERSION && !row.members.is_empty())
            .map(|row| {
                let party = Party {
                    name: crate::net::player_name(&row.name),
                    members: row.members.into_iter().collect(),
                    invited: row.invited.into_iter().collect(),
                    rooms: row.rooms.into_iter().collect(),
                };
                (row.id, party)
            })
            .collect();
        Self { known }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::from_lines(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_lines())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(n: &str) -> PersonId {
        PersonId(format!("{n}{}", "0".repeat(32 - n.len())))
    }

    /// **Nothing gets anybody in but an invitation**, and leaving is final:
    /// a party is a list of people because a code could not say that.
    #[test]
    fn only_an_invitation_admits_and_leaving_is_leaving() {
        let mut parties = Parties::new();
        let id = parties.make("friday", &who("a")).unwrap();
        assert!(parties.is_member(&id, &who("a")), "the founder is in it");

        let why = parties.join(&id, &who("b")).unwrap_err();
        assert!(why.contains("nobody has asked"), "{why}");
        assert!(parties.invite(&id, &who("b"), &who("c")).is_err(), "a stranger invited somebody");

        assert_eq!(parties.invite(&id, &who("a"), &who("b")).unwrap(), "friday");
        parties.join(&id, &who("b")).unwrap();
        assert!(parties.is_member(&id, &who("b")));
        assert!(parties.invite(&id, &who("a"), &who("b")).is_err(), "asked somebody already in");

        assert!(!parties.leave(&id, &who("b")).unwrap(), "two left, one gone, not empty");
        assert!(!parties.is_member(&id, &who("b")));
        assert!(parties.join(&id, &who("b")).is_err(), "the door shut behind them");
        assert_eq!(parties.of(&who("b")).count(), 0);
    }

    /// The last one out takes the party with them: a party nobody is in is a
    /// party nobody can see, and a row for it would stand for ever.
    #[test]
    fn the_last_member_out_takes_the_party_with_them() {
        let mut parties = Parties::new();
        let id = parties.make("friday", &who("a")).unwrap();
        assert!(parties.leave(&id, &who("a")).unwrap(), "emptied");
        assert!(parties.is_empty(), "an empty party stayed");
        assert!(parties.leave(&id, &who("a")).is_err());
    }

    /// A name is the one thing a client chooses here, and it is clamped the
    /// way a player's is. The cap is a backstop rather than a budget.
    #[test]
    fn a_name_is_clamped_and_a_server_holds_only_so_many() {
        let mut parties = Parties::new();
        assert!(parties.make("   ", &who("a")).is_err(), "a blank name made a party");
        let id = parties.make("  Friday\tnight\n ", &who("a")).unwrap();
        assert_eq!(parties.get(&id).unwrap().name, "Fridaynight");
        for n in 1..MAX_PARTIES {
            parties.make(&format!("p{n}"), &who("a")).unwrap();
        }
        let why = parties.make("one more", &who("a")).unwrap_err();
        assert!(why.contains(&MAX_PARTIES.to_string()), "{why}");
    }

    /// Written down and read back, the same bytes each time, with the standing
    /// invitations and the rooms — and a party with nobody in it left behind.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut parties = Parties::new();
        let id = parties.make("friday", &who("a")).unwrap();
        parties.invite(&id, &who("a"), &who("b")).unwrap();
        parties.attach(&id, &RoomId::from("r-den123"));
        parties.attach(&id, &RoomId::from("r-gone45"));
        let lines = parties.to_lines();
        assert_eq!(lines, parties.to_lines(), "two saves of one table differ");

        let mut back = Parties::from_lines(&lines);
        assert_eq!(back.get(&id), parties.get(&id), "the party was lost");
        back.join(&id, &who("b")).unwrap();
        back.keep_rooms(|room| room.as_str() == "r-den123");
        assert_eq!(back.party_of(&RoomId::from("r-den123")), Some(&id));
        assert_eq!(back.party_of(&RoomId::from("r-gone45")), None, "a gone room stayed");

        let empty = r#"{"v":1,"id":"p-empty","name":"x","members":[],"invited":[],"rooms":[]}"#;
        assert!(Parties::from_lines(empty).is_empty(), "a party nobody is in was read");
    }
}
