# Where the design is borrowed from

Named sources, and what each one is actually for. A reference is only useful if it says *which* problem it solved, so each entry here is one idea and the place it applies — not a list of games that are good.

Nothing here is about looking like the thing borrowed from. The palette, the flat panels and the single accent are settled in [theme.rs](../src/client/views/theme.rs) and take after Pezzza's simulations; what follows is about **structure and hierarchy**, which is the part that transfers.

## The menu

| source | what is taken | where it shows |
|---|---|---|
| [Infinite Chess](https://www.infinitechess.org/) | One labelled row per decision, and a row appears **only when the decision it belongs to is live** | The world form: no size on a boundless world, no target on one that never ends, no name on a private room |
| [Infinite Chess](https://www.infinitechess.org/) | A game you make joins the same list everybody else reads — there is no "your games" pane | `ClientMessage::Rooms` is the only listing there is |
| [Clash Royale](https://interfaceingame.com/games/clash-royale/) | Depth almost never exceeds one level, and is disguised where it does | Home and Play are pages, not a stack; the form opens **in place** under the room list |
| [Clash Royale](https://interfaceingame.com/games/clash-royale/) | One accent on the one thing you are meant to do next, a second colour for everything else, no third | One accent-coloured control per page — Play on home, and the primary action at the foot of the form |
| [Clash Royale](https://interfaceingame.com/games/clash-royale/) | Everything important within reach of a thumb, one-handed | The action sits at the **foot** of the form with the fields above it, which is the opposite of the desktop instinct |
| [chess-tui](https://github.com/thomas-mauran/chess-tui) | Every screen has a key that leaves it | Escape, one level at a time, innermost first: a form shuts before a page does, and a page before a world |
| [generals.io](https://generals.io/) | The lobby creator **owns** the lobby | `Rooms::made` records the connection that asked for a room |
| [generals.io](https://generals.io/) | The count of open games is on the way in, before the list | The room and player count under the listing |

Two things are ours rather than borrowed, and are worth saying because they are the load-bearing layout decisions. **The two columns encode existing versus proposed** — a list the server owns beside a form that is yours — rather than being a way to use up width; that is why they are drawn differently rather than as two matching panels. And **the accent is one per column, not one per screen**, which is a widening of Clash Royale's rule that the two-column layout forced: each column has exactly one thing you would do next in it, and they are in different places.

What is deliberately **not** taken is the drop-down. Every choice on the form is two or three wide, so a row of toggles shows the whole of it where a list shows one of it — and a drop-down wants a popup layer, which is one more thing to keep off the world behind the menu.

## The dashboard, and a rating

| source | what is taken |
|---|---|
| [MCSR Ranked](https://wiki.mcsrranked.com/gameplay/elo_and_ranks) | **Named tiers over a bare number.** Six ranks at Elo thresholds — Coal through Netherite — so a rating is a thing to reach rather than a number to read. A raw Elo tells a player nothing about where they stand |
| [MCSR Ranked](https://wiki.mcsrranked.com/gameplay/elo_and_ranks) | **Placement matches** before a rating is shown, so one bad first game does not define somebody |
| [MCSR Ranked](https://wiki.mcsrranked.com/gameplay/elo_and_ranks) | **Decay only at the top**, and only on inactivity — the top 150 lose 5 a day after a week idle. It keeps a leaderboard honest without punishing anybody who plays occasionally |
| MCSR Ranked trackers | **An expandable match history**: a row per game, opening to the detail of that game |

The last of those is the only one buildable today, and it is worth doing before the rest: `client::record` already keeps fifty games and the home screen shows only a summary of them. A recent-matches list, each row opening, needs no server and no identity. The other three all wait on [rating](planned.md#rating), which waits on there being a person to rate.

## The architecture

| source | what is taken |
|---|---|
| [OpenFront](https://github.com/openfrontio/OpenFrontIO) | `core` (deterministic simulation) / `client` / `server`, plus a shared binary wire schema. The closest comparable project — a browser territory RTS — arriving independently at the split this crate calls `sim` / `client`+`render` / `server` / `net::codec` |
| [chess-tui](https://github.com/thomas-mauran/chess-tui) | Rules and display in **crates that cannot see each other** — `shakmaty` and `ratatui`. The same argument as [architecture.md](architecture.md) makes with feature gates, one level up |
| Jonas Tyroller, [Best Code Architectures For Indie Games](https://www.youtube.com/watch?v=8WqYQ1OwxJ4) | Data / Logic / Interface, with the load-bearing clause that **the interface layer never executes logic** |

That last one is second-hand and worth flagging as such: the video is not fetchable as text, and the principle above is as [a devlog summarises it](https://mugule.itch.io/badgertactics/devlog/1626058/04-turn-based-architecture). It is recorded because it names something this crate already half-does and half-does-not.

Where it holds: `views::menu` states the rule in its own module doc — it opens no sockets, sends no messages, holds what was typed and returns what was chosen — and `lobby`, `hotbar` and `stamp` follow it through the same `Chose`/`Picked` return-value convention. `client::desync` and `client::record` are logic with no egui and no socket, and are tested without either.

Where it does not: **`views::battle` is the one violation**, and it is the largest file in the crate. It sits in `client/views/` but holds the world, the link and the GPU pipeline, and it executes logic — `pump_link` folds server messages into the world, `lay` and `click` price and send actions, `advance_to` steps the simulation. See [planned.md](planned.md#the-session-comes-out-of-the-battle-view).
