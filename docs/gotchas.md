# Gotchas

Things that cost a day here, written down so they only cost one. Each of these produced a symptom that pointed somewhere else.

## `TexturesDelta` asserts on drop

egui hands over a `TexturesDelta`, and dropping one that still holds deltas panics. Applying every change is **not enough** — reading it through a reference leaves it full. It must be `clear()`ed.

*Symptom:* the client dies on startup at `epaint/src/textures.rs`, on the first frames when the font atlas arrives and the surface is still reporting `Skip` so nothing has drawn. And because the panic kills the app before any input is dispatched, it looks exactly like the world has stopped responding to clicks.

*Also:* apply textures when they are **produced**, not when they are drawn. A frame is not always drawn.

## `pkg/` is generated, gitignored, and never updated by a pull

The browser client is built by `wasm-pack` into `pkg/`, which `.gitignore` excludes. So a working copy keeps whatever was last built there while `index.html` and the Rust move on, and **nothing detects that they have diverged**. A copy that used to work stops working at a commit that is fine everywhere else, which sends you looking at the commit.

**A build that fails leaves the old `pkg/` standing.** `wasm-pack` writes into that directory; it does not empty it first. So a toolchain too old to compile the crate — the floor is 1.87, and `rust-version` names it now — produces an error and *no change on disk*, and the page goes on serving whatever was built months ago. Reading the output of the build is the whole of catching that.

*Symptom:* it fails on one machine and a fresh clone of the same commit is fine. That is the tell, and it means the difference is something not in git — `pkg/` first, then the browser's cache.

The specific way it bit: `index.html` called `init({ module_or_path })`, the object form, which an older `wasm-bindgen` does not unwrap. It passes the bytes bare now, which every version accepts — a `Uint8Array` is not a plain object, so a newer init takes it as the module rather than trying to destructure it.

## A relative import resolves against the path the page was served at

Every screen has a path now, and each is answered with the same `index.html`. The page imported its module as `./pkg/conwayskingdom.js`, which at `/` means `/pkg/…` and at `/room/arena` means **`/room/pkg/…`** — a 404, so the module never loads and the page is blank.

*Symptom:* it works until you reload, and then it does not. In-app movement is `history.replaceState`, which changes the address and fetches nothing, so the wrong base URL is never exercised while you are playing. A refresh is the first time the browser actually loads the document from that path.

Absolute — `/pkg/…` — because the client is mounted at the root. `<base href="/">` would do the same job and would also silently retarget every other relative URL on the page, which is a wider promise than the one being made.

This was reverted once and had to be put back, which is worth recording because the reasoning for the revert was sound and aimed at the wrong comparison. It said an absolute path assumes the client sits at the origin's root, and that `/` and `/?room=main` both resolve it identically so it could not be what separated them. Both true. The pair it separates is `/room/main` from `/?room=main` — a two-segment path against the root with the room in the query — and that pair is what a reload actually lands on, because `client::route` writes the path form into the address bar. The mount-point worry is answered by `server::ws::serve_client` itself: it mounts the page at `/` and the module at `/pkg` and offers no way to put either anywhere else.

## A finger is not a pointer unless somebody says so

`Views` translated winit's mouse events into `egui::RawInput` by hand and never translated `WindowEvent::Touch` — so on a touchscreen egui received **no press at all**, and every button in the interface was dead: the menu, the lobby, the hotbar, the library.

*Symptom:* the game works and only the things drawn on top of it do not, which is what makes it hard to place. The world responds to a finger because the client reads `App::on_touch` itself; the interface does not, because egui was never told a finger exists. It reads as "the UI is broken" rather than as "one event is missing", and it is invisible on any machine with a mouse.

Two things it needs beyond the obvious press and release. A `PointerMoved` **before** the press, because egui decides what a press landed on using the pointer's position, and on a touchscreen the pointer was last left wherever the previous touch ended. And a `PointerGone` on release, because a finger that lifts leaves nothing hovering — without it a button stays looking hovered under no finger.

Whether the finger belongs to the interface or the world is decided **once, when it goes down**, and remembered until it lifts. A drag that began on a button must not become a drag on the world halfway through because it slid off the button.

## A control drawn inside a platform branch is a control one platform does not have

The refresh that reaches a server was drawn inside the `else` that makes the address a **field**. A browser has a label there instead — its socket comes from the page it was served by, so a typed address would be a promise the client cannot keep — and the button went with the field. The web client then had no way to ask any server anything, and looked exactly like a client that could not connect.

The rule that falls out: **branch on the smallest thing that actually differs.** What differs between the two clients is whether the address can be typed. Asking does not differ, so asking belongs outside the branch.

It is worth a test rather than care, because reading the drawing code does not reliably catch it. egui runs headless — `Context::begin_pass`, call the view, `end_pass` — so a view can simply be asked what it decided, once per platform. `client::views::menu::tests::the_web_client_can_ask_its_server_and_so_can_a_native_one` is that test, and it fails on the bug above.

*Also:* clear the `TexturesDelta` that `end_pass` returns, or the test panics on drop for the reason below.

## `egui-winit` does not build for wasm32

At 0.36.1, `egui::DroppedFile` declares `bytes_async` under `cfg(wasm32)` and egui-winit's impl provides only the native `bytes`, so the trait is unimplemented on that target. Upstream bug, not a wiring mistake.

We translate winit events into `egui::RawInput` by hand instead — about a hundred lines, one code path for both targets. A HUD needs pointer, wheel and modifiers; the IME and clipboard handling egui-winit exists for is not in play.

## A canvas has two sizes, and nothing sets the one that matters

`width`/`height` are the pixels drawn into; the CSS box is where those pixels are stretched to. Styling the canvas `100vw` by `100vh` sets the box only. **Nothing sets the backing store.** winit's resize observer reports the box and emits `Resized`, and wgpu configures a surface, but neither writes `canvas.width`, so it keeps whatever it had — 300×150 by default, or 1×1 once winit has applied its own zero-sized idea of the window.

Do not write the canvas directly either: wgpu's WebGL backend sets the backing store to match whatever the **surface** is configured to, so a write to the canvas is undone on the next frame. Configure the surface from the canvas's client box each frame and the canvas follows it.

winit's `inner_size` is no help: it starts at zero, so the surface is configured 1×1 before any resize observation lands.

*Symptom:* the whole game drawn into a handful of pixels and scaled up, **and it comes right the instant you open the web inspector** — devtools changes the window size, which is what finally gets a resize past whatever guard was blocking it. That tell is worth more than any amount of reading.

## `navigator.gpu` existing does not mean WebGPU works

On a secure origin — `localhost` counts — Chrome exposes `navigator.gpu` and then returns **null** from `requestAdapter` whenever no GPU is usable: a blocklisted driver, a crashed GPU process, a VM, a headless browser. wgpu hands that null back as an `Adapter` anyway, and the first method called on it throws

```
TypeError: Cannot read properties of null (reading 'info')
```

which kills the page before the WebGL2 fallback is reached. Ask the browser yourself and only name `BROWSER_WEBGPU` if the answer is a real adapter.

*Also:* web-sys's WebGPU bindings are behind `--cfg=web_sys_unstable_apis`, so that check goes through `js_sys::Reflect` rather than putting a build flag between the crate and compiling.

## Setting a canvas's size clears it

`Window::inner_size` on the web returns a value winit updates from its **own** resize observer, so it never reflects a request in the same frame. Comparing against it means requesting a resize every frame, and setting a canvas's size clears it — the picture never survives to be shown.

Compare against the last size *requested* instead.

*Symptom:* a completely blank browser canvas, with no error anywhere.

## A one-layer array texture is not an array on GL

wgpu-hal's GL backend picks its texture target from the **texture** descriptor, not the view: `depth_or_array_layers == 1` makes a `TEXTURE_2D`, and a `D2Array` view over it mismatches.

```
wgpu-hal heuristics assumed that the view dimension will be equal to `D2` rather than `D2Array`
```

Allocate the real layer budget. Native Vulkan does not care, so this only shows in a browser. Still live for the **chunk** texture, which is an array with one chunk per layer; the sprite sheet is a plain `D2` now and no longer in reach of it.

*Also:* that message arrives through `log::error!`, so it is non-fatal and its console stack points at the wasm-bindgen shim that forwards it, not at anything of ours.

## `textureSample` needs uniform control flow

WGSL forbids anything using implicit derivatives inside a conditional that varies per fragment. **Naga accepts it; Tint does not**, and it is undefined on the GL path.

That is one reason each cell state has its own sprite rather than compositing: one state, one unconditional sample.

## `write_texture` is exempt from the 256-byte row alignment

`copy_buffer_to_texture` and `copy_texture_to_buffer` require `bytes_per_row` to be a multiple of 256. `Queue::write_texture` does not, which is what makes a 64-byte chunk row a legal upload.

## Alignment decides whether a cast is free

`[u8; 2]` has alignment 1; a `u16` field forces alignment 2. Only the first can be reinterpreted from an arbitrary byte offset, which is what a save file and a wire frame hand you. With alignment 2, `bytemuck::from_bytes` panics on an odd offset.

## Reserved words

- `meta` is reserved in **WGSL**. It fails at pipeline creation, not compile time.
- `gen` is reserved in **Rust edition 2024**. `for gen in 0..n` will not parse.

## bytemuck needs feature flags

Without `min_const_generics`, `Pod` is implemented only for a fixed list of array lengths. `[Cell; 256]` happens to be on it and `[Cell; 65536]` is not, so code compiles at one chunk size and fails at another. `zeroed_box` needs `extern_crate_alloc`.

An enum can never be `Pod`: it has invalid bit patterns. And bytemuck refuses to derive `Pod` on anything with padding, which is a guardrail worth having.

## Two versions of one crate cannot share a window

`egui-winit` tracks winit and `egui-wgpu` tracks wgpu. If the versions disagree with yours, the `Window` and device types are unrelated and nothing will bridge them. Check with `cargo tree -i winit` and `cargo tree -i wgpu` — one entry each.

## `is_empty` is three different questions

For a chunk, "nothing alive" discards panes for good. "Every cell exactly `DEAD`" keeps every chunk life ever passed through, because a cell keeps its owner when it dies, and an infinite world grows without bound. It has to be "no life and no structure".

## A drag is not a sequence of small moves

Deciding that a press has become a drag from the distance between **one pointer event and the next** does not work. A slow, deliberate sweep arrives as a stream of one- and two-pixel moves, and no single one of them clears any threshold worth setting — while a hand that shakes on the button produces one that does. Measure from where the press landed instead.

*Symptom:* dragging out a rectangle places a single cell at the release point, and the faster you drag the more likely it is to work. It reads as the fill being broken rather than the classification.

## The canvas has to be *focused*, and winit will not focus it

winit gives the canvas a `tabindex` so it **can** take focus, then waits for something to give it. A freshly loaded page has focused nothing, so the canvas is not the active element, and two things follow from that one fact:

- **Keyboard events go nowhere.** They are delivered to the focused element, and there is not one. WASD, the arrows, the digits and escape all do nothing.
- **A trackpad pinch pans instead of zooming.** In a browser the ctrl in a pinch is not a modifier state: the browser sets `ctrlKey` on the wheel event itself and no key is down. winit turns that into a `ModifiersChanged` — but only while the canvas has focus, so unfocused, every pinch arrives looking exactly like a two-finger scroll: vertical, small deltas, no ctrl.

*Symptom:* both start working the moment you click the page, and a click also draws a cell — so it reads as the game needing to be "started" rather than as focus, and the pinch bug in particular keeps coming back as fixed-then-broken depending on whether the tester clicked before trying it.

`canvas.focus()` after appending it fixes the keyboard. The pinch is fixed separately and more thoroughly, by reading `ctrlKey` off the wheel event in a **capture**-phase listener on `window` — capture, so it runs before the canvas's own listener has queued the event it belongs to. Bubbling would run after it and be a gesture late. Modelling a per-event flag as a held key is the actual mistake; the focus call only hid it.

## Under Xvfb with no window manager, nothing has keyboard focus either

The same bug, one layer down, and worth knowing before spending an afternoon on a keyboard translation that was correct all along. `Xvfb` with no window manager never sets X input focus, so winit's X11 backend reports no key events at all. `xdotool key` appears to do nothing.

`xdotool windowfocus --sync $(xdotool search --name Conway | tail -1)` sets it directly, and everything works. Clicks need no such thing, which is what makes it confusing: the pointer works, so the window looks alive.

## Ask for the next frame in `about_to_wait`, not at the end of the last one

A `request_redraw()` made **while handling `RedrawRequested`** may be folded into the redraw already being processed. winit says so and several backends do it, so the request is dropped, nothing is left to wake the loop, and under `ControlFlow::Wait` it sleeps until some input arrives — and then the redraw fires immediately behind that input.

*Symptom:* a stall, then two frames almost together, and **worst when the pointer is still**, because nothing else is waking the loop. It looks like the GPU stuttering when nothing about the GPU has changed.

`about_to_wait` runs after the queue has drained and before the loop sleeps, so a request made there is always outstanding when it sleeps and there is always exactly one frame pending. Pacing then comes from the present queue, which is `Fifo` and is the thing that should be setting it.

The second contributor is real too and is not fixed: `Frame::begin` calls `surface.get_current_texture()`, and with `Fifo` that blocks the same thread that dispatches input until the compositor releases a buffer. Input arriving during that block is drained in one go afterwards. Getting rid of that means not presenting on the event thread, which is a bigger change than it is worth so far.

`dt` inherits all of it, and it used to be handed to `World::update` unclamped — so a long stall (a window drag, devtools opening, a backgrounded tab) became `MAX_CATCHUP_STEPS` generations in one frame and the *world* lurched too, not just the picture. It is clamped to one generation's worth now. Connected, the server is the clock and this only paces the interface; offline it is the difference between a hitch and a jump.

## A `Paint` is idempotent for one generation only

Applying the same paint twice at the same tick changes nothing. Applying it one generation late is a different action: the cells it named have moved, and laying them again puts the original pattern back on top of where it went.

That is what a client does if it applies its own actions when the server broadcasts them back, and it needs latency to happen — the action has to miss one server step, which on a loopback socket it never does. So it is invisible locally and ordinary over a real network.

*Symptom:* draw a glider, watch it thicken into a blob and settle into a still life, then snap back to a glider a few seconds later when the resync lands.

## `ui.horizontal` centres against a row height it does not know yet

Two panels side by side, both exactly 56 pixels tall, sat 13 pixels apart. Not a size problem — an alignment one. `ui.horizontal` is `Align::Center`, and each item is centred against the row's height, which is whatever the tallest item turns out to be. The first item is placed before the last one has been measured.

*Symptom:* a row of things that are demonstrably the same height and visibly are not level, and which move when you change something in only one of them.

`horizontal_top` plus `ui.set_min_height` on the row and on each panel fixes it: top alignment needs no row height, and stating the height means nothing has to be guessed. Measured back to zero pixels apart, with and without a stamp in one of them.

*Also:* anything in the row that is deliberately shorter — a divider, say — should still **allocate** the full height and paint short. An item that allocates less takes part in the alignment as the short thing it is.

## Serving over plain HTTP costs you WebGPU

`navigator.gpu` requires a secure context. `http://host:8080` is not one, so a browser falls back to WebGL2 — which works, but is a different backend with lower limits. `localhost` **is** a secure context, so an SSH tunnel gets you WebGPU without TLS.

## A `%` in an address is a slice by byte offset waiting to panic

`client::route::decode` walked the query string looking for `%XX`, and read the two hex digits as `&raw[i + 1..i + 3]` — a slice of a `&str` by **byte** offset. Byte `i + 3` lands in the middle of a character whenever the thing after the `%` is not ASCII, and Rust refuses that slice by panicking.

```
?room=%€   ->  end byte index 3 is not a char boundary; it is inside '€'
```

*Symptom:* the page is blank and reports **"The game did not load"** eight seconds later, which is the same sentence a missing `pkg/` and an unreachable server both produce. This runs on `location.pathname` during `startup`, before the first frame, so nothing else has happened yet to point anywhere.

It works in bytes throughout now and decodes the result as UTF-8, so `%E2%82%AC` comes back as `€` rather than as the three characters it is spelled with, and what cannot be decoded is kept verbatim so a refusal names what was actually typed.

The general shape is worth more than the instance: **an index computed from one string and used to slice another is only safe on ASCII**, and every string that reaches this client came out of an address bar.

## One browser is six connections, and the tunnel counted players

Not in this repository: the tunnel is two Python scripts that sit **beside** it, and they are what makes a server on a home connection reachable at all. Written down here anyway, because the symptom is entirely inside the browser client and this is where somebody will come looking.

`agent.py` kept `--pool` spare connections open through the firewall and returned a slot to the pool when the connection it carried **closed**. That is the right rule for a player and the wrong one for a browser: loading the page opens several TCP connections — the document, the module, the 7.5 MB of wasm, the art — and HTTP keep-alive holds each of them open and idle long after its response has landed.

The websocket is the **last** thing the page asks for, after all of that. So one browser held all four spares through its own page load, the socket found none, and `relay.py` waited `--wait` seconds and closed the browser's connection with no response at all.

*Symptom:* the page loads perfectly and only the socket fails; more often on a slow link, never on a fast one, and never at all on localhost where there is no tunnel. It reads as a game that cannot reach its server, which sends you looking at `net::link_web` and at the server's `/ws` route, both of which are fine.

A spare now asks for its replacement the moment the relay wakes it, so the pool is a floor on how many are *waiting* rather than a ceiling on how many are open. Reproduced with six keep-alive requests in front of a websocket: the old agent starves at its shipped default of four and the new one does not.

The general shape, which is not about tunnels: **anything that pools connections for this game has to be sized in connections and not in people.** A player is one websocket and, before it, a page load's worth of HTTP that the browser holds open long after it is done with it.

## A key bound to a character is a key some keyboards do not have

Four of the client's keys are mnemonics — `R` to rotate, `F` to flip, `?` for help, `` ` `` for the shape reset — and binding them to the **character** rather than to the position is right: on Dvorak the `R` position types `p`, so a positional binding would hide rotate under a key nothing mentions and leave `r`, which the help screen names, doing nothing.

It is only right where the character can be typed. On a Cyrillic, Greek or Hebrew layout the `R` key types a letter that is not `r`, so rotate, flip and the help screen naming them had **no key at all**. And `~`, which the shape reset used to be, is a **dead key** on the Spanish, Portuguese and Nordic layouts: it produces no text on its own, waiting for a vowel to put a tilde over, so it was unreachable even where the alphabet is Latin and every label was in place.

`input::mnemonic` resolves all four in one place, tested without a window. Character first, US position as a fallback, and the fallback is narrow on purpose: `R` and `F` fall back **only when that key types something which is not a Latin letter**, so Dvorak keeps the character binding and nothing hides under an unnamed key. The shape reset moved to `` ` ``, which is the unshifted half of the same key and is never dead.

*Also:* a key bound by **position** needs its label read off the keyboard. The pan cluster already did that — it is `,aoe` on Dvorak — and the digit rows did not, so the help screen said `1-9` to a French keyboard whose top row types ``&é"'(-è_ç`` and to Programmer Dvorak, which needs shift for a digit at all.

*Not attempted:* input methods. Pinyin, Japanese and Korean compose text over several presses and hand it over as a finished string, so `to_text` says nothing during composition and single-key bindings are not a thing those layouts have. Direct-input layouts only.

## A socket object exists long before it connects, and may never connect

`web_sys::WebSocket::new` returns as soon as the URL parses. The connection is made afterwards, and if it never is, the object sits there — open to nobody, erroring never, closing never.

So `link.is_some()` is not "connected", and the HUD read it as though it were. Worse, the path that arrives from a **link** — `?room=`, or `/room/x` — set no deadline at all, where the menu's own "ask a server for its rooms" has had an eight-second one for as long as it has existed. A client that could not reach its server therefore sat in `Screen::Playing` on the world every session starts with, playing alone, saying "connected", with the failure visible only in the console.

*Symptom:* the game works. That is the whole problem: it is a different game from the one the link was for, and nothing on screen distinguishes them.

## The help screen named keys the keyboard did not have

Every label on the bar and the key list is for a key bound by **position**, so
only the label is in question — and the label was learned from what that key
typed when somebody pressed it, seeded with the US answer. Which means a
Dvorak player saw `WASD` until they had pressed all four, and the four they
pressed were labelled `,aoe`. AZERTY was worse: its unshifted digit row prints
`&é"'(-è_çà`, so ten stamp squares named ten keys that layout does not have.

Three separate faults, and they had one root.

**`Digit0` shifted was missing from the seed table**, so a row is drawn
all-or-nothing — half a row of guessed keycaps is a row nobody can read — and
the ten-wide stamp row could therefore never complete. The help screen fell
back to a hard-coded `1-9, 0` for ever, on every layout.

**The stamp squares did not ask at all.** `tool_hint` asked the keyboard and
`stamp_hint` beside it hard-coded `1`–`9` and `0`, so the help screen was right
about the digit row and the bar was not, which is worse than both being wrong.

**And the shifted row was named `shift + 1-4`** for a row that had run to six
the moment a capture and a "more" square joined the four tools. It is asked of
`hotbar::shifted` now, which is the list the keyboard actually uses.

The real fix is not to guess harder. **`navigator.keyboard.getLayoutMap()`
answers all of it at once**, with no press, no layout detection, and no table
of layouts to maintain — the browser reports what each physical key prints on
the keyboard in front of the player. It gives the unshifted value only, so the
shifted row is still seeded and still corrected on press, and it is Chromium-only
behind a permissions policy, so a `get` coming back undefined is a browser
without it rather than an error. Reached through `Reflect`, because `web-sys`
has no binding for it.

Native has no equivalent — winit reports what a key typed, and only once it has
been pressed — so there the seed and the learning are the whole answer.

## The keyboard answers are Chromium's, and most people are not on this machine

Two limits on the key-label work, both worth knowing before trusting it.

`navigator.keyboard.getLayoutMap()` is **Chromium-only**. Safari and Firefox do
not implement it, so a Mac user on Safari — which is a large share of Mac users
— falls back to the same seed-and-learn-on-press the native client uses. That
is not a regression, it is what everybody had before, and it means the fix is
"correct where it can be" rather than "correct everywhere".

And **modifier conventions differ by platform**, which is easy to miss when the
person writing and the person reviewing are both on Linux. Back is `alt+left`
on Linux and Windows and `cmd+[` on a Mac, so `ctrl+[` is bound here only where
it collides with nothing — on a Mac the browser already does it, and binding it
too would call `history.back()` beside the browser's own and go back twice.
`views::on_a_mac` asks the browser rather than `cfg!(target_os)`, which on a
wasm build says `unknown` and would be wrong for everybody.

## Escape did not work for anybody who had moved it

Caps lock mapped to escape is a common enough thing to do that it should have
been the first test, and it did nothing. The client bound `KeyCode::Escape`,
which is winit's **physical** key — where escape sits — and a keysym-level
remap keeps the key where it is and changes what it *means*. So the event
arrives as `physical_key: CapsLock`, `logical_key: Escape`, and a binding on
the position never fires.

Which is the same lesson as the layout labels, from the other side, and the
rule falls out of putting the two together:

**A key bound to what it *means* is bound logically. A key bound to where it
*sits* is bound physically.** Escape, shift and the arrows mean something, so
they are `NamedKey` now. The walk cluster and the digit row are a shape on the
board rather than a meaning, so they stay `KeyCode` — and *their* problem is
the label, which is why the layout map exists.

`App::on_key` carries both halves for this reason: `code` is where the key sits
and `named` is what it means, and a binding picks the one its own answer
depends on.

The general version of this is that defaults cannot be right, which is now
[a roadmap entry](planned.md#keys-the-player-chooses): three separate faults
have come out of this same place, each fixed by guessing better rather than by
letting the player say.
