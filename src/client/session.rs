//! **What this client is to a server**: the link, the seat, the purse, and the
//! machinery that keeps one world in step with another.
//!
//! Split out of [`crate::client::views::game`], which was a *view* by where it
//! lived and was not one by what it did — it held the world, the link and the
//! GPU pipeline in one struct with fifty fields and folded server messages
//! into the world from the middle of a frame. That is the arrangement the
//! [Data / Logic / Interface] rule names, and every other view already avoids
//! it through the `Shown`/`Chose` return-value convention.
//!
//! So this takes messages in and produces two things: **mutations of a world
//! it does not own**, and [`Effect`]s, which are the parts an interface has to
//! do — move a camera, put a screen up, say something in a corner. It needs no
//! GPU and no egui, which is what makes it testable, and none of it was.
//!
//! The world is a parameter rather than a field. It belongs beside the chunk
//! store that draws it, and passing it in is what lets a test step a session
//! against a world with no window anywhere near either.
//!
//! [Data / Logic / Interface]: ../../../docs/inspiration.md#the-architecture

use std::collections::HashSet;

use crate::net::link::Link;
use crate::net::{
    Action, ClientMessage, Holding, Placement, RoomId, RoomInfo, Rules, ServerMessage, Stamped,
    Tick, Victory,
};
use crate::sim::{Coord, Player, PlayerId, World};

/// How often a client asks the server whether they still agree, in
/// generations. Four a second, so this is every few seconds.
const CHECKPOINT_EVERY: u64 = 12;

/// The most chunks one checkpoint carries. Sixteen bytes each, so even the cap
/// is a small message; it exists so a client holding an enormous world cannot
/// send an enormous one.
const MAX_CHECKPOINT_CHUNKS: usize = 512;

/// How often the room list is asked for again while it is on screen.
const ROOM_LIST_REFRESH: f64 = 3.0;

/// How long to wait for a server to say what rooms it has before giving up.
///
/// Generous, because it covers a connection being made as well as answered,
/// and short enough that a wrong address is a mistake you correct rather than
/// a page you reload.
const ROOM_LIST_TIMEOUT: f64 = 8.0;

/// What a `Step` at this tick means for a client sitting at that generation.
///
/// Pure arithmetic, and a free function so it can be tested without a GPU —
/// the same argument that took the camera out of the view. Everything
/// interesting about recovering from a lost message is the decision, and the
/// decision is one comparison.
#[derive(Debug, PartialEq, Eq)]
enum Advance {
    /// The next generation, which is the only thing that ever ought to arrive.
    Step,
    /// Something was lost. Not "we are behind by n" — a `Step` carries the
    /// actions applied at its tick, so a gap is n generations whose *contents*
    /// this client was never told, and there is nothing it can compute that
    /// would fill them in.
    Lost,
}

fn advance(here: u64, tick: Tick) -> Advance {
    if tick == here + 1 {
        Advance::Step
    } else {
        Advance::Lost
    }
}

/// A join that has been decided on and not yet sent.
///
/// The room and the name; the secret is looked up at the moment it goes,
/// because it can change while this waits.
struct Joining {
    name: String,
    room: Option<RoomId>,
}

/// **What the session cannot do itself**, handed back for the interface to do.
///
/// Everything here needs a screen, a camera or a menu, which is exactly the
/// line this module is on the other side of. It is the same convention every
/// view already uses to answer a frame — a value saying what happened, acted
/// on by whoever owns the thing it happened to.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// A different world arrived, and the board is what to look at now. The
    /// chunk store and the camera are both stale.
    Entered,
    /// Put the camera here: a spawn, or a grant made at the whistle.
    ///
    /// Its own effect rather than part of [`Self::Entered`], because a match
    /// lays everybody out at the whistle and the spawn in a `Welcome` was
    /// worked out before any of that existed.
    LookAt((i32, i32)),
    /// This client's rating moved, and a home screen showing one should say so.
    Rated,
    /// Somebody's profile arrived, into whatever asked for it.
    LookedUp,
    /// A list of who else plays here arrived, into whatever asked for it.
    FoundPeople,
    /// The server would not have us, and this is why. The link is kept and its
    /// room list asked for again, so the next choice is a click.
    Refused(String),
    /// A whistle that was not blown. Belongs against the control that produced
    /// it, which is the lobby rather than the HUD's corner.
    NotStarted(String),
    /// The phase moved, or somebody took a side. Whatever the last whistle was
    /// refused for has been answered.
    LobbyMoved,
    /// A room was made. Join it — which is the same `Join` the room list
    /// sends, so there is one way into a world rather than two.
    Made { id: RoomId, code: Option<String> },
    /// It would not make one, and this is why. Belongs in the form that asked.
    NotMade(String),
    /// The room list arrived.
    Rooms(Vec<RoomInfo>),
    /// The link closed. What that means depends on which screen is up, which
    /// is why it is not decided here.
    Closed,
}

/// **A rating as a screen shows it**: the number, whether it has been earned,
/// and what the last result moved it by.
///
/// Three named things rather than a tuple of three, two of which are the same
/// shape — the argument [`Shown`] makes, and the one `here()` was rewritten
/// for.
///
/// [`Shown`]: crate::client::views::Shown
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rating {
    pub number: i32,
    /// Unearned. An Elo from a fixed start means nothing until it has moved,
    /// so it is marked until it has.
    ///
    /// The **server's** answer rather than a comparison done here: the
    /// threshold is its policy, and a client that re-derived it would show a
    /// different mark the day the server changed its mind.
    pub provisional: bool,
    /// How many matches are behind it, which is the whole of what the mark
    /// means. Shown beside the word, because "provisional" on its own is a
    /// label somebody has to already know.
    pub games: u32,
    /// What the last result moved it by. Shown once, beside the number.
    pub change: Option<i32>,
}

/// The client's side of a room.
pub struct Session {
    /// The server connection, if there is one. A client with no link still
    /// simulates: the rules are deterministic, so offline is a game of one
    /// rather than a broken game.
    link: Option<Link>,
    /// A join decided on and not yet sent.
    ///
    /// **Nothing is waited for now.** A join used to be held until the server
    /// sent a challenge to sign, which was a round trip before every join and
    /// a state to get wrong; `Link` already holds messages until the socket is
    /// open, so this is only ever set for one frame.
    joining: Option<Joining>,
    /// Our own player number, once the server has issued one.
    ///
    /// `None` offline before the first grant, and `None` for the whole of a
    /// spectator's visit — see [`Self::watching`].
    pub me: Option<PlayerId>,
    /// **The number this client's cells carry**, which in a team match is the
    /// team's and not this seat's — and the whole of what a client knows about
    /// teams. Everything downstream asks this and never asks who is allied
    /// with whom, because there is nobody to be allied with. Equal to
    /// [`Self::me`] offline and in a free-for-all.
    pub plays_as: Option<PlayerId>,
    /// Watching without a seat.
    ///
    /// Its own flag rather than `me.is_none()`, because those are two
    /// different states that happen to share a field: a client between a
    /// `Join` and its `Welcome` also has no number, and it is not a spectator.
    /// Everything that acts asks this first.
    pub watching: bool,
    /// Whether this seat has given up, as the server last said. Held so the
    /// control can say so rather than offer to concede twice.
    pub forfeited: bool,
    /// **What this server says about this client** — who it calls them, what
    /// they are rated, and what they have done here.
    ///
    /// The client cannot derive any of it, which is the point: anything
    /// another player is shown has to be the server's, or a rating you keep is
    /// a rating you can type. `None` until a server has said, which is a
    /// different thing from the starting number — a client that has reached
    /// nobody has no rating rather than an average one, and a dashboard
    /// showing 1200 to somebody who has never connected would be inventing it.
    pub profile: Option<crate::net::Profile>,
    /// What the last result moved the rating by, so the dashboard can say so
    /// once. Beside the profile rather than in it: a change is a fact about
    /// the match just finished, not about a person.
    pub rating_change: Option<i32>,
    /// Somebody else's profile, once it has been asked for. Whose is on it, so
    /// a slow answer cannot be shown against the wrong name.
    pub looked_up: Option<crate::net::Profile>,
    /// Who else plays here, once it has been asked, with the query that
    /// produced it — so a reply to a prefix the box no longer holds is
    /// dropped rather than shown. `None` until the first ask.
    pub people: Option<(String, Vec<crate::net::Profile>)>,
    /// Which room the server put us in, once it has said.
    ///
    /// Taken from the `Welcome` rather than from what was asked for: a client
    /// may have named no room at all. `None` while offline.
    pub room: Option<RoomId>,
    /// What that room is **called**, for the HUD. Beside the id rather than
    /// looked up from the room list, because a client that joined by code has
    /// never seen a listing.
    pub room_name: Option<String>,
    /// What the match in this room is doing, once the server has said. `None`
    /// in an ordinary room, and in one that has not answered yet.
    pub lobby: Option<crate::net::Lobby>,
    /// Who holds how much ground, most first, as the server last said.
    ///
    /// From the server because a client holds only the chunks it subscribed
    /// to: counting locally would score its own screen rather than the world.
    pub standing: Vec<Holding>,
    /// The generation [`Self::standing`] was counted at, offline. A pass over
    /// the world per generation and not per frame.
    counted_at: Tick,
    /// What this player can spend. Predicted locally with the same arithmetic
    /// the server charges by, so the number on screen is the number the server
    /// will agree with.
    pub value: i32,
    /// Chunks already asked for, so a moving viewport only asks for what is new.
    subscribed: HashSet<Coord>,
    /// Actions taken from an `Acted` before the `Step` that carries them.
    ///
    /// Emptied every `Step`, because an action belongs to one generation and
    /// the `Step` that names it is the last chance to hear about it.
    applied_early: Vec<Stamped>,
    /// How badly this client and the server are disagreeing, as a decaying
    /// rate rather than a log line — see [`crate::client::desync`].
    pub geiger: crate::client::desync::Geiger,
    /// When the room list was asked for, so a server that never answers
    /// becomes a message rather than a menu that says "asking" forever.
    asked_at: Option<f64>,
    /// When the room list last arrived, so it can be asked for again before it
    /// goes stale.
    listed_at: f64,
    /// The game being played, if there is one, waiting to be filed.
    ///
    /// Committed when the room ends for this client — a different `Welcome`, a
    /// link that closed, or the way back to the menu. A tab closed mid-game
    /// loses its record, which is the honest cost of not writing on every
    /// change: a browser gives no reliable moment to write at.
    ///
    /// A spectator never has one. Watching is not playing, and a record of
    /// worlds you looked at is not a record.
    in_play: Option<crate::client::record::InPlay>,
    /// **What the game is doing in this room** — see [`Rules`]. Taken from the
    /// server rather than decided here; offline this client is the server, so
    /// it sets them itself and nothing downstream can tell.
    pub rules: Rules,
    /// One generation asked for while stopped, taken on the next frame.
    ///
    /// Offline only; in a room [`ClientMessage::StepOnce`] asks instead and
    /// the generation comes back as the `Step` everybody else gets.
    step_once: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// A client that has reached nobody.
    pub fn new() -> Self {
        Self {
            link: None,
            joining: None,
            me: None,
            plays_as: None,
            watching: false,
            forfeited: false,
            profile: None,
            rating_change: None,
            looked_up: None,
            people: None,
            room: None,
            room_name: None,
            lobby: None,
            standing: Vec::new(),
            counted_at: u64::MAX,
            value: Player::STARTING_VALUE,
            subscribed: HashSet::new(),
            applied_early: Vec::new(),
            geiger: Default::default(),
            asked_at: None,
            listed_at: 0.0,
            in_play: None,
            rules: Rules::default(),
            step_once: false,
        }
    }

    /// Whether there is a server on the other end of this.
    pub fn connected(&self) -> bool {
        self.link.is_some()
    }

    /// Whether a first word is still being waited for.
    pub fn asking(&self) -> bool {
        self.asked_at.is_some()
    }

    /// **The number this client places under**: its team's in a match, its own
    /// otherwise. Every rule takes this and knows nothing about teams. Before
    /// the server has said, we are player one — offline is a game of one
    /// rather than a game of nobody.
    pub fn player(&self) -> PlayerId {
        self.plays_as.or(self.me).unwrap_or(PlayerId(1))
    }

    /// What this client is rated here, and whether that number is earned yet.
    ///
    /// `None` before a server has said. Every screen that shows a rating asks
    /// this rather than reaching into the profile, so "not connected", "not
    /// earned" and a figure are three answers to one question.
    pub fn rating(&self) -> Option<Rating> {
        self.profile.as_ref().map(|p| Rating {
            number: p.rating,
            provisional: p.provisional,
            games: p.games,
            change: self.rating_change,
        })
    }

    /// Ask what this server says about somebody. The answer is an
    /// [`Effect::LookedUp`].
    pub fn look_up(&mut self, who: crate::net::PersonId) {
        self.looked_up = None;
        self.tell(ClientMessage::Profile { who });
    }

    /// Ask who else plays here. An empty `like` asks for the best rated, which
    /// is the leaderboard. The answer is an [`Effect::FoundPeople`].
    ///
    /// **What was asked is not cleared.** A search box is retyped a character
    /// at a time and every keystroke asks again, so blanking the list on each
    /// one makes it flicker; the previous answer stays up, slightly stale, and
    /// is replaced when the new one lands.
    pub fn find_people(&mut self, like: &str) {
        self.tell(ClientMessage::People { like: like.to_string() });
    }

    /// Whether this room is a match that has not started.
    pub fn gathering(&self) -> bool {
        matches!(self.lobby.as_ref().map(|l| &l.phase), Some(crate::net::MatchPhase::Gathering))
    }

    /// Whether the world will take anything this player does right now.
    ///
    /// A match that has not started takes nothing, and neither does one that
    /// is decided — the server drops those actions, and a client that went on
    /// predicting them would draw cells that appear under the hand and vanish
    /// a moment later when the next `Checkpoint` corrects the world.
    ///
    /// A spectator has no seat, so no number to attribute an action to and no
    /// value to spend. Refused here as well as on the server, so that clicking
    /// the world says why rather than doing nothing.
    pub fn may_act(&self) -> bool {
        !self.watching && self.lobby.as_ref().is_none_or(|l| l.phase.accepts_actions())
    }

    /// **Whether the clock is this client's to press**, which is to say
    /// whether this is a laboratory.
    ///
    /// A world runs, and a match runs when the whistle goes; stopping either
    /// is not a thing the game offers, and a solitary world is still a world.
    /// It was `link.is_none()` as well, so a plain solo game had a pause
    /// button that nothing in the design ever meant it to have.
    pub fn own_clock(&self) -> bool {
        self.rules.laboratory
    }

    /// Whether this client may put something on that square.
    ///
    /// One question in one place, so a laboratory takes the rule off
    /// everywhere it is asked rather than at three of the four call sites.
    pub fn may_place_at(&self, world: &World, row: i32, col: i32) -> bool {
        crate::net::may_place_under(world, self.player(), row, col, &self.rules)
    }

    /// What an action costs, which in a laboratory is nothing.
    pub fn price(&self, world: &World, stamped: &Stamped) -> i32 {
        crate::net::price_under(world, stamped, &self.rules)
    }

    // ---- what a gesture becomes -------------------------------------------

    /// Price an action on these cells: what would be sent, and what it costs.
    ///
    /// Shared by the click, by a drag, and by the preview of a drag, so the
    /// preview cannot promise something the release then refuses and a drag
    /// cannot be priced differently from the click it is made of.
    pub fn quote(
        &self,
        world: &World,
        cells: Vec<(i32, i32)>,
        taking: bool,
        placement: Placement,
    ) -> (Stamped, i32) {
        let action = if taking {
            Action::Erase { cells, placement }
        } else {
            Action::Paint { cells, placement }
        };
        let stamped = Stamped {
            tick: world.generation,
            player: self.player(),
            // This client, as against the number it plays under. In a team
            // match they differ, and telling them apart is what stops a
            // teammate's action being mistaken for one this client predicted.
            seat: self.me.unwrap_or_else(|| self.player()),
            action,
        };
        let delta = self.price(world, &stamped);
        (stamped, delta)
    }

    /// Apply an action here, and send it if there is anyone to send it to.
    ///
    /// Applied straight away, connected or not, so what you draw appears under
    /// your hand rather than a quarter of a second later. The rules are
    /// deterministic and the server runs the same `net::apply`, so acting
    /// immediately shows the right answer a round trip early.
    ///
    /// Usually. The server applies it whenever the message lands, which is
    /// this generation if it arrives before the next step and the one after if
    /// it arrives later — so a click is a coin flip, and on the losing side
    /// this world has evolved those cells a generation earlier than the
    /// server's. That is what `Checkpoint` is for: the divergence is real,
    /// rare, and found by comparing digests rather than prevented by waiting.
    pub fn commit(&mut self, world: &mut World, stamped: &Stamped) {
        crate::net::apply(world, stamped);
        world.dirty = true;
        if let Some(link) = &self.link {
            link.send(ClientMessage::Act(stamped.clone()));
        }
    }

    /// Take a priced action's cost out of the purse, floored and capped the
    /// way the server does it.
    pub fn spend(&mut self, delta: i32) {
        self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
    }

    // ---- the clock ---------------------------------------------------------

    /// Change what the game is doing here.
    ///
    /// Applied at once **and** sent, the way an action is: the server answers
    /// with a `Rules` broadcast that everybody in the room gets, and until it
    /// lands this client's copy is right rather than a round trip behind its
    /// own press. Offline there is nobody to send to, and this client is the
    /// authority.
    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
        if let Some(link) = &self.link {
            link.send(ClientMessage::SetRules(rules));
        }
    }

    /// Ask for one generation and stay stopped.
    ///
    /// **Asked for rather than taken**, in a room as much as offline: the
    /// generation comes back as the `Step` everybody else in the laboratory
    /// gets, so one person stepping is not a world only they can see.
    pub fn ask_for_one_step(&mut self) {
        self.set_rules(Rules { paused: true, ..self.rules });
        match self.link.as_ref() {
            Some(link) => link.send(ClientMessage::StepOnce),
            None => self.step_once = true,
        }
    }

    /// **Empty this laboratory.**
    ///
    /// Asked for in a room, because several people share one and the world
    /// they are all looking at is the room's — the answer comes back as the
    /// `Resync` everybody gets. Offline there is nobody to ask and this client
    /// is the authority, so it is the same clearing `resync_everything` does
    /// when a world has to be thrown away.
    pub fn wipe(&mut self, world: &mut World) {
        match self.link.as_ref() {
            Some(link) => link.send(ClientMessage::Wipe),
            None => {
                let tick = world.generation;
                self.resync_everything(world, tick);
            }
        }
    }

    /// Step a world nobody else is keeping time in, and bank what it mined.
    ///
    /// Offline only, and guarded on it: connected, the world advances when the
    /// server says a generation happened and never on this client's own clock
    /// — see [`Self::advance_to`].
    pub fn advance_alone(&mut self, world: &mut World, dt: f32, span: f32) {
        if self.link.is_some() {
            return;
        }
        // **Stopped means stopped, and one means one.** `World::update` banks
        // elapsed time against the span, so not calling it is the whole of
        // pausing: no time accumulates, and letting go does not release a
        // burst of generations that built up while stopped. A decided match
        // stops too, so a solitary match ends on the generation it was won.
        let stopped = self.lobby.as_ref().is_some_and(|l| !l.phase.stepping());
        let mined = match (self.rules.paused || stopped, std::mem::take(&mut self.step_once)) {
            (false, _) => world.update(dt, span),
            (true, true) => world.step(),
            (true, false) => crate::sim::Mined::default(),
        };
        self.bank(&mined);
        // **And its own standings**, which nothing else produces: they arrive
        // in a `ServerMessage::Standing` and offline there is no server, so
        // every figure that reads them sat at nought for a whole solo game.
        //
        // Counted exactly rather than guessed. A *connected* client holds its
        // screen and a margin, which is why it takes the server's figure;
        // offline it owns the whole world, so the same count the server would
        // do is the right one. `net::standings` is that count, shared so the
        // two cannot disagree.
        if world.generation != self.counted_at {
            self.counted_at = world.generation;
            self.standing = crate::net::standings(world);
            self.decide_alone(world);
        }
    }

    /// Settle a solitary match, if there is one and it is done.
    ///
    /// The same two conditions the server settles, read off the same
    /// `net::standings` — offline this client holds the whole world, so its
    /// count *is* the server's. Guarded on there being no link: two
    /// authorities deciding one match is the bug the whole design avoids.
    fn decide_alone(&mut self, world: &World) {
        if self.link.is_some() {
            return;
        }
        let Some(lobby) = &mut self.lobby else { return };
        let (Some(victory), crate::net::MatchPhase::Running { from }) =
            (lobby.victory, lobby.phase.clone())
        else {
            return;
        };
        let leader = self.standing.first();
        let held = leader.map_or(0, |h| h.score as usize);
        let done = match victory {
            Victory::Timer { generations } => world.generation.saturating_sub(from) >= generations,
            Victory::Territory { squares } => held >= squares,
        };
        if !done {
            return;
        }
        lobby.phase = crate::net::MatchPhase::Over {
            winner: leader.map(|h| h.who),
            held,
            at: world.generation,
        };
        log::info!("a solitary match ended at generation {}", world.generation);
    }

    /// Step the world up to the generation the server is on.
    ///
    /// Normally exactly one step: the server sends one of these per
    /// generation. Anything else means this client and the server disagree
    /// about where in the sequence they are, which is not something to paper
    /// over quietly — the worlds have already diverged, and the honest thing
    /// is to say so, take the server's number, and ask for the world again.
    fn advance_to(&mut self, world: &mut World, tick: Tick) {
        let here = world.generation;
        if let Advance::Step = advance(here, tick) {
            let mined = world.step();
            self.bank(&mined);
            return;
        }
        // **Anything else means messages were lost, so nothing is stepped.**
        //
        // Catching up is the bug rather than the recovery: a `Step` carries
        // the actions applied at its tick, so a gap is not "we are behind" but
        // "n generations happened whose contents we were not told" — and
        // stepping to close it runs those generations *empty*, which is a
        // world nobody else has within a minute.
        //
        // A websocket does not lose or reorder; the broadcast channel in front
        // of it does, and a backgrounded tab is the ordinary case rather than
        // an exotic one.
        log::warn!(
            "out of step: the server is at {tick} and this client at {here}; \
             discarding {} generation(s) and asking again",
            tick.saturating_sub(here)
        );
        self.resync_everything(world, tick);
    }

    /// Throw away what this client holds and fetch it from the server.
    ///
    /// **The whole world, not the chunks that look wrong.** Every chunk was
    /// stepped alongside every other, so one that missed an action has been
    /// feeding wrong cells across its edges ever since — and a chunk outside
    /// the viewport is never checkpointed, so "the ones we know are wrong" is
    /// a set this client cannot compute.
    fn resync_everything(&mut self, world: &mut World, tick: Tick) {
        // **Keeping the seed**, which the shape does not carry: `build` makes
        // a world that has not been told which game it is, and a client
        // rolling from nought against a server rolling from the room's number
        // disagrees at the first contested birth. That reads as a desync, so
        // it resyncs, so it rebuilds, so it disagrees again -- which is what
        // `examples/two` caught the moment this was wired up.
        let seed = world.seed();
        *world = world.kind().build();
        world.set_seed(seed);
        world.set_generation(tick);
        world.dirty = true;
        // Cleared, or `subscribe` would take this client's word for what it
        // already holds and ask for none of it.
        self.subscribed.clear();
        // And this, which is a record of what was applied to the world that
        // has just been thrown away. Kept, it would tell the next `Step` to
        // skip an action that nothing in the new world has ever seen.
        self.applied_early.clear();
        self.geiger.reset();
    }

    /// Fold a generation's mining into the predicted purse, floored at zero
    /// the way the server floors the real one.
    ///
    /// A prediction, and a low one: only the mines in chunks this client holds
    /// are counted. `Purse` is what makes it right again.
    fn bank(&mut self, mined: &crate::sim::Mined) {
        self.spend(crate::net::earnings(mined, self.player()));
    }

    // ---- the wire ----------------------------------------------------------

    /// Drain the socket and fold what arrived into the world.
    ///
    /// What comes back is what an interface has to do about it; everything
    /// else has already been done. See [`Effect`].
    pub fn pump(&mut self, world: &mut World, now: f64) -> Vec<Effect> {
        let mut effects = Vec::new();
        let Some(link) = &mut self.link else { return effects };
        let messages = link.drain();
        let closed = link.is_closed();

        for msg in messages {
            match msg {
                ServerMessage::Welcome {
                    you,
                    tick,
                    spawn,
                    profile,
                    value,
                    room,
                    name,
                    world: shape,
                    rules,
                } => {
                    // The way back is the secret this client already has, so
                    // there is nothing to file per room. What is kept is who
                    // the server said we are, so the settings screen can say
                    // it without waiting for the next join — the server issues
                    // it, so this is the only way the client ever has it.
                    if let Some(mine) = &profile {
                        crate::net::keep::remember_person(&mine.who);
                    }
                    self.profile = profile;
                    // A different room is a different match, or none.
                    self.lobby = None;
                    log::info!(
                        "joined \"{name}\" ({room}) as {you:?} at tick {tick}, in a {} world",
                        describe(shape)
                    );
                    // Taken from the server, not assumed: a player coming back
                    // has a value already, and guessing the starting figure
                    // would have this client offering to spend money the
                    // server knows is gone.
                    self.value = value;
                    self.me = Some(you);
                    self.plays_as = Some(you);
                    self.watching = false;
                    self.forfeited = false;
                    self.room = Some(room);
                    self.room_name = Some(name);
                    self.asked_at = None;
                    self.enter(world, shape, tick, rules);
                    self.in_play = Some(crate::client::record::InPlay::joined(
                        self.room_name.clone().unwrap_or_default(),
                        shape,
                        tick,
                    ));
                    effects.push(Effect::Entered);
                    // Look at our own ground, which is the only place we may
                    // build. Derived rather than sent: `spawn_for` is the same
                    // function on both sides.
                    effects.push(Effect::LookAt(spawn));
                }
                // Watching: the world and its clock, and no player at all.
                ServerMessage::Watching { room, name, tick, world: shape, rules } => {
                    log::info!("watching \"{name}\" ({room}) from tick {tick}");
                    self.lobby = None;
                    self.me = None;
                    self.watching = true;
                    self.value = 0;
                    self.room = Some(room);
                    self.room_name = Some(name);
                    self.asked_at = None;
                    self.enter(world, shape, tick, rules);
                    // No spawn to look at, because nothing here is ours. The
                    // camera stays where it was, which for a fresh client is
                    // the origin — and the origin is where the first grant
                    // goes, so it is where anything is likely to be.
                    effects.push(Effect::Entered);
                }
                // A result, and what it did to somebody's number. Ours moves
                // the dashboard; everybody else's is the half of a rating that
                // makes it a comparison rather than a score, and is worth a
                // line in the log until there is a screen for it.
                ServerMessage::Rated { who, rating, change } => {
                    if self.profile.as_ref().map(|p| &p.who) == Some(&who) {
                        log::info!("rated {rating} ({change:+})");
                        if let Some(mine) = &mut self.profile {
                            mine.rating = rating;
                            // A result is one of the matches a rating stops
                            // being provisional after, and the server said so
                            // by sending this — but only it knows the count,
                            // so what this can do is show the new number and
                            // wait for the next join to settle the mark.
                            mine.games += 1;
                        }
                        self.rating_change = Some(change);
                        effects.push(Effect::Rated);
                    } else {
                        log::info!("{who} is now rated {rating} ({change:+})");
                    }
                }
                // What this server says about somebody else, asked for from a
                // lobby or a standings bar.
                ServerMessage::Profile(found) => {
                    match &found {
                        Some(p) => log::debug!("{} is rated {}", p.label(), p.rating),
                        None => log::debug!("this server has not met them"),
                    }
                    self.looked_up = found;
                    effects.push(Effect::LookedUp);
                }
                // Somebody was given their opening ground. Ours moves the
                // camera, because a match lays everybody out at the whistle
                // and the spawn in our `Welcome` was worked out before any of
                // it existed — it is stale by the time it matters, which is
                // why this used to need a reload.
                // **Only if it still answers what is being asked.** Replies
                // arrive in the order the server sent them and the box has
                // moved on since; one that overwrote the list would show
                // results for a prefix that is no longer there.
                ServerMessage::People { like, found } => {
                    log::debug!("{} people like {like:?}", found.len());
                    self.people = Some((like, found));
                    effects.push(Effect::FoundPeople);
                }
                ServerMessage::Spawned { player, at } => {
                    if Some(player) == self.plays_as {
                        log::info!("granted ground at {at:?}");
                        effects.push(Effect::LookAt(at));
                    }
                }
                ServerMessage::Rejected { reason } => {
                    log::error!("server refused the connection: {reason}");
                    // The refusal names the rooms that do exist, which with no
                    // other listing is the most useful thing on the screen —
                    // and the link is kept, so the next choice is a click
                    // rather than a reconnect.
                    self.ask_for_rooms(now);
                    effects.push(Effect::Refused(reason));
                    return effects;
                }
                // Who is winning. Kept whole rather than merged, because a
                // player who has lost every square drops out of the list and a
                // merge would leave their last bar standing forever.
                ServerMessage::Match(lobby) => {
                    // A decided match is a result, and the only moment this
                    // client is told one. Recorded when it arrives rather than
                    // when the room is left, because a player who watches the
                    // final board and then closes the tab still played it.
                    if let (crate::net::MatchPhase::Over { winner, .. }, Some(live)) =
                        (&lobby.phase, self.in_play.as_mut())
                    {
                        live.decided(*winner == self.plays_as && self.plays_as.is_some());
                    }
                    // **Which player this client is now.** Joining a team is
                    // taking the controls of the team's player, so from here
                    // on this client's cells carry the team's number: what it
                    // may place, what that costs and which of the actions
                    // coming back are its own all follow from this one line.
                    //
                    // Read out of the roster rather than sent per connection,
                    // because `Match` is broadcast to the whole room and a
                    // field naming one recipient would be wrong for the rest.
                    // A seat on no team plays as itself, which is a
                    // free-for-all.
                    self.plays_as = self
                        .me
                        .map(|me| {
                            let mine = lobby.teams.iter().find(|t| t.players.contains(&me));
                            mine.map_or(me, |t| t.id)
                        })
                        .or(self.plays_as);
                    self.lobby = Some(lobby);
                    effects.push(Effect::LobbyMoved);
                }
                // The room's clock or its switches moved, for everybody in it
                // at once. Taken rather than reconciled: this client may have
                // predicted its own press, and anybody else's is news.
                ServerMessage::Rules(rules) => {
                    log::debug!("the room's rules are now {rules:?}");
                    self.rules = rules;
                }
                ServerMessage::Standing { held, .. } => {
                    if let (Some(live), Some(me)) = (self.in_play.as_mut(), self.plays_as) {
                        let mine = held.iter().find(|h| h.who == me);
                        live.holding(mine.map(|h| h.score).unwrap_or(0));
                    }
                    self.standing = held;
                }
                ServerMessage::Purse { value } => {
                    // Taken, not reconciled. A client only sees the mines in
                    // its own viewport, so its guess is always low and always
                    // getting lower; the server's number is the number. The
                    // cost is that an action sent for this tick and not yet
                    // applied shows for a moment as money still in hand, which
                    // a checkpoint interval later is right again.
                    if value != self.value {
                        log::debug!("purse: {} -> {value}", self.value);
                        self.value = value;
                    }
                }
                // A whistle that was not blown, into the lobby it was pressed
                // in. Its own message rather than `Rejected`, which closes a
                // connection: this leaves you exactly where you were, with a
                // reason to read.
                ServerMessage::NotStarted { reason } => {
                    log::info!("the match did not start: {reason}");
                    effects.push(Effect::NotStarted(reason));
                }
                // The answer to `Create`, into the form it was sent from. A
                // refusal has to land beside the fields that produced it:
                // "there is already a room called that" is a thing to correct,
                // not a thing to be told once and then hunt for.
                ServerMessage::Made(made) => match made {
                    Ok(crate::net::Made { id, name, code }) => {
                        log::info!("made \"{name}\" ({id}); joining it");
                        effects.push(Effect::Made { id, code });
                    }
                    Err(why) => {
                        log::info!("the server would not make that room: {why}");
                        effects.push(Effect::NotMade(why));
                    }
                },
                ServerMessage::Rooms { rooms } => {
                    log::debug!("the server has {} room(s)", rooms.len());
                    self.asked_at = None;
                    self.listed_at = now;
                    effects.push(Effect::Rooms(rooms));
                }
                ServerMessage::ChunkData { tick, chunk, cells } => {
                    match bytemuck::try_from_bytes::<crate::sim::Chunk>(&cells) {
                        Ok(c) => {
                            // The generation is not taken from here. A chunk
                            // reply and the step broadcast reach the socket by
                            // different routes, so a chunk can arrive from a
                            // tick either side of the one this client is on --
                            // and setting the clock from it without stepping
                            // would leave the world's state and its label
                            // disagreeing, quietly, for good. The step stream
                            // owns the clock; this only carries cells.
                            if tick != world.generation {
                                log::debug!(
                                    "chunk {chunk:?} is from tick {tick}, and this client is \
                                     on {}",
                                    world.generation
                                );
                            }
                            world.put_chunk(chunk, *c);
                        }
                        Err(e) => log::warn!("chunk {chunk:?} was the wrong size: {e}"),
                    }
                }
                ServerMessage::Step { tick, actions } => {
                    // Applied at the generation the server applied them at,
                    // then stepped to the generation it stepped to. Order and
                    // timing both matter: the step is a pure function of state
                    // and tick, so doing this a generation early or late is
                    // the same as doing something else.
                    //
                    // **Except our own, which were applied when they were
                    // made.** A `Paint` is idempotent on the generation it was
                    // meant for and not one later, so laying it twice stamps
                    // the original pattern back over where it has got to —
                    // draw a glider, watch it thicken into a blob, watch it
                    // snap back when the resync lands.
                    //
                    // **By seat, not by player**: a teammate's action carries
                    // the team's number, so skipping by player never applied
                    // it at all. And not one already applied from an `Acted`,
                    // which is a shortcut that can be dropped, so the `Step`
                    // carries everything and this is where the two meet.
                    let early = std::mem::take(&mut self.applied_early);
                    let theirs =
                        actions.iter().filter(|s| Some(s.seat) != self.me && !early.contains(s));
                    for stamped in theirs {
                        // **A teammate's comes out of the purse we share**, so
                        // it is predicted for the same reason our own spending
                        // is. Priced before it is applied, which is the
                        // contract of `value_delta`.
                        if Some(stamped.player) == self.plays_as {
                            let delta = crate::net::value_delta(world, stamped);
                            self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
                        }
                        crate::net::apply(world, stamped);
                    }
                    self.advance_to(world, tick);

                    // Every so often, ask whether we still agree. Cheap enough
                    // to do often, and the sooner a divergence is found the
                    // less of the world has been built on top of it.
                    if world.generation % CHECKPOINT_EVERY == 0 {
                        self.send_checkpoint(world);
                    }
                }
                // **An action, the moment the server took it**, rather than at
                // the next generation — 125 ms of doing nothing on a link that
                // costs four.
                //
                // Not on the tick it names, which was the wrong test and cost
                // a whole generation near a boundary: `stamped.tick` is the
                // actor's guess, and the server applies what is pending on the
                // step it happens to be on, which is the generation this
                // client is on too.
                ServerMessage::Acted(stamped) => {
                    if Some(stamped.seat) != self.me {
                        // **Priced before it is applied**, as in the `Step` arm
                        // above and for the same reason: `value_delta` reads
                        // what is on the square *now*, and the server priced it
                        // against that. Applied first, every cell compares
                        // equal to what the placement would put there --
                        // `apply_to` and `remove_from` are idempotent -- so the
                        // delta was always exactly zero and a teammate's
                        // spending never moved this client's purse until the
                        // next `Purse` corrected it.
                        if Some(stamped.player) == self.plays_as {
                            let delta = self.price(world, &stamped);
                            self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
                        }
                        crate::net::apply(world, &stamped);
                        self.applied_early.push(stamped);
                    }
                }
                ServerMessage::Resync { tick, chunks } => {
                    // One click per chunk, not per message: a resync naming
                    // forty chunks is a world being rebuilt, and one naming a
                    // single chunk is one prediction that missed. The log line
                    // says it happened; the counter says how often.
                    self.geiger.clicks(chunks.len(), now);
                    log::warn!(
                        "desynced at tick {tick}; refetching {} chunks (rate {:.1})",
                        chunks.len(),
                        self.geiger.rate()
                    );
                    // Asked for again at once rather than left to the viewport
                    // to notice: a wrong chunk off screen is still wrong, and
                    // it will be back on screen eventually.
                    for c in &chunks {
                        self.subscribed.remove(c);
                    }
                    if let Some(link) = &self.link {
                        link.send(ClientMessage::Subscribe { chunks });
                    }
                }
            }
        }

        if closed {
            self.file_game(world);
            self.link = None;
            self.asked_at = None;
            effects.push(Effect::Closed);
        }
        effects
    }

    /// Take a room's world, replacing whatever this client was holding.
    ///
    /// Only now, and not before: until a `Welcome` or a `Watching` arrives
    /// there is nothing authoritative to replace it with, and an empty screen
    /// is worse than a local game.
    ///
    /// Built to the shape the server named. A client that assumed an infinite
    /// plane against a wrapping server folded no coordinates: chunks the
    /// server calls the same one were several to the client, digests were
    /// taken against coordinates it had never heard of, and the seam showed
    /// the moment anything crossed it.
    fn enter(&mut self, world: &mut World, shape: crate::sim::WorldKind, tick: Tick, rules: Rules) {
        self.applied_early.clear();
        // **The room's rules, not this client's.** A pause and a free hand are
        // things a *world* does, so they arrive with it rather than being
        // carried over from the last one.
        self.rules = rules;
        self.file_game(world);
        *world = crate::net::sane_world(shape, self.room.as_ref().expect("the room was just set"));
        // A birth's owner is seeded from the generation, so a client
        // simulating at a different tick would make different choices from
        // identical cells.
        world.set_generation(tick);
        self.subscribed.clear();
        // A different room is a different world and a different argument about
        // it, so the counter starts over rather than showing the last room's
        // trouble against this one.
        self.geiger.reset();
    }

    /// Tell the server what this client thinks it holds, so the two can find
    /// out cheaply whether they agree.
    ///
    /// A chunk is 512 bytes and its digest is eight, so a whole world's worth
    /// of state fits in a message that costs nothing to send — which is the
    /// point: agreement can be checked constantly, and only the chunks that
    /// actually disagree are ever sent back.
    ///
    /// Stamped with the generation the digests were taken at, because a chunk
    /// compared against the wrong tick disagrees for a reason that is not a
    /// bug. The server ignores a checkpoint from any tick but its own, so one
    /// that arrives late is skipped rather than answered wrongly.
    pub fn send_checkpoint(&self, world: &World) {
        let Some(link) = &self.link else { return };
        // Only the chunks this client has actually asked for.
        //
        // `stored()` is the wrong set on a wrapping world: a torus is
        // allocated whole, so every chunk exists from the moment the world is
        // built and the client would claim to hold hundreds it has never been
        // sent. They read as empty, the server disagrees with every one of
        // them, and it answers with a `Resync` naming the lot -- every
        // checkpoint, until the whole world has been dragged across. An
        // infinite world hid this, because there `stored()` is only what has
        // been fetched or grown.
        //
        // Asked-for rather than received, because the two differ only where
        // the server had nothing to send, and a chunk it says nothing about is
        // one it agrees is empty.
        let chunks: Vec<(Coord, u64)> = world
            .stored()
            .iter()
            .filter(|(coord, _)| self.subscribed.contains(coord))
            .filter_map(|&(coord, _)| Some((coord, world.chunk_digest(coord)?)))
            .take(MAX_CHECKPOINT_CHUNKS)
            .collect();
        if chunks.is_empty() {
            return;
        }
        link.send(ClientMessage::Checkpoint { tick: world.generation, chunks });
    }

    /// Ask for any chunk in view that has not been asked for already.
    ///
    /// Written against the viewport, which the camera hands in, so panning
    /// needs no new code and this module needs no camera.
    pub fn subscribe(&mut self, world: &World, min: (i32, i32), max: (i32, i32)) {
        // Folded onto the chunks that actually exist before anything is asked
        // for. On a wrapping world the viewport runs off the edge and comes
        // back, so the same chunk is covered under several global coordinates
        // -- and a `Resync` names the folded one. Asking under the unfolded
        // name would subscribe several times to one chunk and then fail to
        // match the name the server used when it said that chunk was wrong.
        let mut wanted: Vec<_> = World::chunks_covering(min, max)
            .into_iter()
            .map(|c| world.canonical(c))
            .filter(|c| !self.subscribed.contains(c))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        if wanted.is_empty() {
            return;
        }
        self.subscribed.extend(wanted.iter().copied());
        if let Some(link) = &self.link {
            link.send(ClientMessage::Subscribe { chunks: wanted });
        }
    }

    /// How many chunks this client has been sent, for the HUD.
    ///
    /// What it asked for, not what its world has room for: a torus is
    /// allocated whole, so a stored count there is the size of the world and
    /// says nothing about what has arrived.
    pub fn chunks_held(&self) -> usize {
        self.subscribed.len()
    }

    /// Every visible chunk is stale — a resize, or a viewport that moved.
    pub fn forget_what_was_asked_for(&mut self) {
        self.subscribed.clear();
    }

    // ---- going places ------------------------------------------------------

    /// Reach a server and ask what rooms it has.
    ///
    /// Any previous socket goes first. Two links would both be draining into
    /// one client, and the second `Welcome` would arrive into a world built
    /// for the first. `false` is an address a socket cannot be built for,
    /// which in a browser is the one way dialling fails.
    pub fn connect(&mut self, link: Option<Link>, now: f64) -> bool {
        self.link = link;
        self.ask_for_rooms(now);
        self.link.is_some()
    }

    /// **Where the client was told to go**, before the first frame.
    ///
    /// A destination stated on a command line or in a link is a choice already
    /// made. `false` is an address no socket can be built for — a browser
    /// refusing to construct one, which is the one way dialling fails there —
    /// and the menu is what the client falls back to.
    ///
    /// **Timed like the menu's own ask, which it was not.** A link into a room
    /// set no deadline, so a socket that never opened produced no message, no
    /// retry and no way to tell: the client sat on the world it starts every
    /// session with, and a browser's socket object exists long before it
    /// connects, so the HUD said "connected" for the whole of it. A game that
    /// quietly turns out to be a different game is worse than one that says it
    /// failed.
    pub fn go(
        &mut self,
        link: Option<Link>,
        name: String,
        room: Option<RoomId>,
        watch: bool,
    ) -> bool {
        self.link = link;
        if self.link.is_none() {
            return false;
        }
        // A link that says watch is answered by `Watch`, which takes no name
        // and no token: there is no player to be remembered as.
        match (room, watch) {
            (Some(room), true) => self.tell(ClientMessage::Watch { room }),
            (room, _) => self.join(name, room),
        }
        self.asked_at = Some(0.0);
        true
    }

    /// Join a room, as a name.
    pub fn join(&mut self, name: String, room: Option<RoomId>) {
        crate::net::keep::remember_name(&name);
        self.joining = Some(Joining { name, room });
        self.send_pending_join();
    }

    /// Watch one, which takes no name and keeps no token: there is no player
    /// to be remembered as.
    pub fn watch(&mut self, room: RoomId) -> bool {
        let Some(link) = &self.link else { return false };
        link.send(ClientMessage::Watch { room });
        true
    }

    /// Ask for a room that is not here yet. The answer is a
    /// [`Effect::Made`], and joining it is a separate `Join`.
    pub fn create(&mut self, msg: ClientMessage) -> bool {
        let Some(link) = &self.link else { return false };
        link.send(msg);
        true
    }

    /// Send anything else this client has decided on: a whistle, a forfeit, a
    /// side taken. All of them are answered by a broadcast rather than a
    /// reply, so there is nothing here to wait on.
    pub fn tell(&mut self, msg: ClientMessage) {
        if let Some(link) = &self.link {
            link.send(msg);
        }
    }

    /// Send the waiting join, if there is one and there is a link to send it
    /// on.
    ///
    /// A client with no secret sends none, and plays as somebody this server
    /// will not remember — the honest outcome for a browser that cannot keep
    /// one, rather than a reason to refuse to let anybody play.
    pub fn send_pending_join(&mut self) {
        let Some(link) = self.link.as_ref() else { return };
        let Some(joining) = self.joining.take() else { return };
        link.send(ClientMessage::Join {
            person: crate::net::keep::secret_or_new(),
            name: joining.name,
            room: joining.room,
        });
    }

    /// Whether a join is still waiting for a link to go out on.
    pub fn join_waiting(&self) -> bool {
        self.joining.is_some()
    }

    /// Ask this server for its rooms, and start the clock on an answer.
    fn ask_for_rooms(&mut self, now: f64) {
        if let Some(link) = &self.link {
            link.send(ClientMessage::Rooms);
            self.asked_at = Some(now);
        }
    }

    /// Ask again, so the list does not go stale under the pointer. Only worth
    /// calling while the list is on screen, which the view decides.
    pub fn refresh_room_list(&mut self, now: f64, showing: bool) {
        if !showing || now - self.listed_at < ROOM_LIST_REFRESH {
            return;
        }
        self.listed_at = now;
        self.tell(ClientMessage::Rooms);
    }

    /// The same, at once, for somebody who has just made a room elsewhere and
    /// does not want to wait out the interval.
    pub fn refresh_now(&mut self, now: f64) {
        self.listed_at = now;
        self.tell(ClientMessage::Rooms);
    }

    /// Give up on a server that has not answered, and say so.
    ///
    /// A menu that says "asking" forever is indistinguishable from one that is
    /// broken, and the two most likely causes — a wrong address, and a server
    /// that is not running — both look exactly like this.
    ///
    /// **This runs for a client that was told where to go, too.** A link into
    /// a room used to set no deadline at all, so a socket that never opened
    /// left the client playing the world it starts every session with, on its
    /// own, with nothing said and the HUD reading "connected" — a browser's
    /// socket object exists long before it connects, and may never connect.
    pub fn timed_out(&mut self, now: f64) -> bool {
        let Some(asked) = self.asked_at else { return false };
        if now - asked < ROOM_LIST_TIMEOUT {
            return false;
        }
        self.asked_at = None;
        self.link = None;
        true
    }

    /// **Give the seat up**, without closing the connection.
    ///
    /// Going back used to keep it, on the reasoning that another `Join` would
    /// take its place — true of somebody who rejoins the same room, and false
    /// of everything else. The player stayed online, so the room went on
    /// counting them, and the rejoin token, which only returns you to a player
    /// who is *not* online, found them online and issued a new one. Leave and
    /// come back three times and a room with one person in it said three.
    ///
    /// The token is kept: this is the seat being vacated, not the player being
    /// forgotten.
    pub fn leave(&mut self, world: &World, now: f64) {
        self.file_game(world);
        if self.me.is_some() || self.watching {
            self.tell(ClientMessage::Leave);
        }
        self.clear_seat();
        self.ask_for_rooms(now);
    }

    /// Leave whatever server this client is on and be the authority.
    ///
    /// **Playing alone has to mean alone.** It meant the same screen with the
    /// socket still open: leaving gave up the seat but kept the link, so a
    /// `Welcome` replaced the world with the server's, the HUD said connected,
    /// and the board did not move — the world advances on this client's clock
    /// only when there is no link. A frozen board in a world you cannot build
    /// in is not a game.
    pub fn play_alone(&mut self, world: &World, victory: Option<Victory>) {
        self.file_game(world);
        // Dropping it closes it — see the `Drop` on the browser's `Link`, and
        // the socket thread that ends with its channel on native.
        self.link = None;
        // Or the room list this client is no longer waiting for times out and
        // drags it back to the menu, four seconds into a game of one, to say
        // that a server it has stopped talking to did not answer.
        self.asked_at = None;
        self.clear_seat();
        // A different world is a different argument about it.
        self.geiger.reset();
        // What the server had issued belonged to the seat that was given up.
        self.value = Player::STARTING_VALUE;
        // A solitary world is a plain one. A laboratory is a room now, so
        // there is nothing here to switch off and nothing for it to mean.
        self.rules = Rules::default();
        self.room_name = Some(crate::net::SOLO_ROOM.to_string());
        // A lobby of one, so the clock along the top has something to read.
        // `None` for a sandbox, which is what playing alone has always been.
        self.lobby = victory.map(|victory| crate::net::Lobby {
            phase: crate::net::MatchPhase::Running { from: 0 },
            victory: Some(victory),
            players: vec![crate::net::Seat { id: PlayerId(1), name: String::new(), who: None }],
            teams: Vec::new(),
            owner: None,
            started_by: None,
            code: None,
        });
    }

    /// Forget the key, and with it every seat it could have come back to.
    ///
    /// **Everything, and it cannot be undone.** A key nobody else holds is a
    /// key nobody can give back, which is why the menu asks twice.
    pub fn forget_everything(&mut self) {
        log::warn!("forgetting this client's key, record and settings");
        crate::net::keep::forget_everything();
        self.link = None;
        self.joining = None;
        self.clear_seat();
    }

    /// The seat, and everything that only meant anything while sitting in it.
    /// `room_name` is left for whoever is setting one.
    fn clear_seat(&mut self) {
        self.me = None;
        self.plays_as = None;
        self.room = None;
        self.room_name = None;
        self.lobby = None;
        self.watching = false;
        self.forfeited = false;
        self.standing.clear();
        self.subscribed.clear();
    }

    /// File the game in play, if there is one, and forget it.
    ///
    /// Called wherever a room ends for this client. Idempotent, so the paths
    /// that overlap — a link closing on the way back to the menu — file once.
    pub fn file_game(&mut self, world: &World) {
        let Some(mut live) = self.in_play.take() else { return };
        live.at(world.generation);
        let game = live.finish();
        log::info!(
            "filing \"{}\": {} generations, {} squares at its largest",
            game.room,
            game.generations,
            game.best
        );
        crate::client::record::remember(&game);
    }
}

/// A world's shape, for a log line.
fn describe(kind: crate::sim::WorldKind) -> String {
    match kind {
        crate::sim::WorldKind::Infinite => "boundless".to_string(),
        crate::sim::WorldKind::Toroidal { rows, cols } => format!("{rows}x{cols} wrapping"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Only the next generation is a step; everything else is a loss.**
    ///
    /// This used to step forward to close a gap of up to thirty-two, which is
    /// the branching bug rather than the recovery from it: those generations
    /// carried actions, stepping runs them empty, and the world that comes out
    /// is one nobody else has. There is no arithmetic that recovers a message
    /// nobody kept.
    #[test]
    fn a_step_is_the_next_generation_and_nothing_else_is() {
        assert_eq!(advance(0, 1), Advance::Step);
        assert_eq!(advance(400, 401), Advance::Step);

        // Behind, by one message and by many. Both used to be caught up
        // locally and both are worlds this client would be inventing.
        assert_eq!(advance(400, 402), Advance::Lost, "one dropped step is still a loss");
        assert_eq!(advance(400, 432), Advance::Lost);
        assert_eq!(advance(400, 100_000), Advance::Lost);

        // Ahead, or the same tick twice: a websocket does not reorder, so
        // either means something upstream is not what it is thought to be, and
        // the answer is the same one.
        assert_eq!(advance(400, 400), Advance::Lost);
        assert_eq!(advance(400, 399), Advance::Lost);
        assert_eq!(advance(400, 0), Advance::Lost);
    }

    /// **A session with no link is testable, and none of this was.**
    ///
    /// It lived on a struct that also held the GPU pipeline and the sprite
    /// atlas, so reaching any of it meant a window.
    ///
    /// The clock is a **laboratory's**, not an offline client's: a world runs
    /// and a solitary world is still a world, so a plain solo game has nothing
    /// to press.
    #[test]
    fn a_solitary_session_is_the_authority_and_keeps_its_own_time() {
        let mut world = World::infinite();
        crate::net::grant(&mut world, PlayerId(1));
        let mut s = Session::new();
        assert!(!s.connected());
        assert!(!s.own_clock(), "a plain world runs, alone or not");

        s.set_rules(Rules { laboratory: true, ..s.rules });
        assert!(s.own_clock(), "and a laboratory's is yours");

        let at = world.generation;
        s.set_rules(Rules { paused: true, ..s.rules });
        s.advance_alone(&mut world, 10.0, 0.25);
        assert_eq!(world.generation, at, "stopped means stopped");

        s.ask_for_one_step();
        s.advance_alone(&mut world, 0.0, 0.25);
        assert_eq!(world.generation, at + 1, "and one means one");
        assert!(s.rules.paused, "and it stays stopped");
    }

    /// The purse is predicted with the arithmetic the server charges by, so
    /// what is on screen is what the server will agree with — and floored and
    /// capped the same way, since a player who cannot stop owing is a player
    /// who cannot act.
    #[test]
    fn a_purse_is_floored_and_capped_where_the_server_floors_and_caps_it() {
        let mut s = Session::new();
        s.value = 5;
        s.spend(-100);
        assert_eq!(s.value, 0, "nobody goes into debt");
        s.spend(Player::MAX_VALUE);
        assert_eq!(s.value, Player::MAX_VALUE, "and nobody hoards past the ceiling");
    }

    /// A spectator has no seat, so nothing it does can be attributed to
    /// anybody — refused here as well as on the server, so that clicking says
    /// why rather than doing nothing.
    #[test]
    fn a_watcher_may_not_act_and_a_gathering_match_takes_nothing() {
        let mut s = Session::new();
        assert!(s.may_act(), "an ordinary room takes what you do");

        s.watching = true;
        assert!(!s.may_act(), "watching is not playing");

        s.watching = false;
        s.lobby = Some(crate::net::Lobby {
            phase: crate::net::MatchPhase::Gathering,
            victory: None,
            players: Vec::new(),
            teams: Vec::new(),
            owner: None,
            started_by: None,
            code: None,
        });
        assert!(!s.may_act(), "nothing happens before the whistle");
        assert!(s.gathering());
    }
}
