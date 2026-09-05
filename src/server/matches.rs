//! What a match *does*, and the sides that play it. The types it does it to
//! are in [`crate::net`], because a client has to be told what a match is
//! doing and the wire is where the two sides agree on a vocabulary.
//!
//! A team is not a second thing here. [`crate::sim::Player`] is the one player
//! type and a side is one of them, so making sides, joining one, checking they
//! are even, blowing the whistle, giving up and deciding a winner are all the
//! same handful of rows — which is why they read as one file rather than two.
//!
//! **Nothing here is in `sim`.** The simulation does not know what a match is,
//! the same way it does not know what money is: a match is an arrangement of
//! when a room steps and who may join it, and both of those are the server's
//! business. What that buys is that a match cannot introduce a rule the world
//! has to honour, and so cannot make a match world behave differently from the
//! one people practise in.

use super::Server;
pub use crate::net::Victory;
use crate::sim::{Player, PlayerId};

pub use crate::net::MatchPhase as Phase;

impl Victory {
    /// Read `timer 2000` or `territory 500`.
    pub fn parse(kind: &str, value: &str) -> Result<Self, String> {
        let n: u64 = value.parse().map_err(|_| format!("\"{value}\" is not a number of {kind}"))?;
        if n == 0 {
            return Err(format!("a {kind} of zero is a match that is over already"));
        }
        match kind {
            "timer" | "time" | "ticks" => Ok(Self::Timer { generations: n }),
            "territory" | "ground" | "land" => Ok(Self::Territory { squares: n as usize }),
            other => Err(format!("no win condition \"{other}\"; try timer or territory")),
        }
    }
}

/// Who is holding the most, and how much. `None` when nobody holds anything.
///
/// **A side counts as one because a side is one**: everybody on it places
/// cells carrying its number, so the count under that number is already the
/// side's total. There used to be a `leader_of` beside this that took the
/// roster's allegiances and summed each side by hand; the two answers are the
/// same answer now.
///
/// Ties go to the **lower player number**, which is arbitrary and has to be
/// something: two players on exactly the same count is a real possibility on a
/// small world, and a winner picked by iteration order would differ between
/// runs of the same match.
pub fn leader(held: &[usize; PlayerId::COUNT]) -> (Option<PlayerId>, usize) {
    let mut best = (None, 0);
    for (id, &count) in held.iter().enumerate().skip(1) {
        if count > best.1 {
            best = (Some(PlayerId(id as u8)), count);
        }
    }
    best
}

impl Server {
    /// The sides, as a lobby needs them: each with its name and who sits on
    /// it. Empty in a free-for-all.
    ///
    /// A side's name is the name on its own row, because a side is a player
    /// and a player has a name. There used to be a `team_names` vector beside
    /// the sides, indexed from one, with a `saturating_sub` to get back to it.
    pub fn teams(&self) -> Vec<crate::net::Team> {
        self.sides
            .iter()
            .map(|&id| crate::net::Team {
                id,
                name: self.players.get(&id).map(|p| p.name.clone()).unwrap_or_default(),
                players: self
                    .players
                    .values()
                    .filter(|p| p.online && p.plays_as == id)
                    .map(|p| p.id)
                    .collect(),
            })
            .collect()
    }

    /// What number a seat's cells carry: its side's, or its own.
    ///
    /// The one lookup that replaces `Sides`. Everything downstream — placing,
    /// pricing, spawning, scoring — takes the answer and never asks again who
    /// is allied with whom, because by then there is nobody to be allied with.
    pub fn plays_as(&self, seat: PlayerId) -> PlayerId {
        self.players.get(&seat).map_or(seat, |p| p.plays_as)
    }

    /// Put this player on a side, or take them off one.
    ///
    /// Only while gathering. Changing sides mid-match would hand your ground
    /// to the people you were fighting, which is not something a lobby should
    /// let anybody do by accident.
    ///
    /// `team` is the side's own number, or the seat's own to step off one.
    pub fn join_team(&mut self, seat: PlayerId, team: PlayerId) -> Result<(), String> {
        if self.sides.is_empty() {
            return Err("this match has no teams".into());
        }
        // A world has no whistle, so its teams are never settled: people join
        // and leave one as they like. A match's are fixed at the start, or
        // changing sides would hand your ground to the people you were
        // fighting.
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        // Their own number steps off a side; anything else has to be a side
        // this match actually has, or a client could put its cells under
        // somebody else's number by asking.
        if team != seat && !self.sides.contains(&team) {
            return Err(format!("this match has {} teams", self.sides.len()));
        }
        let Some(player) = self.players.get_mut(&seat) else {
            return Err("you are not in this match".into());
        };
        player.plays_as = team;
        self.lobby_changed = true;
        Ok(())
    }

    /// **Which side somebody arriving should be put on**, or `None` where the
    /// question does not arise — a room with no sides, or a match already
    /// running, where sides are settled.
    ///
    /// **The lobby's decision and not the form's.** A match is described
    /// before anybody is in it, so the person making one cannot say who goes
    /// where; the lobby is where that is known, and it is where somebody can
    /// disagree with the answer by pressing a different side.
    ///
    /// Empty sides first, in the order they were made, because a side nobody
    /// is on is the one thing that stops a whistle — see [`Self::teams_are_fair`].
    /// After that the smallest, which keeps a room that fills up roughly even
    /// without ever refusing the uneven match people meant to arrange.
    pub(super) fn side_for_somebody_new(&self) -> Option<PlayerId> {
        if self.sides.is_empty() || !self.phase.open_to_newcomers() {
            return None;
        }
        // **People on it, and a side is not a person.** A side is a `Player`
        // row like anybody else and `Player::new` sets `plays_as` to its own
        // id, so counting naively has every side counting itself: none is ever
        // empty, and "empty first" is a clause that never fires.
        let on = |side: PlayerId| {
            self.players
                .values()
                .filter(|p| !self.sides.contains(&p.id) && p.plays_as == side)
                .count()
        };
        self.sides.iter().copied().min_by_key(|&side| (on(side) > 0, on(side), side.0))
    }

    /// Call a side something.
    pub fn name_team(&mut self, team: PlayerId, name: &str) -> Result<(), String> {
        if !self.sides.contains(&team) {
            return Err("no such team".into());
        }
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        let name = crate::net::team_name(name)?;
        let ordinal = self.sides.iter().position(|&s| s == team).unwrap_or(0) as u8 + 1;
        let row = self.players.get_mut(&team).expect("a side has a row");
        row.name = if name.is_empty() { crate::net::default_team_name(ordinal) } else { name };
        self.lobby_changed = true;
        Ok(())
    }

    /// Whether the sides are even enough to start, or what is wrong with them.
    ///
    /// Two things, and both are about a match nobody would want to play rather
    /// than about fairness in the abstract. **Everybody has to be on a side**,
    /// because a player left on nobody's is a free agent in a team game and
    /// the scoring has nowhere to put them. And **no side may be empty**, since
    /// a three-way match with one side unoccupied is a two-way match that
    /// scores as if it were not.
    ///
    /// Sizes are *not* checked beyond that. Three against two is a match
    /// people may well have arranged on purpose, and a server that refuses it
    /// is a server they work around by leaving somebody out.
    fn teams_are_fair(&self) -> Result<(), String> {
        let here: Vec<&Player> = self.players.values().filter(|p| p.online).collect();
        if let Some(stray) = here.iter().find(|p| p.plays_as == p.id) {
            return Err(format!("{} has not picked a team", stray.name));
        }
        if let Some(&empty) = self.sides.iter().find(|&&t| !here.iter().any(|p| p.plays_as == t)) {
            let name = self.players.get(&empty).map(|p| p.name.as_str()).unwrap_or("that side");
            return Err(format!("nobody is on {name}"));
        }
        Ok(())
    }

    /// Give this match sides. Only before it starts, and only on a match.
    /// **A side is a player, so making one takes a number.** They come from
    /// the same pool the seats do and are handed out here, before anybody
    /// joins, so a side and a seat can never be the same number — which is
    /// exactly the collision that used to seat an unaligned player 3 on top of
    /// team 3. What it costs is seats: a world holds [`PlayerId::MAX`]
    /// numbers, and a match with `n` sides has `n` fewer people in it.
    pub fn make_teams(&mut self, n: u8) -> Result<(), String> {
        // A world may have them too. What a team is, is people building as one
        // player — one purse, one patch of ground, one colour — and none of
        // that needs a result to be about. What a *match* adds is that the
        // teams have to be even before the whistle.
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        if !(crate::net::MIN_TEAMS..=crate::net::MAX_TEAMS).contains(&n) {
            return Err(format!(
                "a match has between {} and {} teams",
                crate::net::MIN_TEAMS,
                crate::net::MAX_TEAMS
            ));
        }
        for ordinal in 1..=n {
            let Some(id) = self.next_player_id() else {
                return Err("this world has no numbers left for another side".into());
            };
            let mut row = Player::new(id, crate::net::default_team_name(ordinal));
            // A side is not sitting anywhere and has nothing to come back
            // with: it is a number that holds ground and a purse, and the
            // people at its controls have seats of their own.
            row.online = false;
            row.value = self.starting_value();
            self.players.insert(id, row);
            self.sides.push(id);
        }
        Ok(())
    }

    /// How many sides this match has. Nought is a free-for-all.
    pub fn team_count(&self) -> u8 {
        self.sides.len() as u8
    }

    /// Make this room a match, gathering and not yet stepping.
    pub fn make_match(&mut self, victory: Victory) {
        self.phase = Phase::Gathering;
        self.victory = Some(victory);
        self.lobby_changed = true;
    }

    /// Start the clock. The tick it starts at is what the deadline is measured
    /// from, so a match that gathered for an hour still runs its full length.
    ///
    /// Blow the whistle. `by` is the player who pressed it, or `None` for the
    /// console, which is the operator rather than anybody in the room.
    pub fn start_match(&mut self, by: Option<PlayerId>) -> Result<(), String> {
        match self.phase {
            Phase::Gathering => {
                // **The balance check is here and not in the lobby.** A lobby
                // that refuses to let you join your friend because the sides
                // would be uneven is a lobby that makes people argue about the
                // order they clicked in; one that refuses to *start* until the
                // sides are even is a lobby where they sort it out and press
                // it again.
                if !self.sides.is_empty() {
                    self.teams_are_fair()?;
                }
                self.phase = Phase::Running { from: self.tick() };
                self.started_by = by;
                self.lobby_changed = true;
                // **Everybody spawns at the whistle, together.** Granting on
                // arrival would put the first player's block on a world the
                // last player has not seen yet, and would hand out seats in
                // the order people happened to click -- so a match's world is
                // empty until it starts, and then it is laid out all at once.
                //
                // In player order rather than in whatever order the map
                // iterates, because grants are laid on a spiral by number and
                // two peers must lay the same one.
                let mut here: Vec<PlayerId> =
                    self.players.values().filter(|p| p.online).map(|p| p.id).collect();
                here.sort_unstable();
                for id in here {
                    self.grant_territory(id);
                }
                Ok(())
            }
            Phase::Open => Err("that room is not a match".into()),
            Phase::Running { .. } => Err("that match is already running".into()),
            Phase::Over { .. } => Err("that match is over".into()),
        }
    }

    /// Whether this number still has anybody playing it.
    ///
    /// **The question a forfeit asks**, and it composes with teams for free: a
    /// seat plays a number, so a number is in the match while at least one
    /// seat playing it is here and has not given up. A lone player is their
    /// own number, so giving up puts them out; a team is out when everybody at
    /// its controls has gone or conceded, and one of three walking away leaves
    /// two hands on it.
    ///
    /// **Not `online`.** A connection that dropped is a player who can come
    /// back with their token — that is what the token is for — so being away
    /// is not being out. Only giving up is.
    pub fn still_in(&self, player: PlayerId) -> bool {
        self.seats().any(|p| p.plays_as == player && !p.forfeited)
    }

    /// Every row somebody sits in, which is every row that is not a team.
    ///
    /// A team has a `Player` row like anybody else — that is what makes it a
    /// player — so anything counting *people* has to leave them out, or a team
    /// counts as one of its own members.
    fn seats(&self) -> impl Iterator<Item = &Player> {
        self.players.values().filter(|p| !self.sides.contains(&p.id))
    }

    /// Every number still playing, in order.
    fn survivors(&self) -> Vec<PlayerId> {
        let mut left: Vec<PlayerId> =
            self.seats().filter(|p| !p.forfeited).map(|p| p.plays_as).collect();
        left.sort_unstable();
        left.dedup();
        left
    }

    /// Give up, for this seat.
    ///
    /// Only while a match is running. A world has nothing to concede — there
    /// is no result — and a gathering match is one you leave rather than lose.
    pub fn forfeit(&mut self, seat: PlayerId) -> Result<(), String> {
        if !matches!(self.phase, Phase::Running { .. }) {
            return Err("no match is running here".into());
        }
        let Some(player) = self.players.get_mut(&seat) else {
            return Err("you are not in this match".into());
        };
        if player.forfeited {
            return Err("you have already given up".into());
        }
        player.forfeited = true;
        let (name, plays) = (player.name.clone(), player.plays_as);
        log::info!("{seat:?} \"{name}\" gave up");
        self.lobby_changed = true;
        // Said out loud when it takes a whole number out, because that is the
        // moment it changes the match rather than one player's evening.
        if !self.still_in(plays) {
            log::info!("{plays:?} is out of match \"{}\"", self.room);
        }
        // **And a match with one number left is over.** Checked here rather
        // than in `decide`, because "one player remains" is only a result when
        // the others *conceded*: a match that simply started with one player
        // in it has not been won by anybody, and putting this in `decide`
        // ended every such match on its first generation.
        let left = self.survivors();
        if left.len() <= 1 {
            let held = self.territory();
            let winner = left.first().copied();
            let count = winner.map_or(0, |w| held[w.0 as usize]);
            self.phase = Phase::Over { winner, held: count, at: self.tick() };
            log::info!("match \"{}\" ended with everybody else out: {winner:?}", self.room);
        }
        Ok(())
    }

    /// Call it off early, with the score as it stands.
    ///
    /// Whoever started the match may end it, which is the same person and the
    /// same reasoning: they arranged it, so they are the one who can say it is
    /// over when it has stopped being worth playing. The result is real —
    /// whoever leads at that moment wins, and it is rated — because a match
    /// that ends with no result is one nobody can be held to.
    pub fn end_match(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::Running { .. }) {
            return Err("no match is running here".into());
        }
        let held = self.territory();
        let (winner, count) = leader(&held);
        self.phase = Phase::Over { winner, held: count, at: self.tick() };
        self.lobby_changed = true;
        log::info!("match \"{}\" was called off at tick {}", self.room, self.tick());
        Ok(())
    }

    /// Has this match been decided, and by whom.
    ///
    /// Checked after a step rather than before, so the generation that met the
    /// condition is the one the score is read from.
    pub(super) fn decide(&mut self) -> Option<&Phase> {
        let (Some(victory), Phase::Running { from }) = (self.victory, self.phase.clone()) else {
            return None;
        };
        let held = self.territory();
        // **A side is scored as one.** Territory is still contested per player
        // — two allies keep a border between their ground, they simply cannot
        // be hurt by it — so the sum is taken here, at the one place a result
        // is decided, rather than by teaching the rule about teams.
        let (winner, count) = leader(&held);
        let done = match victory {
            Victory::Timer { generations } => self.tick().saturating_sub(from) >= generations,
            Victory::Territory { squares } => count >= squares,
        };
        if !done {
            return None;
        }
        self.phase = Phase::Over { winner, held: count, at: self.tick() };
        self.lobby_changed = true;
        log::info!(
            "match \"{}\" is over at tick {}: {}",
            self.room,
            self.tick(),
            match winner {
                Some(id) => format!("{id:?} holds {count} squares"),
                None => "nobody held anything".into(),
            }
        );
        Some(&self.phase)
    }

    /// Who finished this match, for whatever wants to rate it.
    ///
    /// Only players with a person: somebody who joined without a key is not
    /// somebody this server can remember, so there is nowhere to put a result
    /// for them. The rest are still rated against each other — see
    /// [`ratings::Ratings::settle`].
    ///
    /// A **side's** score rather than a seat's, because that is what a result
    /// is between: two allies win or lose the same match. Nothing here has to
    /// add a side up any more — a side's cells all carry its own number, so
    /// `territory` has already counted them under it.
    ///
    /// Side rows are skipped for free: they carry no person, and a result has
    /// nowhere to go without one.
    pub fn finishers(&self) -> Vec<crate::server::profiles::Finisher> {
        let held = self.territory();
        self.players()
            .filter_map(|p| {
                Some(crate::server::profiles::Finisher {
                    who: crate::net::PersonId(p.person.clone()?),
                    // What they last joined under, so a profile has a name on
                    // it before anybody looks it up.
                    name: p.name.clone(),
                    team: p.plays_as.0,
                    score: held[p.plays_as.0 as usize],
                })
            })
            .collect()
    }

    /// Whether this match was decided on the generation just stepped.
    pub fn just_decided(&self) -> bool {
        matches!(self.phase, Phase::Over { at, .. } if at == self.tick())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gathering_world_does_not_step_and_a_running_one_does_not_admit() {
        assert!(!Phase::Gathering.stepping(), "nothing moves before the whistle");
        assert!(!Phase::Gathering.accepts_actions(), "and nobody does anything either");
        assert!(Phase::Gathering.open_to_newcomers());

        let running = Phase::Running { from: 0 };
        assert!(running.stepping() && running.accepts_actions());
        assert!(!running.open_to_newcomers(), "no late joining");

        let over = Phase::Over { winner: None, held: 0, at: 10 };
        assert!(!over.stepping(), "a decided match stops");
        assert!(!over.accepts_actions(), "and cannot be played on afterwards");
        assert!(!over.open_to_newcomers());

        assert!(Phase::Open.stepping() && Phase::Open.accepts_actions());
        assert!(Phase::Open.open_to_newcomers());
    }

    #[test]
    fn win_conditions_read_back_as_they_were_typed() {
        assert_eq!(Victory::parse("timer", "2000"), Ok(Victory::Timer { generations: 2000 }));
        assert_eq!(Victory::parse("territory", "500"), Ok(Victory::Territory { squares: 500 }));
        assert!(Victory::parse("timer", "0").is_err(), "over before it began");
        assert!(Victory::parse("timer", "soon").is_err());
        assert!(Victory::parse("vibes", "3").is_err());
    }

    #[test]
    fn the_leader_is_who_holds_most_and_ties_go_to_the_lower_number() {
        let mut held = [0usize; PlayerId::COUNT];
        assert_eq!(leader(&held), (None, 0), "nobody holds anything");

        held[3] = 10;
        held[7] = 40;
        assert_eq!(leader(&held), (Some(PlayerId(7)), 40));

        held[3] = 40;
        assert_eq!(leader(&held), (Some(PlayerId(3)), 40), "a tie is broken by number");
    }
}
