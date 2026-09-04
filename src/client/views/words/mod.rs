//! Every string the client puts on screen, and the shape that lets there be
//! more than one language of them.
//!
//! **A struct rather than a tree of constants.** What the game says is a
//! decision, and decisions are easier to get right side by side than scattered
//! through the code that draws them — which is why this was one file of `const`
//! to begin with. What a file of constants cannot be is *chosen*: a second
//! language has to be a second set of the same words, and a `const` is one set
//! by construction.
//!
//! So the words are fields on [`Words`], the values live one per language in a
//! module of their own — [`eng`] is the first — and [`w`] hands back whichever
//! is current. Nothing is copied: a language is a `static` built at compile
//! time out of string literals, so reading one is a pointer and choosing one is
//! a pointer swap.
//!
//! Log lines are not here. Those are for whoever is running it, and belong
//! where the thing they describe happens.
//!
//! ## The formatters are still English
//!
//! Fifty-three of these are not constants but small functions — plurals,
//! counts, "3 of 7 matches won". They live in [`eng`] beside its words and are
//! re-exported here, so a call site reads the same as it always did.
//!
//! That is honest for one language and is **not** a translation of them: word
//! order, plural rules and grammatical number all differ, and a second language
//! wants its own implementations rather than the same format string with
//! different nouns. The shape that fits is a trait with a method per formatter,
//! and the moment there is a second language that is the change to make. Doing
//! it now would be fifty-three trait methods with one implementor, which is a
//! cost with nothing on the other side of it.

pub mod eng;

// The formatters, from whichever language is compiled in. See the note above:
// this is a re-export today and wants to be a trait the day there are two.
pub use eng::{
    clock,
    // And the handful that sit at the top rather than under a screen.
    describe,
    desync,
    help,
    hud,
    lobby,
    menu,
    phase,
    profile,
    provisional,
    rating,
    record,
    refused,
    room_kind,
    stamps,
};

/// Which language is being spoken.
///
/// Set once, if at all. Unset is English, which is also what a client that
/// never asks gets — so nothing has to be initialised for the game to have
/// words.
static CHOSEN: std::sync::OnceLock<&'static Words> = std::sync::OnceLock::new();

/// The words, in whichever language this client is speaking.
///
/// Short on purpose: it is read a few hundred times across the views, and a
/// longer name would be the most repeated thing in the interface.
pub fn w() -> &'static Words {
    CHOSEN.get().copied().unwrap_or(&eng::WORDS)
}

/// Speak this language from now on. The first call wins and later ones are
/// ignored rather than racing, because a screen half-drawn in two languages is
/// worse than one drawn in the wrong one.
pub fn speak(words: &'static Words) {
    let _ = CHOSEN.set(words);
}

/// The home screen: who you are, what you have done, and the way in.
/// Who else plays here, and the leaderboard, which is the same list with
/// nothing typed into the box.
pub struct MenuPeople {
    pub title: &'static str,
    pub note: &'static str,
    pub hint: &'static str,
    pub asking: &'static str,
    pub nobody: &'static str,
    /// Not the same sentence as [`NOBODY`]. An empty board means the
    /// server has met nobody it is sure about yet, which is a fact about
    /// the server rather than about what was typed.
    pub nobody_yet: &'static str,
}

/// You, as a page: the name, the rating, the record and the key.
pub struct MenuAccount {
    pub title: &'static str,
    pub rated: &'static str,
    /// Before a first join. **Not an error and not a blank**: a face comes
    /// off the key a server issues, so there is nothing to draw one from
    /// until you have played somewhere.
    pub unnamed: &'static str,
}

/// What somebody who has just arrived cannot work out by clicking. Every
/// entry is a rule people lose to before they learn it, in the order they
/// bite, and the argument for each is in docs/game.md.
/// The practice patches on the how-to page.
/// **Placeholder copy.** Everything below marked lorem is a stand-in for
/// words somebody will write; the patches themselves are real and run the
/// game's own rule. Replacing a string here changes what a patch says and
/// nothing about what it does.
pub struct MenuTutorial {
    pub run: &'static str,
    pub stop: &'static str,
    pub step: &'static str,
    /// Fills the outline in, for somebody who would rather watch than trace.
    pub show_me: &'static str,
    pub clear: &'static str,
    /// The heading and body above each patch, in the order the page draws
    /// them. Lorem for now — see the note on this module.
    pub lessons: &'static [(&'static str, &'static str)],
}

pub struct MenuHowto {
    pub title: &'static str,
    pub note: &'static str,
    pub rules: &'static [(&'static str, &'static str)],
    pub tip_title: &'static str,
    pub tip: &'static str,
}

/// **A word about Conway, at the end.**
/// The rule this game is built on is his, and he did not want to be
/// remembered for it — he was open about finding it a nuisance. It was a
/// Sunday afternoon with counters on a Go board in 1970, it went round the
/// world through Martin Gardner's column, and it stood in front of
/// everything else he did for fifty years.
/// So this says what he would rather you looked up, briefly and without
/// ceremony. Somebody who read to the bottom of a page about the Game of
/// Life is exactly the person who should be told there is far more.
pub struct MenuConway {
    pub title: &'static str,
    pub body: &'static str,
    /// Name, what it is, and where. Links rather than explanations: the
    /// point is to say there is more and where it is, not to teach it.
    pub work: &'static [(&'static str, &'static str, &'static str)],
}

pub struct MenuHomeSettings {
    pub key: &'static str,
    /// Said plainly, because it is not the bargain people expect from
    /// something called a key. There is no account behind it, no
    /// address to send a reset to, and it is the same you on every
    /// server rather than one of them.
    pub key_note: &'static str,
    pub key_take: &'static str,
    /// A key is made at startup, so this is the store refusing it or
    /// there being no entropy to make one from — and the consequence
    /// is worth stating rather than the absence.
    /// A server issues the name it calls you, so there is nothing to
    /// show until one has.
    pub key_unseen: &'static str,
    pub key_none: &'static str,
    pub forget: &'static str,
    pub forget_note: &'static str,
    pub confirm: &'static str,
    pub cancel: &'static str,
    pub forget_ask: &'static str,
    pub forget_ask_note: &'static str,
    pub key_ask: &'static str,
    pub key_ask_note: &'static str,
}

pub struct MenuHome {
    pub play: &'static str,
    pub who: &'static str,
    pub record: &'static str,
    pub profile: &'static str,
    pub people: &'static str,
    pub account: &'static str,
    pub howto: &'static str,
    pub settings_label: &'static str,
    pub settings_hide: &'static str,
    /// A first visit has nothing to show, and five zeroes would say only
    /// that the game keeps score.
    pub nothing_yet: &'static str,
    pub settings: MenuHomeSettings,
}

/// Reaching a room that is not in the listing.
pub struct MenuCode {
    pub label: &'static str,
    pub hint: &'static str,
    pub go: &'static str,
    /// What the server hands back after making a private room. The thing
    /// you send somebody, so it is worth saying that out loud.
    pub made: &'static str,
}

/// Watching without a seat.
pub struct MenuWatch {
    pub watch: &'static str,
    pub join: &'static str,
    /// Blowing the whistle, in the lobby, for whoever made the match.
    pub start: &'static str,
    pub start_note: &'static str,
    pub not_yours: &'static str,
    pub at_console: &'static str,
    /// Said on the HUD for the whole visit, because a spectator whose
    /// clicks do nothing needs to know why the first time rather than the
    /// fifth.
    pub watching: &'static str,
    pub no_seat: &'static str,
}

/// Making a room. One label per decision, and a label appears only when
/// the decision it belongs to is live — see [inspiration.md].
/// [inspiration.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/inspiration.md#the-menu
pub struct MenuMake {
    /// The action, when there is no server to ask. The same form, so the
    /// same questions; a different place for the answer to go.
    pub alone: &'static str,
    /// Opens the form. Says "world" rather than "room" because that is
    /// what you get and what the game calls it everywhere else; "room" is
    /// the machinery's word.
    pub open: &'static str,
    pub title: &'static str,
    pub name: &'static str,
    pub name_hint: &'static str,
    pub shape: &'static str,
    pub boundless: &'static str,
    pub wrapping: &'static str,
    pub size: &'static str,
    /// Two fields, because a size is two numbers. Naming them separately
    /// is also what lets an error say which one is wrong.
    pub rows: &'static str,
    pub cols: &'static str,
    /// Chunks, not cells. Said out loud because the number is small and
    /// would otherwise read as a tiny world.
    /// Between the two numbers, so a size reads as `12x12` — which is how
    /// a size is written and what `--torus` takes. Two boxes labelled
    /// "Rows" and "Columns" said, over two lines, what one character says.
    pub by: &'static str,
    /// On hover rather than on a line of its own: worth knowing once, and
    /// worth no space after that.
    pub size_note: &'static str,
    pub together: &'static str,
    pub solo: &'static str,
    pub teams: &'static str,
    pub sides: &'static str,
    /// Teams are picked in the lobby, not here — said out loud, because a
    /// form that asks how many and never asks who reads as unfinished.
    pub sides_note: &'static str,
    pub private: &'static str,
    pub listed: &'static str,
    pub unlisted: &'static str,
    /// The third answer to who can find it, and the one that used to be a
    /// page of its own: nobody, and no server.
    pub solo_access: &'static str,
    pub solo_note: &'static str,
    pub listed_note: &'static str,
    /// The name field is ignored for a private room, and a field being
    /// quietly discarded is worse than one that is not there.
    pub unlisted_note: &'static str,
    /// **The first question, because it decides the rest.** It used to
    /// be implied by "ends: never", which told a world and a laboratory
    /// apart not at all — the laboratory was not a room.
    pub kind: &'static str,
    pub world: &'static str,
    pub r#match: &'static str,
    pub experiment: &'static str,
    pub world_note: &'static str,
    pub match_note: &'static str,
    /// Says what it is *for*. The switches themselves are in the room, on
    /// the bar, because they are things this world does rather than things
    /// it was made with.
    pub experiment_note: &'static str,
    pub ends: &'static str,
    pub timer: &'static str,
    pub territory: &'static str,
    pub timer_note: &'static str,
    pub territory_note: &'static str,
    pub generations: &'static str,
    pub squares: &'static str,
    pub make: &'static str,
    /// A world is made **on** a server, so there has to be one. Said at
    /// the point of pressing rather than by the form being absent.
    pub no_server: &'static str,
    pub clear: &'static str,
    pub making: &'static str,
    /// A match does not start on its own, so somebody about to make one
    /// should know that before they make it rather than after.
    pub match_waits: &'static str,
}

/// The screen before the game.
pub struct Menu {
    pub title: &'static str,
    pub name: &'static str,
    pub name_hint: &'static str,
    pub server: &'static str,
    pub server_hint: &'static str,
    pub asking: &'static str,
    /// A server that answered, said once and quietly. The room list below it
    /// is the real answer; this is only here so that the moment of connecting
    /// is not silent.
    pub reached: &'static str,
    pub retry: &'static str,
    /// Reaching a server and asking it again are the same act from where the
    /// player stands, so they are one control whose meaning follows the state
    /// — which the hover text says.
    ///
    /// **Drawn rather than written**: it was `\u{21bb}` and rendered as a box,
    /// because no font is loaded anywhere in this client. See
    /// [`crate::client::views::icons::refresh`], and the same for the back
    /// arrow. A control that is one symbol has nothing left when the symbol is
    /// missing.
    pub refresh_ask: &'static str,
    pub refresh_again: &'static str,
    /// The column of what is already here. "Worlds" rather than "Rooms",
    /// which is the machinery's word — a player joins a world.
    pub rooms: &'static str,
    /// An empty list is an invitation, not a failure: there is a form in the
    /// next column and this is the moment to point at it.
    pub no_rooms: &'static str,
    /// Waiting is a different thing from a server with nothing on it, and
    /// reads differently: one is a pause, the other is an invitation.
    pub not_asked: &'static str,
    /// Out of a screen, by pointer. Escape does the same, and both exist
    /// because a phone has no escape key and a keyboard user should not have
    /// to reach for the mouse.
    pub back: &'static str,
    /// What the same button says when you are already enrolled in a match.
    /// Starting a solitary game is never what pressing the only other button
    /// meant, so the press means the opposite instead.
    pub back_to_match: &'static str,
    pub back_to_match_note: &'static str,
    pub empty_room: &'static str,
    pub lost_connection: &'static str,
    /// The home screen: who you are, what you have done, and the way in.
    /// Who else plays here, and the leaderboard, which is the same list with
    /// nothing typed into the box.
    pub people: MenuPeople,
    /// You, as a page: the name, the rating, the record and the key.
    pub account: MenuAccount,
    /// What somebody who has just arrived cannot work out by clicking. Every
    /// entry is a rule people lose to before they learn it, in the order they
    /// bite, and the argument for each is in docs/game.md.
    /// The practice patches on the how-to page.
    ///
    /// **Placeholder copy.** Everything below marked lorem is a stand-in for
    /// words somebody will write; the patches themselves are real and run the
    /// game's own rule. Replacing a string here changes what a patch says and
    /// nothing about what it does.
    pub tutorial: MenuTutorial,
    pub howto: MenuHowto,
    /// **A word about Conway, at the end.**
    ///
    /// The rule this game is built on is his, and he did not want to be
    /// remembered for it — he was open about finding it a nuisance. It was a
    /// Sunday afternoon with counters on a Go board in 1970, it went round the
    /// world through Martin Gardner's column, and it stood in front of
    /// everything else he did for fifty years.
    ///
    /// So this says what he would rather you looked up, briefly and without
    /// ceremony. Somebody who read to the bottom of a page about the Game of
    /// Life is exactly the person who should be told there is far more.
    pub conway: MenuConway,
    pub home: MenuHome,
    /// Reaching a room that is not in the listing.
    pub code: MenuCode,
    /// Watching without a seat.
    pub watch: MenuWatch,
    /// Making a room. One label per decision, and a label appears only when
    /// the decision it belongs to is live — see [inspiration.md].
    ///
    /// [inspiration.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/inspiration.md#the-menu
    pub make: MenuMake,
}

/// The bar along the bottom.
pub struct Hotbar {
    /// The keys these two squares teach, which live with the rest of the key
    /// labels — `help::keys` is where a key's name is decided, and a second
    /// spelling here would be a square and a key list disagreeing.
    pub run_key: &'static str,
    pub step_key: &'static str,
    /// The four figures on the bar. One word each, lower case: they label a
    /// number rather than heading a section, and a capital would make each of
    /// them look like the start of something.
    pub purse: &'static str,
    /// What you hold, which is territory. "held" was the field's own name and
    /// said nothing about what was being held.
    pub ground: &'static str,
    pub tick: &'static str,
    pub rating: &'static str,
    pub life: &'static str,
    pub factory: &'static str,
    pub turret: &'static str,
    pub ice: &'static str,
    pub dynamite: &'static str,
    /// The square that takes a stamp. Short, because it sits in a 44px box.
    /// The shape axis. Verbs, because they are how the cells get chosen
    /// rather than what ends up in them.
    pub draw: &'static str,
    pub pane: &'static str,
    /// What the shape square says while a stamp is held: the axis is the same
    /// one, so it shows what is on it rather than going blank.
    pub pattern: &'static str,
    /// The key that puts the shape back to whatever the held material is
    /// usually wanted in, shown on the square rather than in a help screen —
    /// it is the one key on the bar that does something rather than selecting
    /// something.
    ///
    /// The clock section. Words rather than glyphs, because the sheet has no
    /// art for any of them and a triangle painted by hand is a decision about
    /// a whole icon set — see `icons::back`, which is the precedent and the
    /// argument for eventually doing it.
    pub run_hint: &'static str,
    pub stop_hint: &'static str,
    pub step_hint: &'static str,
    /// The panel's own heading. The square that opens it is an icon.
    pub rules: &'static str,
    /// Said as what it does to the world rather than as "reset", which reads
    /// like putting something back the way it was.
    pub wipe_hint: &'static str,
    pub rules_hint: &'static str,
    pub anywhere: &'static str,
    pub anywhere_note: &'static str,
    pub free: &'static str,
    pub free_note: &'static str,
    pub capture: &'static str,
    /// The square that opens the library.
    pub library: &'static str,
    /// The character, not the chord: it is bound by what it types, so the
    /// label is right on every layout.
    pub help: &'static str,
    pub help_hint: &'static str,
}

/// The library of captured patterns.
pub struct Stamps {
    pub title: &'static str,
    /// Turning is a thing you do to a pattern, so with none held the key
    /// changes nothing on the screen — which looks like a key that does not
    /// work rather than one that had nothing to act on.
    pub nothing_to_turn: &'static str,
    pub forget: &'static str,
    pub none_yet: &'static str,
    pub how: &'static str,
    pub draw: &'static str,
    pub keep: &'static str,
    pub clear: &'static str,
    /// The library survives a session, so a stamp is worth naming.
    pub keep_name: &'static str,
    pub rename_hint: &'static str,
    pub edit: &'static str,
    pub edit_hint: &'static str,
    pub on_bar: &'static str,
    pub on_bar_hint: &'static str,
    pub bar_full: &'static str,
    /// Editing one rather than drawing a new one, so `keep` means replace.
    pub editing: &'static str,
    pub draw_how: &'static str,
    pub nothing_to_capture: &'static str,
    pub gone: &'static str,
}

/// How much of a match is left.
pub struct Clock {}

/// What a server says about somebody.
pub struct Profile {
    pub title: &'static str,
    /// Asked for, and not answered yet. Its own line rather than an empty
    /// panel, because a wait and a blank look the same and only one of them is
    /// worth waiting through.
    pub asking: &'static str,
    /// A real answer, and it says which kind of nothing it is: this server has
    /// never met them, as against not having replied.
    pub unknown: &'static str,
    pub you: &'static str,
    /// **Your own diary**, against the server's count above it. The two
    /// headings are what make the two numbers readable rather than a
    /// contradiction.
    pub everywhere: &'static str,
    /// Not "unrated", which reads as a judgement. No server has met you, so
    /// there is nobody to have an opinion.
    pub unrated: &'static str,
    /// A client with no key, which is a browser that cannot keep one.
    pub nobody: &'static str,
    /// **On the panel, once.** A server can only speak for what happened on
    /// it, and a screen that did not say so would read as a record of a person
    /// rather than of a visit.
    pub here: &'static str,
}

/// The screen before a match starts.
pub struct Lobby {
    /// **"Team", not "side".** They were the same word doing one job, and the
    /// game says team everywhere a player reads it.
    pub take_side: &'static str,
    pub leave_side: &'static str,
    pub code: &'static str,
    pub rename: &'static str,
    pub keep_name: &'static str,
    pub nobody_on_it: &'static str,
    pub waiting: &'static str,
    pub finished: &'static str,
    pub nobody: &'static str,
    pub you: &'static str,
    pub you_won: &'static str,
    pub how: &'static str,
}

/// The panel in the corner.
pub struct Hud {
    pub connected: &'static str,
    pub offline: &'static str,
    /// Giving up, which is not the same as leaving: the back arrow beside this
    /// walks out of the room and gives up the seat, and somebody losing a
    /// match should be able to concede it rather than vanish from it.
    pub forfeit: &'static str,
    pub forfeit_hint: &'static str,
    pub gave_up: &'static str,
    /// Only for whoever started it, which is the same person and the same
    /// reasoning as the whistle.
    pub end_match: &'static str,
    pub end_match_hint: &'static str,
    pub holding: &'static str,
    /// The arrow out. A glyph rather than the word, because it sits beside a
    /// player's name in a row that is already full.
    /// **Drawn rather than written.** This was the arrow itself and came out
    /// as a box: no font is loaded anywhere in this client, so a glyph outside
    /// what egui bundles is tofu — and the one control whose whole job is to
    /// be recognised at a glance was a square. See
    /// [`crate::client::views::icons::back`]. Kept as a constant because the
    /// help screen still spells it in a line of text, where it is surrounded
    /// by words and reads.
    pub back: &'static str,
    pub back_hint: &'static str,
    pub boundless: &'static str,
    pub over_panel: &'static str,
    pub on_world: &'static str,
    pub nothing_yet: &'static str,
    /// The hint lines, in the order they are shown.
    ///
    /// A list rather than a run of `ui.small` calls, so what the game claims
    /// you can do is one thing to read and one thing to keep true.
    pub hints: &'static [&'static str],
}

/// The disagreement counter, which is quiet almost all of the time.
pub struct Desync {
    /// The connection has slipped before and is settled now. Worth saying,
    /// because a rate back at nought and a link that has never slipped look
    /// identical and are not the same thing.
    pub settled: &'static str,
    /// Ticking over. Prediction costs this and always has.
    pub background: &'static str,
    pub noticeable: &'static str,
    pub alarming: &'static str,
}

/// The keycaps themselves. Spelled the way a keyboard is read rather than
/// the way winit names them — nobody has a key called `ArrowLeft`.
pub struct HelpKeys {
    /// What to say before anybody has pressed one of them and there is
    /// nothing to report: the arrows do the same job and are the same
    /// everywhere, so they are the honest half of the answer.
    pub pan_arrows: &'static str,
    pub pan_fast: &'static str,
    /// **Middle drag only.** Space held used to do this too, which is
    /// the convention in a drawing tool and is the weaker claim on the
    /// key: panning also has the walk cluster and the arrows, and a pause
    /// has nowhere else obvious to live. The cost is real and worth
    /// knowing — a trackpad with no middle button has no drag-to-pan.
    pub pan_drag: &'static str,
    pub zoom: &'static str,
    /// **Only reached if a key has no label at all**, which since the
    /// digit row is seeded means never. Kept, and said without a count,
    /// because the shifted row grows when a square is added to the bar and
    /// a number here would go stale the way `shift + 1-4` did — it named
    /// four keys for a row that had run to six.
    pub tools: &'static str,
    pub stamps: &'static str,
    /// The unshifted half of the key `~` is on, so it is one press — and
    /// `~` is a dead key on the Spanish, Portuguese and Nordic layouts,
    /// which produces no text at all and left the shape reset unreachable
    /// there. `~` still works; this is what the square says.
    pub turn: &'static str,
    pub mirror: &'static str,
    pub drag: &'static str,
    /// Return, and a full stop. Golly's, because somebody who wants a
    /// pause button has almost certainly used Golly.
    pub play: &'static str,
    pub step_one: &'static str,
    pub walk: &'static str,
    pub choose: &'static str,
    pub move_on: &'static str,
    pub back: &'static str,
    pub help: &'static str,
}

pub struct Help {
    pub title: &'static str,
    pub close: &'static str,
    pub dismiss: &'static str,
    pub looking: &'static str,
    pub building: &'static str,
    pub getting_about: &'static str,
    pub pan: &'static str,
    pub pan_faster: &'static str,
    pub pan_by_hand: &'static str,
    pub zoom: &'static str,
    pub tools: &'static str,
    /// The shape axis has one key and it goes to the default; the other shape
    /// is a click away on the bar. See `hotbar::Held::defaulted`.
    /// **So a glider is one stamp and not four.** Turning is held rather than
    /// saved, so it changes nothing in the library.
    pub turn: &'static str,
    pub mirror: &'static str,
    pub stamps: &'static str,
    pub drag: &'static str,
    /// **The clock, and it is yours only when you are alone.** Connected, a
    /// generation happens when the server says one did — see
    /// `networking.md` — so these say what they did rather than doing
    /// nothing, which is the difference between a rule and a broken key.
    pub the_clock: &'static str,
    pub play: &'static str,
    pub step_one: &'static str,
    /// Said when either is pressed in a game — which is every room but a
    /// laboratory, where the clock belongs to whoever is in it.
    pub server_keeps_time: &'static str,
    pub go_back: &'static str,
    pub paused: &'static str,
    pub wiped: &'static str,
    pub running: &'static str,
    pub walk: &'static str,
    pub choose: &'static str,
    pub move_on: &'static str,
    /// One key, one meaning: back out of the innermost thing. It was listed
    /// twice — once for abandoning a drawing and once for leaving a screen —
    /// which is two answers to one question. It is a ladder, and saying so is
    /// shorter than saying it twice.
    pub back: &'static str,
    pub help: &'static str,
    /// The keycaps themselves. Spelled the way a keyboard is read rather than
    /// the way winit names them — nobody has a key called `ArrowLeft`.
    pub keys: HelpKeys,
}

/// The record on the home screen.
pub struct Record {
    pub nothing_yet: &'static str,
    pub largest: &'static str,
    pub form: &'static str,
    pub worlds: &'static str,
    pub matches_won: &'static str,
    pub largest_ever: &'static str,
    pub generations: &'static str,
    pub won: &'static str,
    pub lost: &'static str,
    pub no_result: &'static str,
}

/// What the world says back when it refuses something.
pub struct Refused {}

pub struct Words {
    /// **The way out of a panel**, and there is one of them.
    ///
    /// Three modules each declared this, which is three places for one control to
    /// come to be called three things. Every panel is drawn by
    /// [`super::panel`] now, so there is one button as well as one word.
    pub close: &'static str,
    /// A laboratory whose clock is stopped, which is the one thing about one that
    /// is worth knowing before you go in.
    pub stopped: &'static str,
    /// Every word the client puts on screen.
    ///
    /// One file, for the same reason `sim::rule` holds every number: a string a
    /// player reads is a decision, and decisions are easier to get right when they
    /// are next to each other than when they are scattered through the code that
    /// happens to draw them. Changing what the game *says* should not mean reading
    /// what it *does*.
    ///
    /// It is also where a translation would start, and where anybody can see the
    /// whole voice of the thing at once — which is the only way to notice that one
    /// screen says "server" and another says "host".
    ///
    /// Not log lines. Those are for whoever is running it and are written where
    /// the thing they describe happens.
    /// The screen before the game.
    pub menu: Menu,
    /// The bar along the bottom.
    pub hotbar: Hotbar,
    /// The library of captured patterns.
    pub stamps: Stamps,
    /// How much of a match is left.
    pub clock: Clock,
    /// What a server says about somebody.
    pub profile: Profile,
    /// The screen before a match starts.
    pub lobby: Lobby,
    /// The panel in the corner.
    pub hud: Hud,
    /// The disagreement counter, which is quiet almost all of the time.
    pub desync: Desync,
    pub help: Help,
    /// The record on the home screen.
    pub record: Record,
    /// What the world says back when it refuses something.
    pub refused: Refused,
}

#[cfg(test)]
mod tests {
    use super::w;

    /// **No paragraph carries the gap its source was wrapped at.**
    ///
    /// A long literal written across several lines keeps every line's
    /// indentation, so each sentence arrives with a run of spaces in the middle
    /// of it — a gap every seventy characters or so, which made the how-to page
    /// look shattered. Nothing reads a string and everything renders one, so it
    /// went unnoticed in eight of them.
    ///
    /// Only the prose is checked. A run of spaces is deliberate elsewhere: a
    /// middot separator is set with one either side, which is what typography
    /// asks for and is why this is not a rule about the whole file.
    #[test]
    fn no_paragraph_carries_the_gap_it_was_wrapped_at() {
        let words = w();
        let mut prose: Vec<&str> = vec![
            words.menu.howto.note,
            words.menu.howto.tip,
            words.menu.howto.tip_title,
            words.menu.conway.title,
            words.menu.conway.body,
        ];
        for (heading, body) in words.menu.howto.rules {
            prose.push(heading);
            prose.push(body);
        }
        for (name, what, url) in words.menu.conway.work {
            prose.push(name);
            prose.push(what);
            prose.push(url);
        }
        for line in prose {
            assert!(!line.contains("  "), "a run of spaces in {line:?}");
            assert!(!line.contains('\n'), "a newline in {line:?}");
        }
    }

    /// **Every language is the same set of words**, which is what the struct is
    /// for: a language that forgot one would not compile, and one that invented
    /// an extra would not either. With a single language that is a tautology —
    /// it is here so it is already written when there are two.
    #[test]
    fn a_language_is_a_whole_set() {
        let _: &'static super::Words = &super::eng::WORDS;
        assert_eq!(w().menu.howto.rules.len(), 5, "the how-to page lost a rule");
    }
}
