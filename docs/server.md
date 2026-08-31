# Server

Holds the worlds, owns the tick, and assigns player numbers. Links `sim` and `net` and nothing else.

```
cargo run --no-default-features --features server --bin server -- --serve .
```

`Server::handle` takes a decoded `ClientMessage` and returns the replies, so whatever carries the bytes is somebody else's problem.

## What is served

`--serve DIR` publishes the browser client, and it publishes **three things and nothing else**: `index.html`, `pkg/` and `assets/`.

That is an allowlist and it has to be. The documentation tells people to run `--serve .`, and `.` is the repository — so serving the directory wholesale published `src/`, `Cargo.toml` and, worse, `.git/`, which carries every version of everything ever committed. A denylist of `/src` and a handful of other names would be whack-a-mole against a directory the server does not control; naming what the client needs leaves nothing else to reach.

Each of the client's own screens is answered with the page, so a refresh on `/play` or `/room/arena` comes back with the client rather than a 404 — see [game.md](game.md#where-you-are-in-the-address-bar). Those paths are listed by name for the same reason: an unknown path is a **404 and not a copy of the page**, so a mistyped address says so instead of silently opening the game.

## Rooms

One `Server` is one **room**: one world, one player table, one tick. A process runs several side by side in a `server::rooms::Rooms`, so "the server" in the sense of the address you connect to is the `Rooms`, and a `Server` is one of the worlds behind it.

A room is a separate world rather than a view of a shared one, which is the simpler of the two things it could have meant and what "rooms" usually means. Nothing in `sim` had to learn that a world might be one of many, because a world never is. What it costs is that **territory, value, player numbers and the rejoin token are all per room**, so a player in two rooms is two players and their number in one says nothing about the other. `rooms::Seat` is the pair that does identify somebody: which room, and who they are in it.

**Rooms are declared, not conjured.** Joining a name nobody declared is refused, and the refusal names the rooms that do exist:

```
REFUSED — no room "loby" here; this server has arena, lobby
```

The alternative — creating a room for whoever asks — turns a typo into a world: `loby` gets you an empty plane where you own nothing, know nobody, and cannot tell that you are the only one who will ever be there.

The menu asks `ClientMessage::Rooms` and shows what comes back, so a name is normally clicked rather than typed. The rejection still carries the names, because a name can still arrive typed — `--room` on a command line, `?room=` in a link — and a client refused that way falls back to the menu with **both** the reason and the live list on screen. The two are halves of one answer, and showing only the list reads as the click having done nothing.

`Join` carries `room: Option<RoomName>`; `None` takes the server's default, so a client with nothing to say about rooms still lands somewhere. `Welcome` names the room back, because the client may have asked for none and because the token it is about to keep is filed under that name.

A room name is lowercase letters, digits, `-` and `_`, at most 24 characters, and is folded to lowercase. Narrow because the name is also the save file's name: a path separator in it would escape the rooms directory, and on a case-insensitive filesystem `Lobby` and `lobby` would be two rooms on one machine and one on another. `net::room_name` is the whole rule, and the client checks `--room` against it before connecting so a bad name is a message about the argument rather than a connection that opens and is turned away.

### Teams

**A team is a player, and several clients drive it.** That is the whole design and everything else here follows from it. `Server::make_teams` creates the teams as ordinary `Player` rows — a number, a purse, a patch of granted ground — before anybody joins, and joining one takes its controls: `Player::plays_as` says which number a client's cells carry, and it is the client's own number in a free-for-all.

Nothing else in the crate learns that teams exist. `net::reach`, `may_place`, `value_delta`, `grant`, `spawn_for`, `credit`, `territory` and `matches::leader` take a `PlayerId` and compare it, exactly as they did before there were teams — because two allies *are* the same player, so "are these two on the same side" is a question with nobody left to ask it about.

What that replaced: a `Sides` array indexed by `PlayerId`, copied onto every `Match` broadcast so the client could price a placement beside a teammate the way the server would, and an `allied()` call threaded through placement, pricing, spawning, mining, scoring and colour. There is nothing to price differently now — a teammate's cells are the client's own.

`ClientMessage::Create` carries `teams: Option<u8>` — `None` is a free-for-all and `Some(n)` is n teams. **A world may have them as much as a match can**: a team is people playing as one player, one purse and one patch of ground, and that is worth having without a result to win. What a match adds is that the teams have to be even at the whistle.

A world's teams are never settled either, because there is no whistle to settle them at: people join and leave one as they like. A match's are fixed once it is running, or changing sides would hand your ground to the people you were fighting. `JoinTeam` naming your own number is how you step off a team.

**A team costs a number.** There are fifteen — `PlayerId` is four bits in the cell — and teams and seats come out of the same pool, so a match with `n` teams has `n` fewer people in it. That is the price of the two being the same kind of thing, and it is what makes it impossible for a team and a seat to be the same number: they used to be drawn from one 1..15 space by two different rules, and an unaligned player 3 was seated on top of team 3. `net::MAX_TEAMS` is seven for the same reason — a team nobody can sit on is not a team.

Scoring needs no summing. A team's cells all carry its number, so `Server::territory` has already counted them under it and `matches::leader` is the answer; `matches::leader_of`, which took the roster's allegiances and added each side up by hand, is gone.

One purse, and it is the team's own row. `Server::value_of` and `credit` both resolve through `plays_as`, so the `Welcome`, the `Purse` that rides on a checkpoint and the refusal in `handle` all read one number. There used to be a copy on every ally and an invariant keeping them equal.

`start_match` refuses a match nobody would want to play: somebody unplaced, or a team with nobody on it. Sizes beyond that are not checked.

### Ending one

Three ways a match finishes, and the phase is the same `Over` for all of them.

Its own condition, which `decide` checks after each step so the generation that met it is the one the score is read from.

**Whoever started it calls it off**, with `ClientMessage::EndMatch` — the same person and the same reasoning as the whistle: they arranged it, so they are the one who can say it has stopped being worth playing. The result is real and is rated, because a match that ends with no result is one nobody can be held to.

**Everybody else gives up.** `ClientMessage::Forfeit` concedes for a **seat**, which is the distinction a team needs — one of three walking away leaves two pairs of hands on the team, and `Server::still_in` says a number is in the match while at least one seat playing it has not conceded. Being *offline* is not being out: a dropped connection is a player who can come back with their token, which is what the token is for. A seat that has given up stops placing, or a concession would show in the scoreboard and nowhere else.

The check for "one number left" lives in `forfeit` and not in `decide`, and that is not tidiness: a match that simply *has* one player in it has not been won by them, and putting it in `decide` ended every such match on its first generation.

### Who somebody is

**The client keeps a secret and the server issues the name.** A `Secret` is sixteen random bytes made at startup, kept in the client's own store, and sent on a `Join`; the server exchanges it for a `PersonId` — issuing one the first time it sees that secret and giving the same one back for ever after — and `server::people` remembers the pairing. The id is what everybody else is shown, in a lobby or beside a rating, and it says nothing about the secret behind it because it was picked at random rather than derived.

That means the client cannot know its own public name until a server has told it, which is why `Welcome` carries it and the client writes it down.

**It was an ed25519 keypair.** The client made both halves, the public one *was* the id, and a join was a signature over a nonce the server had just sent. That buys exactly one thing this does not: a server cannot be you on a *different* server, because it never learns the secret half. With one server it buys nothing, and it cost a signature scheme, an OpenSSH key parser, a round trip before every join, and a dependency — so it went, along with `ServerMessage::Challenge` and the state on both sides that waited for it.

What is left is exactly as strong as the rejoin token it will replace: whoever holds the secret is you, and the server it is presented to knows it. That is the strength [networking.md](networking.md#coming-back) already argues is right for a game with no accounts.

**Before a second server exists this has to change**, and not because anything breaks — because "the server knows your secret" stops being harmless the moment there is somewhere else to be you. See [planned.md](planned.md#player-profiles).

`server::people` therefore holds secrets now, which it did not before and which is worth saying out loud rather than leaving to be discovered: that file is what an attacker who reached the disk would want. It is not a new exposure — a room file already holds a rejoin token per seat, which is the same bargain with a smaller blast radius — but it is a single-server design written down as one.

### Watching

`ClientMessage::Watch` takes a room and no seat. `ServerMessage::Watching` answers with the room, its name, its tick and its shape — a `Welcome` without a player, because a spectator has no number, no token, no purse and no spawn, and sending zeroes would have the client draw a purse belonging to nobody.

A spectator is **not a player with the actions taken away**, and that is forced rather than chosen. A seat is one of fifteen — `PlayerId::MAX`, four bits of cell — so spending one on somebody who is only watching costs a real player their place. And **no late joining is a rule about players**: somebody arriving at generation four hundred is exactly who watching is for, so `Watch` is admitted at any generation while `Join` to a running match is still refused.

`rooms::Caller` carries the connection, the seat if there is one, and the room being watched if there is one. A watcher is routed to its room like a player with no number: `Server::handle` already takes `Option<PlayerId>` and already answers a `Subscribe` from nobody with the chunks it asked for, so reading works out of the box and everything that *acts* is refused for want of an id.

That last part had to be made true. **An action belongs to the connection that sent it**, checked against `stamped.player` — without which the `player` field was a claim rather than an identity, any connection in a room could act as anybody in it, and a connection with no seat could act as everybody.

### Made by a client

`ClientMessage::Create { name, shape, victory }` makes one over the wire — the three things `world new` and `match new` take at the console, with `victory: Option<Victory>` being the whole of the difference between them. The answer is `ServerMessage::Made(Result<RoomName, String>)`: the name the room actually got, or the refusal in the wording the console would have printed.

Answerable **without a seat**, which makes it the third such message after `Join` and `Rooms` and for a sharper version of the same reason — it names a room that does not exist, so there is nowhere to have been standing when it was sent.

**Making a room does not put you in it.** The client sends `Join` with the name that came back, which is the same `Join` the room list sends, so there is one path into a world rather than two. The name has to come back rather than be assumed because `net::room_name` trims and lowercases: what was typed and what the room is called are not always the same string, and only the second one joins.

Two things hold the line on a server anybody can ask for worlds. **A cap**, `rooms::MAX_MADE_ROOMS` and `--max-rooms`, counted over rooms made this way and not over rooms an operator declared — thirty-two by default. And **an owner**: the connection that asked is recorded in `Rooms::made`, readable with `made_by`. Nothing enforces the owner yet; what recording it buys is that "close what you opened" and "you have three open already" are answerable later without a migration, and that the log line for a room that appeared says who asked for it.

A connection id is not a player. A room is made before anybody has joined it, so there is no `PlayerId` to record — `rooms::Caller` carries the connection, which exists from the moment the socket opens, alongside the seat, which appears only with a `Welcome`. Ids are never reused, so a room's owner cannot become somebody else by a counter filling a gap.

The cap is the backstop rather than the fix. The fix is sleeping a room nobody is in, which is half built already and is written up in [planned.md](planned.md#making-rooms-from-the-client).

Every room runs on **one clock**. Separate worlds, but one generation span: a room with its own rate would be a second thing for a client to be told and a second way for the two to disagree about what a tick is.

There is one broadcast channel, and every message on it carries the room it came from; each connection drops what is not its own. One channel per room would save that comparison and cost a shared map of senders that connections and the simulation task would both have to lock — a lock, to avoid a string compare, on the one path that must not have one.

## Matches

A room with a beginning, an end and a winner. An ordinary room runs forever and nobody wins it; a match is the same world with three things added — everybody starts together, it stops, and when it stops somebody has the most ground.

**Nothing about it is in `sim`.** A match is an arrangement of when a room steps and who may join it, and both are the server's business. What that buys is that a match cannot introduce a rule the world has to honour, so a match world behaves exactly like the one people practise in.

Three phases, on `server::matches::Phase`:

| | steps | takes actions | admits newcomers |
|---|---|---|---|
| `Open` | yes | yes | yes |
| `Gathering` | **no** | **no** | yes |
| `Running` | yes | yes | **no** |
| `Over` | no | no | no |

**Nothing happens before the whistle.** Gathering neither steps nor takes actions: players join, get their patch and their block, and wait. Letting people place while gathering was tried and is wrong — it is fair in *generations* and unfair in **time**, because somebody who joined ten minutes early has had ten minutes to think and draw and the last to arrive has had none. Holding the tick still does not hold a clock still.

**A match's world is empty until the whistle.** `Server::join` grants on arrival only where the phase takes actions, so a gathering match hands out nothing, and `start_match` grants every player who is here, in player order — the order matters because seats are a spiral by number and two peers must lay the same one. Players join a match with a starting value of **zero**, from `Server::starting_value`, for the same reason nobody may place: value spent while gathering is an opening bought in wall-clock time.

**The lobby is a screen, not an honour system.** A match that has not started looks exactly like a game that is broken — nothing moves, and nothing a player does appears — so the client is told what the match is doing and shows a panel saying so, with who else is here and how it is won. `ServerMessage::Match` carries it, and it goes out **when it changes** rather than on a cadence: a gathering match does not step, so there is no tick to hang "every so often" from and a lobby that only refreshed when the world moved would never refresh at all. `RoomInfo` carries the same phase, so the room list says which rooms are matches and which have already started, rather than letting somebody click into one and be refused.

What that makes the opening is a race rather than a draw: everybody is looking at the same thing when the clock starts, and the first thing anybody lays is laid against a running world, where hesitating costs generations. An action arriving before the whistle is **dropped**, which is what an action the server will not take already does — the client predicted it locally and the next `Checkpoint` puts the world and the purse back. It will keep doing that until a match's phase reaches the client and it can refuse for itself.

**No late joining.** A player arriving at generation four hundred is not in the same race: everyone else has four hundred generations of ground and they have a block. A `Join` into a running match is refused with a reason rather than allowed and hopeless, which would read as the game being broken. A player *already* in the match is a different question and still gets back in with their token — that is the door, not the room.

The deadline is a **tick**, not a clock. The tick is the generation and is already what a client adopts from its `Welcome`, so a match ending at generation N needs no clock synchronisation, cannot be lengthened by a client that pauses, and is the same instant for everybody by construction. It is measured from the tick the match started at, so a match that gathered for an hour still runs its full length.

Two win conditions. `timer N` is most ground after N generations; `territory N` is first to N squares. Both are read from `Server::territory`, one pass over what is held — the world keeps no running total, and one kept up to date would have to be corrected by every rule that moves ownership.

**Granted ground does not count towards a score.** `HOME` never decays, so a player wiped out in the first minute still holds their patch at the whistle, and scoring it would be points for having turned up. The floor stays — they can still build on it — it simply wins nothing.

A match is **not saved**. It is an event rather than a world to keep, and a half-finished one restored into a server that had forgotten it was a match would run on forever with nobody able to win it. A restart loses it, which is the honest outcome.

### At the console

```
match                                        what matches there are, and what they are doing
match new dawn infinite timer 2000           most ground after two thousand generations
match new arena toroidal 18x18 territory 500 first to five hundred squares, wrapping
match start dawn                             start that one's clock
match dispatch                               start the one that is waiting
```

Named like any other room, because a match **is** one: that is the name people type to join it, the name `match start` takes, and the name it is listed under, so a generated one would be a second vocabulary for the same thing. A name a room already has is refused rather than reopened, for the reason `new` refuses one — "make" that sometimes means "and empty it" is one keystroke from destroying a world somebody is standing in.

A torus without a size is refused rather than given a default: how big a wrapping world is, is most of what makes one match different from another, so guessing it would hide the important number. `dispatch` refuses to choose between two waiting matches, because starting the wrong one cannot be taken back.


## Players

The lowest unused number, from 1. Zero is reserved for unowned cells, and the cell has four bits, so **15 players** is the capacity — a full server refuses rather than truncating a number into a cell. It was 31 until the level took a bit off the owner byte; see [simulation.md](simulation.md#fifteen-players).

Joins and departures are logged:

```
connection opened
join: PlayerId(1) "late" in room lobby at tick 73 (1 online)
subscribe: Some(PlayerId(1)) asked for 81 chunks, sending 4 that hold life
leave: PlayerId(1) "late" from room lobby after 10 ticks (0 still on)
```

The subscribe line is the useful one when a client sees nothing: it says whether the viewport is even pointed somewhere the server has data.

## The console

The server reads its own terminal. `help` lists what it takes:

```
  world new NAME SHAPE [ROWSxCOLS]         make a world: infinite|toroidal
  world delete NAME                        remove it, and the file it was saved to
  world sleep NAME                         stop stepping it
  world wake NAME                          step it again
  world                                    what worlds there are, and who is in them
  match new NAME SHAPE [ROWSxCOLS] HOW N   a match, and timer|territory N
  match start NAME                         start that match's clock
  match dispatch                           start the one match that is waiting
  match delete NAME                        remove it
  match                                    what matches there are, and what they are doing
  rooms                                    everything, worlds and matches together
  stop                                     save every room and shut down
  help                                     this
```

**A world reads like a match without a way to win**, word for word, because that is what one is. Two vocabularies for one idea is how a console stops being something anybody can remember, so `world new` takes exactly what `match new` takes less the win condition — including *requiring* a shape, where the old `new arena` fell back on whatever the command line asked for. `new` and `room` still reach it, since that is what the muscle typed for months, but they take the new arguments.

**Sleeping is nearly free, because the tick is the generation.** Every room steps four times a second for as long as the process lives, whether or not anybody is in it — a world somebody built in and walked away from costs its full simulation for nobody. A sleeping world does not move, so waking is indistinguishable from never having slept and a returning client adopts the tick it left off at. Actions are not applied either: one applied to a world that is not moving would land on a tick that has not happened.

**A match does not sleep.** It has a clock and a deadline measured in generations, and a sleep would be a pause in a race some of whose runners are asleep and some of whom are not.

**Deleting refuses what it cannot take back**: a world with anybody in it, because the difference between "nobody is in it" and "nobody was a moment ago" is a question the person typing can answer and the server cannot; and the default room, because `resolve(None)` sends every client that names none there and a server without one has nowhere to put anybody.

`new` exists because a room was declared on the command line and there was no way to make one afterwards, so adding a world meant restarting — which disconnects everybody in every *other* world to add one nobody is in yet. A room made this way is **saved before anything is in it**: one that lived only in memory until the next periodic save would vanish on a crash, and the person who made it would have no way to tell whether it had ever been real.

The size argument is the first per-room shape there is. `--torus` applies to every room a run creates, so a server could not offer a wrapping world and a boundless one side by side; `new ring 18x18` can. Without a size a room gets whatever the command line asked for, so `new arena` means what `--room arena` would have meant.

An existing name is refused rather than reopened, because "create" that sometimes means "and empty it" is one keystroke from destroying a world somebody is standing in. A name that is not a room name is refused with the same rule a join is refused by.

**The up arrow works**, and `rustyline` is why. Not clap, which is a different problem: clap parses argv, and what a console wants is a *line editor* — the two do not overlap, and clap would not have given the up arrow. The parser stays hand-written because it is a hundred lines, is a pure function of a string and the worlds, and is tested command by command including that `help` and the parser agree; clap is argv-shaped, wants to print usage and exit, and would put the tested surface inside a dependency. It is also native-only and behind the `server` feature, since `server::console` compiles for wasm32 too and a browser has no terminal.

History is kept in a file as well as in memory, because restarting is exactly when you want the command you typed before. Where there is no terminal to edit — a pipe, a file, systemd's `/dev/null` — it falls back to reading plain lines, so `echo rooms | server` still works. **End of input is not `stop`**: it arrives the moment a backgrounded server's terminal goes away, so treating it as a command shuts down a server nobody asked to shut down. Ctrl-C abandons the line rather than the server, for the same reason — the way out is to type it, and typing it saves on the way.

**Log lines appear above the command being typed**, not through the middle of it. A logger writing straight to the terminal lands in the half-typed line, because the cursor is sitting after a prompt it knows nothing about; rustyline's external printer wipes the prompt, writes the line, and draws the prompt and whatever was typed back underneath.

`env_logger` keeps doing the formatting — the timestamps, levels and colours are its business — and only its `Target` changes, to a `Write` that buffers to the newline and hands whole lines to the printer. Whole lines because a printer call redraws the prompt, and env_logger writes a record in several `write` calls. The printer is a global, installed by the console thread once it has an editor and taken away when it loses one, because the logger is set up before there is any terminal and may never get one: under systemd there is no prompt to protect and the lines go to stderr exactly as they always did.

**Parsing and doing are in `server::console`; reading is not.** `console::run` takes a line and the `Rooms` and returns what to print, which is the only way any of it can be tested — a terminal is not something a test has. The reading is a thread of its own in `server::ws`, not `tokio::io::stdin`, whose reads are documented as not cancellation-safe: a pending read dropped inside a `select!` swallows the line it was in the middle of, so a command would go missing whenever a generation ticked at the wrong moment.

Commands run on the **simulation task**, because making a room is touching the worlds and there is exactly one place allowed to do that.

Answers go to stdout rather than to the log. A log line is something that happened; the answer to a question somebody typed is neither a warning nor a record, and routing it through the logger would let a quiet log level swallow the reply to a command.

A server with no terminal — under systemd, or started with `< /dev/null` — reads end-of-file, drops the console and runs on. That is the ordinary case for a server nobody is sitting at, so it is not a failure, and it must not become a loop that spins on end-of-file: the branch is guarded, and a headless server idles under one per cent of a core.

## Stopping

Three things mean stop, and they all mean the same thing:

| | |
|---|---|
| **SIGINT** | ctrl-C, what a person at a terminal sends |
| **SIGTERM** | `kill`, `systemctl stop`, `docker stop`, `timeout` in a script |
| `stop` | typed at the console |

SIGTERM is the one that matters most and the one a person is least likely to use, because it is how a server is stopped when nobody is watching. Listening only for ctrl-C meant every one of those killed the process outright, taking up to `save_every` of every room with it.

They meet at one `tokio::sync::watch` channel — a `watch` rather than a `Notify` because it remembers, so a waiter that arrives after the signal still sees it, and it has as many receivers as there are things to stop. The HTTP server drains connections on it and the simulation task breaks its loop and saves.

## Saving

Every 30 seconds and on a clean shutdown, every room at once. Writes go to a temporary beside the target and are renamed into place, so a crash mid-write cannot leave a half-written world where the real one was.

**One file per room**, `<name>.ckw` in the rooms directory. The format holds one world and its players, which is exactly one room, so the file is unchanged and the directory is what grew. The room's name is the file's name and is not written inside it: two places to keep one fact is one too many, and the one a person can rename is the one that has to win. Renaming `lobby.ckw` renames the room.

A player record carries their token, so a restart does not hand every returning player a new number and leave their ground standing there, theirs and unreachable.

**What is not in the format is whether they were connected**, and that is deliberate: `online` is a fact about a socket, and the socket ended with the process. It also has to be *set* on the way back in, which is where this went wrong for a while. `Player::new` is what a player joins with, and joining means being online, so a player rebuilt from a file came back marked connected — and a player who is online cannot be returned to by their token, that check being what stops two tabs becoming one player. Every player who was in a room when it was written therefore found their token refused on the next run and joined as somebody new, which is the exact failure the token exists to prevent, arriving only when the server closed. `persist::load` clears it now, and `a_token_survives_the_server_closing` is the test that would have caught it.

A missing file starts fresh. A corrupt or mismatched one is an **error** naming the room that failed, not a silent reset — discarding a world is the worst possible response to a bad read, and with several of them "cannot read the world" no longer says which. A file in the directory that is not a room name is skipped with a warning rather than opened: refusing to start over one stray name would take every other world down with it. One room failing to save does not stop the others, for the same reason.

The shutdown save is **waited for**, not aborted. The simulation task saves after its loop; `sim.abort()` cancelled it at its next await point, which is inside the loop, so the save never ran and a clean exit quietly lost up to thirty seconds of every room. The wait is bounded at ten seconds, because a shutdown that does not shut down is worse than one that loses a save it warned about.

### The `.ckw` format

Hand-rolled and little-endian. A chunk is already a flat byte array, so the file is mostly a memcpy, and owning the format means a version byte can gate a migration.

```
"CKW\0"                    magic
u8                         version (currently 2)
u8 u8                      chunk edge, cell width
u8                         world kind: 0 infinite, 1 toroidal
[i32 i32]                  rows, cols — toroidal only
u64                        tick
u32                        chunk count
  i32 i32                  coordinate            } repeated, sorted,
  [u8; N*N*cell]           cells, verbatim       } only chunks holding something
u32                        player count
  u8                       id                    }
  u16 + bytes              name                  } repeated
  u64                      last seen             }
  i32                      value                 }
```

The header records chunk size and cell width, and a file written by a build with different ones is **refused** rather than read as garbage — the shapes are exactly what would otherwise decode into plausible nonsense.

Empty chunks are not written; they are implied — which is why a world is rebuilt **empty** and then filled from the file. Building a torus with `World::toroidal`, which seeds a glider, meant every chunk the file did not mention kept it, so loading a saved wrapping world invented five cells nobody had placed, in chunk zero, on every restart. The infinite arm was already `infinite_empty`, and that asymmetry is how it hid.
