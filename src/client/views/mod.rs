//! The views, and the interface they are drawn with.
//!
//! **Two screens, and what they share.** [`game`] is the world and everything
//! drawn over it — the HUD, the hotbar, the lobby, the key list, the stamp
//! library; [`menu`] is what comes before it. What is left here is what both
//! need: [`theme`] is every colour and measurement, [`words`] every string,
//! [`hue`] a player's colour, [`icons`] the sprite sheet as egui wants it, and
//! [`record`] the games this client has played.
//!
//! A module moves down into a screen the moment only that screen uses it, and
//! back up the moment two do. The one thing that crossed was a `Victory` in a
//! sentence, which the lobby showed and the creation form borrowed; it is
//! `words::describe` now, because a helper one screen borrows from another is
//! the thing keeping two screens in one module.
//!
//! Every view answers with a [`Shown`]: what it covered, and what it was told.
//! [`Views`] is the egui plumbing they share.
//!
//! Lives under `client` rather than `render` because what to show is policy,
//! not plumbing — `render` stays generic wgpu and winit, and knows nothing
//! about egui. The client feeds it events and hands it the pass.
//!
//! egui draws into the same render pass as the world, so there is no second
//! surface and no compositing step.
//!
//! Input is translated from winit by hand rather than by `egui-winit`, which
//! does not compile for wasm32 at 0.36: `egui::DroppedFile` declares
//! `bytes_async` under `cfg(wasm32)` and egui-winit's implementation only
//! provides the native `bytes`. Translating here keeps one code path for both
//! targets, and a HUD needs only pointer, wheel and modifiers — the IME and
//! clipboard handling that egui-winit exists for is not in play.

/// A button whose label starts at the left, at the full width it was given.
///
/// **Left, because a column of buttons is a list**, and a list is read down its
/// left edge. Centred labels put every word at a different place across the
/// column, so the eye has to hunt for each one instead of running down them —
/// which is what a menu of full-width buttons is for. `Atom::grow` after the
/// text is what pushes it over; egui centres by default.
pub fn wide(
    ui: &mut egui::Ui,
    label: egui::RichText,
    height: f32,
    fill: egui::Color32,
) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), height],
        egui::Button::new((label, egui::Atom::grow())).fill(fill),
    )
}

pub mod face;
pub mod game;
pub mod glyph;
pub mod hue;
pub mod icons;
pub mod menu;
pub mod record;
pub mod theme;
pub mod words;

use crate::render::context::GpuState;

pub struct Views {
    ctx: egui::Context,
    /// Events gathered since the last frame.
    events: Vec<egui::Event>,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    /// What each panel covered last frame, in points.
    ///
    /// Consumption is decided from this rather than from egui's own
    /// `wants_pointer`, which depends on interaction state this integration
    /// feeds by hand: if any of that is wrong the answer sticks true and the
    /// world silently stops receiving clicks, with nothing to show why. A
    /// rectangle can be reasoned about, printed, and seen.
    ///
    /// One rectangle per panel, never their union. Two panels' union is their
    /// bounding box, and the panels are in opposite corners — the box between
    /// a HUD at the top left and a hotbar at the bottom centre is most of the
    /// window, so the world only received the strip beyond it.
    claimed: Vec<egui::Rect>,
    /// The finger the interface is following, and whether it began on the
    /// interface. One finger: egui's pointer is a pointer, and a second is a
    /// pinch, which is the world's business.
    finger: Option<(u64, bool)>,
    /// True while a widget is being dragged, so a drag that leaves the panel
    /// still belongs to the panel.
    dragging_widget: bool,
    start: f64,
    /// What each digit key types with shift held.
    ///
    /// Starts as what the **common layout** types, and is corrected the moment
    /// a key says otherwise. Both halves matter: seeding means the great
    /// majority see the right label on the first frame without pressing
    /// anything, and correcting means somebody on Programmer Dvorak — where
    /// the digits are shifted to begin with, so shift and `1` is not `!` — is
    /// only shown the wrong one until they use it.
    ///
    /// What each physical key **prints on the keyboard in front of the
    /// player**, learned as they press them.
    ///
    /// Guessed rather than asked, because there is no portable way to ask: on
    /// the web `navigator.keyboard.getLayoutMap()` would answer properly and
    /// is Chrome-only and asynchronous, and natively there is nothing.
    ///
    /// What is *not* guessed is the binding. A key bound by position is the
    /// same key everywhere and only its **label** is in question — which is
    /// the whole reason this exists, and why a key bound by character needs no
    /// entry here at all: `R` is `R` wherever it is, so the label is right by
    /// construction.
    ///
    /// Keyed on the shift state as well as the position, because one key
    /// prints two things.
    learned: std::collections::HashMap<(winit::keyboard::KeyCode, bool), String>,
    pub theme: theme::Theme,
    renderer: egui_wgpu::Renderer,
}

/// What these keys print on the layout most people have, so the great majority
/// see the right thing on the first frame. Every one of them is corrected by
/// [`Views::learned`] the moment a key disagrees.
///
/// Only the keys something on screen names. A label nobody is shown is a guess
/// nobody can be wrong about.
fn common_labels() -> std::collections::HashMap<(winit::keyboard::KeyCode, bool), String> {
    use winit::keyboard::KeyCode as K;
    /// The digit row, in the order the bar and the help screen read it.
    ///
    /// Ten, not nine. `Digit0` was missing from the shifted half, so a row of
    /// ten keycaps could never be complete and the help screen fell back to a
    /// hard-coded `1-9, 0` for ever.
    const DIGITS: [(K, &str, &str); 10] = [
        (K::Digit1, "1", "!"),
        (K::Digit2, "2", "@"),
        (K::Digit3, "3", "#"),
        (K::Digit4, "4", "$"),
        (K::Digit5, "5", "%"),
        (K::Digit6, "6", "^"),
        (K::Digit7, "7", "&"),
        (K::Digit8, "8", "*"),
        (K::Digit9, "9", "("),
        (K::Digit0, "0", ")"),
    ];
    const WALKING: [(K, &str); 4] =
        [(K::KeyW, "W"), (K::KeyA, "A"), (K::KeyS, "S"), (K::KeyD, "D")];
    DIGITS
        .into_iter()
        .flat_map(|(code, plain, shifted)| {
            [((code, false), plain.to_string()), ((code, true), shifted.to_string())]
        })
        .chain(WALKING.into_iter().map(|(code, label)| ((code, false), label.to_string())))
        .collect()
}

/// What the browser says each physical key prints, once it has answered.
///
/// A global because the answer arrives on a promise and the thing that wants
/// it is behind a `RefCell` on the app. Drained rather than read, so the merge
/// happens once.
#[cfg(target_arch = "wasm32")]
static FROM_THE_BROWSER: std::sync::Mutex<Option<Vec<(String, String)>>> =
    std::sync::Mutex::new(None);

/// **Ask the keyboard what it prints, rather than waiting to be told.**
///
/// Every label here is bound by position and learned from a press, which means
/// a Dvorak player sees `WASD` on the help screen until they have pressed all
/// four — and the four they press are labelled `,aoe`, which is the answer
/// they needed before they pressed anything. AZERTY is worse: its unshifted
/// digit row prints ``&é"'(-è_çà``, so ten stamp squares were labelled with
/// ten keys that layout does not have.
///
/// `navigator.keyboard.getLayoutMap()` answers all of it at once and needs no
/// press, no layout detection and no table of layouts to keep — which is the
/// point, because a table would be a guess about which layouts exist and this
/// is the browser reporting the one in front of the player. It gives the
/// **unshifted** value only, so the shifted row is still seeded and still
/// corrected on press.
///
/// Reached through `Reflect` rather than through `web-sys`, which has no
/// binding for it: it is behind a permissions policy and is Chromium-only so
/// far, and a `get` that comes back undefined is a browser without it rather
/// than an error.
#[cfg(target_arch = "wasm32")]
pub fn ask_the_keyboard() {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else { return };
    let navigator = window.navigator();
    let Ok(keyboard) = js_sys::Reflect::get(&navigator, &"keyboard".into()) else { return };
    if keyboard.is_undefined() || keyboard.is_null() {
        log::info!(
            "no navigator.keyboard here, so key labels are learned as keys are pressed. \
             That is every browser but Chromium's."
        );
        return;
    }
    let Ok(get) = js_sys::Reflect::get(&keyboard, &"getLayoutMap".into()) else { return };
    let Ok(get) = get.dyn_into::<js_sys::Function>() else { return };
    let Ok(promise) = get.call0(&keyboard) else { return };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else { return };

    wasm_bindgen_futures::spawn_local(async move {
        let Ok(map) = wasm_bindgen_futures::JsFuture::from(promise).await else { return };
        let Ok(get) = js_sys::Reflect::get(&map, &"get".into()) else { return };
        let Ok(get) = get.dyn_into::<js_sys::Function>() else { return };
        let mut found = Vec::new();
        for (code, shift) in named_keys() {
            if shift {
                continue;
            }
            let name = format!("{code:?}");
            if let Ok(value) = get.call1(&map, &name.as_str().into()) {
                if let Some(text) = value.as_string() {
                    if !text.is_empty() {
                        found.push((name, text));
                    }
                }
            }
        }
        log::info!("the browser named {} of this keyboard's keys", found.len());
        if let Ok(mut slot) = FROM_THE_BROWSER.lock() {
            *slot = Some(found);
        }
    });

    // **And again whenever the layout changes.** Somebody testing this is
    // almost certainly *switching* to Dvorak with the page already open, which
    // is the one case a query at startup cannot answer: it asked while the
    // keyboard was still QWERTY and there was nothing to ask again. Chromium
    // fires `layoutchange` on `navigator.keyboard` for exactly this.
    let again = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        log::info!("the keyboard layout changed; asking again");
        ask_the_keyboard();
    });
    let listened = js_sys::Reflect::get(&keyboard, &"addEventListener".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .map(|f| f.call2(&keyboard, &"layoutchange".into(), again.as_ref()));
    if listened.is_none() {
        log::debug!("no layoutchange to listen for");
    }
    again.forget();
}

/// Whether this is a Mac, for the labels whose key is spelled differently
/// there.
///
/// **Because most people are not on the machine this was written on.** The
/// modifier conventions genuinely differ — a Mac's back is `cmd+[` where
/// everywhere else it is `alt+left` — so a key list that names one of them
/// names the wrong key for a large share of whoever is reading it.
///
/// Asked of the browser rather than of `cfg!(target_os)`, which on a wasm
/// build says `unknown` and would be wrong for everybody. `userAgentData` is
/// the modern spelling and `platform` the one Safari and Firefox still answer,
/// so both are tried; failing both, the majority answer is "not a Mac".
#[cfg(target_arch = "wasm32")]
pub fn on_a_mac() -> bool {
    let Some(window) = web_sys::window() else { return false };
    let navigator = window.navigator();
    let modern = js_sys::Reflect::get(&navigator, &"userAgentData".into())
        .ok()
        .and_then(|data| js_sys::Reflect::get(&data, &"platform".into()).ok())
        .and_then(|p| p.as_string());
    let said = modern.or_else(|| navigator.platform().ok()).unwrap_or_default();
    let said = said.to_ascii_lowercase();
    said.contains("mac") || said.contains("iphone") || said.contains("ipad")
}

/// Natively, the build knows.
#[cfg(not(target_arch = "wasm32"))]
pub fn on_a_mac() -> bool {
    cfg!(target_os = "macos")
}

/// Every physical key the screen puts a name to, so one list decides what is
/// asked about rather than each caller guessing.
///
/// **Bound by position, so only the label is in question.** A key bound by
/// character needs no entry — `R` is `R` wherever it is — which is why this is
/// the digit row and the walk cluster and nothing else.
pub fn named_keys() -> Vec<(winit::keyboard::KeyCode, bool)> {
    use winit::keyboard::KeyCode as K;
    const DIGITS: [K; 10] = [
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
        K::Digit0,
    ];
    DIGITS
        .into_iter()
        .flat_map(|code| [(code, false), (code, true)])
        .chain([K::KeyW, K::KeyA, K::KeyS, K::KeyD].map(|code| (code, false)))
        .collect()
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use winit::keyboard::KeyCode as K;

    /// **Every key the screen names has a label from the first frame.**
    ///
    /// A row is drawn all-or-nothing — half a row of keycaps with the rest
    /// guessed is a row nobody can read — so one missing entry silently falls
    /// the whole help screen back to a hard-coded string. `Digit0` shifted was
    /// missing, so the ten-wide stamp row could never complete and the screen
    /// said `1-9, 0` for ever, on every layout, including the ones where that
    /// names ten keys the keyboard does not have.
    #[test]
    fn every_named_key_starts_with_a_label() {
        let labels = common_labels();
        for key in named_keys() {
            assert!(labels.contains_key(&key), "{key:?} has no label to start from");
        }
    }

    /// The seed is the US answer, which is the layout most people have and is
    /// what every one of these is corrected away from — by a press, or by the
    /// browser saying what this keyboard actually prints.
    #[test]
    fn the_seed_is_the_layout_it_was_written_on() {
        let labels = common_labels();
        assert_eq!(labels.get(&(K::Digit1, false)).map(String::as_str), Some("1"));
        assert_eq!(labels.get(&(K::Digit1, true)).map(String::as_str), Some("!"));
        assert_eq!(labels.get(&(K::Digit0, false)).map(String::as_str), Some("0"));
        assert_eq!(labels.get(&(K::Digit0, true)).map(String::as_str), Some(")"));
        assert_eq!(labels.get(&(K::KeyW, false)).map(String::as_str), Some("W"));
    }

    /// The names asked of the browser are the ones `KeyboardEvent.code` uses,
    /// because that is what `getLayoutMap` is keyed by. `Debug` on a
    /// `KeyCode` happens to spell them, and this is what says so out loud —
    /// it is a coincidence worth a test rather than a comment.
    #[test]
    fn a_keycode_spells_its_own_web_name() {
        assert_eq!(format!("{:?}", K::KeyW), "KeyW");
        assert_eq!(format!("{:?}", K::Digit0), "Digit0");
        assert_eq!(format!("{:?}", K::Space), "Space");
    }
}

/// Borrowed out so the match arm above reads as one thing. `KeyEvent::state`
/// is a field, and taking a reference to it inside the pattern would move the
/// event out of the borrow.
fn state_of(event: &winit::event::KeyEvent) -> &winit::event::ElementState {
    &event.state
}

/// The egui key a winit key means, where egui has one.
///
/// Only what a text field and a menu need: editing, moving the caret,
/// confirming, and leaving. Not the letters and digits — those reach a field
/// as `Text`, and egui only wants them as `Key` for shortcuts, which this
/// integration has no clipboard to serve.
fn egui_key(key: &winit::keyboard::Key) -> Option<egui::Key> {
    use winit::keyboard::{Key, NamedKey};
    let named = match key {
        Key::Named(named) => named,
        _ => return None,
    };
    Some(match named {
        NamedKey::Enter => egui::Key::Enter,
        NamedKey::Tab => egui::Key::Tab,
        NamedKey::Space => egui::Key::Space,
        NamedKey::Backspace => egui::Key::Backspace,
        NamedKey::Delete => egui::Key::Delete,
        NamedKey::Escape => egui::Key::Escape,
        NamedKey::ArrowLeft => egui::Key::ArrowLeft,
        NamedKey::ArrowRight => egui::Key::ArrowRight,
        NamedKey::ArrowUp => egui::Key::ArrowUp,
        NamedKey::ArrowDown => egui::Key::ArrowDown,
        NamedKey::Home => egui::Key::Home,
        NamedKey::End => egui::Key::End,
        NamedKey::PageUp => egui::Key::PageUp,
        NamedKey::PageDown => egui::Key::PageDown,
        NamedKey::Insert => egui::Key::Insert,
        _ => return None,
    })
}

/// Hand every texture change to the renderer, then empty the delta.
///
/// Emptying it is not tidiness. `TexturesDelta` asserts on drop that it is
/// empty, and reading it through a reference leaves it full, so the assert
/// fires however faithfully the deltas were handled. Split out so the emptying
/// can be tested without a GPU, since the bug is in the bookkeeping rather
/// than in the upload.
fn consume_textures(delta: &mut egui::TexturesDelta, mut sink: impl FnMut(Change<'_>)) {
    // A texture can arrive as several partial updates in one frame, so each id
    // carries a list rather than a single delta.
    for (id, deltas) in &delta.set {
        for d in deltas {
            sink(Change::Set(*id, d));
        }
    }
    for id in &delta.free {
        sink(Change::Free(*id));
    }
    delta.clear();
}

/// One texture change. A single callback rather than two, because both need
/// the renderer and two closures cannot borrow it at once.
enum Change<'a> {
    Set(egui::TextureId, &'a egui::epaint::ImageDelta),
    Free(egui::TextureId),
}

/// Whether any panel covers the pointer.
///
/// A list rather than one rectangle, and this is why: the panels sit in
/// different corners, and anything that folds them into a single rectangle
/// first claims all the world between them.
fn claims(panels: &[egui::Rect], pointer: egui::Pos2) -> bool {
    panels.iter().any(|panel| panel.contains(pointer))
}

/// **IBM Plex, both faces**, in place of the two egui ships with.
///
/// Sans for everything and Mono for the figures on the bar — and the mono is
/// the reason there is a decision here at all. A number that changes every
/// generation in a proportional face is a number whose width changes with it,
/// so the label under it slides about and the eye re-finds it every time; in a
/// monospaced one the digits sit in columns and only the digits move. The same
/// argument is why the key list is monospaced.
///
/// Bundled rather than asked of the system: a browser has no font to lend, and
/// a client that looked different on every machine would make every screenshot
/// of a bug a screenshot of a different client. They are `assets/fonts/`, under
/// the SIL Open Font License in `LICENSE.txt` beside them.
fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("plex", &include_bytes!("../../../assets/fonts/IBMPlexSans-Regular.ttf")[..]),
        ("plex-mono", &include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf")[..]),
        (glyph::FAMILY, ICON_FONT),
    ] {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(FontData::from_static(bytes)));
    }
    // In front of what egui ships, rather than instead of it: the fallbacks
    // are what draw a character Plex does not have, and a missing glyph box is
    // worse than a glyph in the wrong face.
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "plex".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".into());
    // **A family of its own**, and nothing else in it. Put among the
    // proportional fallbacks instead, an icon's codepoint would be looked for
    // in Plex first — which has nothing in the private use area, so it would
    // work — and a *letter* would be looked for in the icon font, which is
    // fine until somebody subsets it. Its own family means an icon is only
    // ever asked of the font that has icons.
    fonts.families.insert(FontFamily::Name(glyph::FAMILY.into()), vec![glyph::FAMILY.into()]);
    ctx.set_fonts(fonts);
}

/// The icon face, embedded the way the two text faces are — **cut down at
/// build time to the glyphs [`glyph`] names**.
///
/// Phosphor regular, MIT, in `PHOSPHOR-LICENSE.txt` beside the whole face in
/// `assets/`. What ships is `build.rs`'s output: a few kilobytes against the
/// four hundred and eighty the face weighs, and it cannot go stale, because
/// the list it is cut to is read out of the module that does the naming.
pub const ICON_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icons.ttf"));

/// Every codepoint a font has a glyph for, read out of its `cmap`.
///
/// **So a test can check the font rather than trust it.** A named icon whose
/// glyph is not in the file draws a blank box, silently, on whichever screen
/// uses it — which is exactly what a subset built from a stale list produces,
/// and exactly the failure `tools/subset-icons.sh` could otherwise introduce.
///
/// Formats 4 and 12, which is what a modern font's Unicode table is; anything
/// else is skipped rather than guessed at, and the test asserts it found a
/// plausible number so a silent nought cannot pass.
#[cfg(test)]
fn font_codepoints(font: &[u8]) -> std::collections::HashSet<u32> {
    let mut out = std::collections::HashSet::new();
    let be16 = |at: usize| -> Option<u32> {
        Some(u16::from_be_bytes(font.get(at..at + 2)?.try_into().ok()?) as u32)
    };
    let be32 = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(font.get(at..at + 4)?.try_into().ok()?))
    };

    // The table directory: a 12-byte header, then 16 bytes per table.
    let Some(tables) = be16(4) else { return out };
    let mut cmap = None;
    for i in 0..tables as usize {
        let at = 12 + i * 16;
        if font.get(at..at + 4) == Some(b"cmap") {
            cmap = be32(at + 8).map(|o| o as usize);
        }
    }
    let Some(cmap) = cmap else { return out };

    let Some(subtables) = be16(cmap + 2) else { return out };
    for i in 0..subtables as usize {
        let rec = cmap + 4 + i * 8;
        let Some(offset) = be32(rec + 4) else { continue };
        let sub = cmap + offset as usize;
        match be16(sub) {
            // Segment mapping to delta values: four parallel arrays.
            Some(4) => {
                let Some(seg2) = be16(sub + 6) else { continue };
                let segs = (seg2 / 2) as usize;
                let ends = sub + 14;
                let starts = ends + seg2 as usize + 2;
                let deltas = starts + seg2 as usize;
                let ranges = deltas + seg2 as usize;
                for s in 0..segs {
                    let (Some(end), Some(start), Some(delta), Some(range)) = (
                        be16(ends + s * 2),
                        be16(starts + s * 2),
                        be16(deltas + s * 2),
                        be16(ranges + s * 2),
                    ) else {
                        continue;
                    };
                    if start == 0xFFFF {
                        continue;
                    }
                    for c in start..=end {
                        let glyph = if range == 0 {
                            (c + delta) & 0xFFFF
                        } else {
                            let at = ranges + s * 2 + range as usize + (c - start) as usize * 2;
                            match be16(at) {
                                Some(0) | None => continue,
                                Some(g) => (g + delta) & 0xFFFF,
                            }
                        };
                        if glyph != 0 {
                            out.insert(c);
                        }
                    }
                }
            }
            // Segmented coverage: groups of (start, end, first glyph).
            Some(12) => {
                let Some(groups) = be32(sub + 12) else { continue };
                for g in 0..groups as usize {
                    let at = sub + 16 + g * 12;
                    let (Some(start), Some(end)) = (be32(at), be32(at + 4)) else { continue };
                    for c in start..=end.min(start + 0xFFFF) {
                        out.insert(c);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// What a view drew, and what it was told while drawing it.
///
/// **One shape for every view**, because they all answer the same two
/// questions and used to answer them five ways: a bare `bool`, a bare
/// `Option<Rect>`, `(Rect, Did)`, `(Did, Rect)`, and one struct of its own.
/// At a call site that is an order to remember and a `.0` to decode, and the
/// two that differ only in order are the ones that get swapped silently.
///
/// `did` is the view's own enum rather than a shared one: what a hotbar can be
/// told and what a lobby can be told have nothing in common, and folding them
/// into one type would mean every caller matching arms that cannot happen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shown<T> {
    /// What it covered, so a click on it does not also reach the world.
    /// `None` for a view that drew nothing this frame.
    pub rect: Option<egui::Rect>,
    pub did: T,
}

impl<T: Default> Shown<T> {
    /// Drew nothing and was told nothing, which is a screen this view is not
    /// on rather than a failure.
    pub fn nowhere() -> Self {
        Self::default()
    }
}

impl<T> Shown<T> {
    pub fn new(rect: impl Into<Option<egui::Rect>>, did: T) -> Self {
        Self { rect: rect.into(), did }
    }
}

/// **A panel over the world**: a titled frame, anchored, with a way out.
///
/// Four views drew this by hand — the key list, the stamp library, the rules
/// switches and a profile — and by the fourth they had come to differ in their
/// padding, their width and whether the title had a close button beside it,
/// none of which anybody decided. A panel is a panel; what belongs to a view
/// is what goes *in* one.
///
/// A struct because the argument list was heading for seven, which is the
/// point at which their order is the thing most likely to be got wrong — the
/// same reason [`super::game::lobby::Look`] is one.
pub struct Panel<'a> {
    /// egui's id for the area. Distinct per panel, or two of them share a
    /// position and fight over it.
    pub id: &'static str,
    pub title: &'a str,
    /// Where it sits. Centred for anything you open and read; a corner for
    /// anything that hangs off the control that opened it.
    pub at: egui::Align2,
    pub offset: [f32; 2],
}

impl Panel<'_> {
    /// Centred, which is what a panel you open and read wants.
    pub fn middle<'a>(id: &'static str, title: &'a str) -> Panel<'a> {
        Panel { id, title, at: egui::Align2::CENTER_CENTER, offset: [0.0, 0.0] }
    }
}

/// Draw one, and say what its body was told.
///
/// **It owns being closed.** `open` goes false when the button is pressed, so
/// a view whose only answer was "I was shut" needs no answer type at all —
/// which is what `help::Did` and `profile::Did` each were, and a third of what
/// the rules panel returned.
pub fn panel<T>(
    ctx: &egui::Context,
    theme: &theme::Theme,
    what: Panel<'_>,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui) -> T,
) -> Shown<T> {
    let (p, m) = (theme.palette, theme.metrics);
    let mut told = None;
    let area = egui::Area::new(what.id.into()).anchor(what.at, what.offset).show(ctx, |ui| {
        egui::Frame::new()
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.line))
            .corner_radius(m.rounding)
            .inner_margin(m.panel_padding * 1.4)
            .show(ui, |ui| {
                // One width for every panel, from the theme, which is where a
                // measurement belongs. Two of these carried a hard-coded 280
                // and two asked the theme, so a phone got a panel sized for a
                // desktop half the time.
                ui.set_width(theme.panel_width(ctx.content_rect().width()));
                ui.horizontal(|ui| {
                    ui.heading(what.title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(words::CLOSE).clicked() {
                            *open = false;
                        }
                    });
                });
                ui.add_space(m.item_spacing);
                told = Some(body(ui));
            });
    });
    Shown::new(area.response.rect, told.expect("the body runs inside the frame"))
}

/// The shapes a frame of interface produced.
///
/// Deliberately holds no `TexturesDelta`. egui panics if one is dropped with
/// deltas unapplied, and a frame is not always drawn — the surface can report
/// Skip while it settles, which is exactly when the font atlas first arrives.
/// Uploading textures when they are produced rather than when they are drawn
/// removes the failure case instead of guarding it.
pub struct Output {
    primitives: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
}

impl Views {
    pub fn new(gpu: &GpuState) -> Self {
        let ctx = egui::Context::default();
        let theme = theme::Theme::default();
        // Once, not per frame: egui keeps its style between passes.
        install_fonts(&ctx);
        theme.apply(&ctx);
        Self {
            ctx,
            theme,
            events: Vec::new(),
            pointer: egui::Pos2::ZERO,
            modifiers: egui::Modifiers::default(),
            claimed: Vec::new(),
            finger: None,
            dragging_widget: false,
            start: 0.0,
            learned: {
                // Asked once, here, so the answer is on its way before the
                // first frame that needs it.
                #[cfg(target_arch = "wasm32")]
                ask_the_keyboard();
                common_labels()
            },
            // No depth buffer and one sample, matching the world's pipeline;
            // egui has to agree with it because they share a pass.
            renderer: egui_wgpu::Renderer::new(
                &gpu.device,
                gpu.config.format,
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    depth_stencil_format: None,
                    ..Default::default()
                },
            ),
        }
    }

    /// The egui context, for anything that has to be registered with it —
    /// a texture, say — before a frame is built.
    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Whether the interface, rather than the world, should get the pointer.
    pub fn wants_pointer(&self) -> bool {
        self.dragging_widget || claims(&self.claimed, self.pointer)
    }

    /// What this key prints on the keyboard in front of the player.
    ///
    /// `None` for a key nobody has pressed and nothing guessed at, which is
    /// the honest answer: better a label that is missing than one that is
    /// wrong, since the whole point of showing it is that somebody who does
    /// not know the key is reading it.
    pub fn label(&self, code: winit::keyboard::KeyCode, shift: bool) -> Option<&str> {
        self.learned.get(&(code, shift)).map(String::as_str)
    }

    /// Take what the browser said about this keyboard, if it has answered.
    ///
    /// Called each frame and does nothing on all but one of them. A press is
    /// still stronger evidence and still wins, because a press arrives later
    /// and overwrites; both are right, so which wins does not matter.
    /// Whether anything changed, so a caller caching what the keys print knows
    /// when to rebuild rather than doing it every frame.
    pub fn take_what_the_browser_said(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(found) = FROM_THE_BROWSER.lock().ok().and_then(|mut s| s.take()) else {
                return false;
            };
            for (code, shift) in named_keys() {
                if shift {
                    continue;
                }
                let name = format!("{code:?}");
                if let Some((_, text)) = found.iter().find(|(key, _)| *key == name) {
                    self.learned.insert((code, false), text.clone());
                }
            }
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// What shift and this digit type here, for the squares on the hotbar.
    pub fn shifted_digit(&self, digit: u32) -> Option<&str> {
        use winit::keyboard::KeyCode as K;
        const DIGITS: [K; 9] = [
            K::Digit1,
            K::Digit2,
            K::Digit3,
            K::Digit4,
            K::Digit5,
            K::Digit6,
            K::Digit7,
            K::Digit8,
            K::Digit9,
        ];
        self.label(*DIGITS.get((digit as usize).checked_sub(1)?)?, true)
    }

    /// What this digit types on its own, which is what a stamp's square shows.
    ///
    /// Zero included, unlike [`Self::shifted_digit`]: the tenth stamp is `0`,
    /// and the shifted row has never run that far.
    pub fn plain_digit(&self, digit: u32) -> Option<&str> {
        use winit::keyboard::KeyCode as K;
        const DIGITS: [K; 10] = [
            K::Digit0,
            K::Digit1,
            K::Digit2,
            K::Digit3,
            K::Digit4,
            K::Digit5,
            K::Digit6,
            K::Digit7,
            K::Digit8,
            K::Digit9,
        ];
        self.label(*DIGITS.get(digit as usize)?, false)
    }

    /// Whether the interface, rather than the world, should get the keyboard.
    ///
    /// Asked of egui directly, unlike [`Self::wants_pointer`], and the reason
    /// the two differ is worth stating. A pointer is claimed by *where it is*,
    /// which this integration can answer from the rectangles each panel
    /// reported, and answering it that way avoids depending on interaction
    /// state fed by hand. The keyboard is claimed by *what has focus*, which
    /// is egui's own bookkeeping and not something a rectangle can express:
    /// there is nowhere else to ask.
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }

    /// Translate a window event. Returns whether the world should ignore it.
    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent, scale: f32) -> bool {
        use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                // egui works in points; winit reports physical pixels.
                self.pointer = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                self.events.push(egui::Event::PointerMoved(self.pointer));
                // Never withheld. The client tracks the cursor for its own
                // hover and drag handling, and a position is not an action.
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.events.push(egui::Event::PointerGone);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = (match button {
                    MouseButton::Left => Some(egui::PointerButton::Primary),
                    MouseButton::Right => Some(egui::PointerButton::Secondary),
                    MouseButton::Middle => Some(egui::PointerButton::Middle),
                    _ => None,
                }) else {
                    return false;
                };
                self.events.push(egui::Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: *state == ElementState::Pressed,
                    modifiers: self.modifiers,
                });
                self.wants_pointer()
            }
            // **A finger is a pointer, and egui was never told.**
            //
            // Nothing here translated a touch, so on a touchscreen egui
            // received no press at all and every button in the interface was
            // dead — the menu, the lobby, the hotbar, the library. The world
            // was fine, because the client reads `App::on_touch` itself, which
            // is exactly why it went unnoticed: the game worked and only the
            // things drawn on top of it did not.
            //
            // One finger, the first one down. egui has no use for a second —
            // its pointer is a pointer — and the second finger is a pinch,
            // which is the world's business.
            WindowEvent::Touch(touch) => {
                use winit::event::TouchPhase;
                let at =
                    egui::pos2(touch.location.x as f32 / scale, touch.location.y as f32 / scale);
                match touch.phase {
                    TouchPhase::Started if self.finger.is_none() => {
                        // Moved first, so that what egui decides about this
                        // press is decided at the place it happened. Without
                        // it the press lands wherever the pointer was left,
                        // which on a touchscreen is wherever the last one
                        // ended.
                        self.pointer = at;
                        self.events.push(egui::Event::PointerMoved(at));
                        self.events.push(egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: self.modifiers,
                        });
                        // Whether this finger belongs to the interface is
                        // decided **once, here**, and remembered: a drag that
                        // began on a button must not become a drag on the
                        // world halfway through because it left the button.
                        let claimed = claims(&self.claimed, at);
                        self.finger = Some((touch.id, claimed));
                        claimed
                    }
                    _ if self.finger.map(|(id, _)| id) != Some(touch.id) => false,
                    TouchPhase::Moved => {
                        self.pointer = at;
                        self.events.push(egui::Event::PointerMoved(at));
                        self.finger.is_some_and(|(_, claimed)| claimed)
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        let claimed = self.finger.is_some_and(|(_, claimed)| claimed);
                        self.events.push(egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: self.modifiers,
                        });
                        // Gone, not merely up. A finger that lifts leaves
                        // nothing hovering, and a button left looking hovered
                        // under no finger is a button that appears stuck.
                        self.events.push(egui::Event::PointerGone);
                        self.finger = None;
                        claimed
                    }
                    TouchPhase::Started => false,
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, d) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(p) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(p.x as f32 / scale, p.y as f32 / scale),
                    ),
                };
                self.events.push(egui::Event::MouseWheel {
                    unit,
                    delta: d,
                    // winit gives no phase for a wheel, and egui's own advice
                    // when it is unknown is Move.
                    phase: egui::TouchPhase::Move,
                    modifiers: self.modifiers,
                });
                self.wants_pointer()
            }
            WindowEvent::ModifiersChanged(state) => {
                let s = state.state();
                // **`command` is the platform's shortcut key, not control.**
                //
                // egui routes every text-editing shortcut off `command` —
                // select-all, copy, cut, paste, undo. Wired to control alone, a
                // Mac gets none of them: cmd+A selects nothing and cmd+C copies
                // nothing, while control+A, which on macOS means "go to the
                // start of the line", selects everything instead. That is the
                // whole of highlighting not behaving the way it does in a
                // browser — the modifier the platform actually uses was never
                // reported. [`on_a_mac`] already existed for the key list and
                // answers this too, including in a browser, where the build's
                // own `target_os` says `unknown` for everybody.
                let mac = on_a_mac();
                self.modifiers = egui::Modifiers {
                    alt: s.alt_key(),
                    ctrl: s.control_key(),
                    shift: s.shift_key(),
                    mac_cmd: mac && s.super_key(),
                    command: if mac { s.super_key() } else { s.control_key() },
                };
                false
            }
            // Typing. Nothing needed it until there was a field to type into,
            // and a menu is two text fields, so this is where the keyboard
            // stops being the game's alone.
            //
            // Two events, not one. `Text` is what a character key contributes
            // to a field; `Key` is what backspace, the arrows, enter and escape
            // do to one. A key that produces text produces both, because egui
            // routes shortcuts off `Key` and content off `Text`, and a field
            // that got only text could never be corrected.
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let pressed = *state_of(event) == ElementState::Pressed;
                if let Some(key) = egui_key(&event.logical_key) {
                    self.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }
                // Watch what shift and a digit actually types, so the hotbar
                // can label its keys with what is on the keyboard rather than
                // with what a US layout would have printed.
                // **Every key, not just the ones on the bar.** It used to
                // learn the nine shifted digits and nothing else, so the help
                // screen went on saying WASD to somebody on Dvorak whose pan
                // keys are `,aoe` — a list of keys that is wrong is worse than
                // no list, and this is the list that exists to be read by
                // somebody who does not know the keys yet.
                if pressed
                    && let (winit::keyboard::PhysicalKey::Code(code), Some(text)) =
                        (event.physical_key, event.text.as_ref())
                {
                    let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !typed.is_empty() {
                        self.learned.insert((code, self.modifiers.shift), typed);
                    }
                }
                // Only on the way down, and never while a command modifier is
                // held: ctrl+V is a paste, and inserting a literal "v" beside
                // it is the sort of thing that only shows up in somebody's
                // password field.
                if pressed
                    && !self.modifiers.command
                    && !self.modifiers.alt
                    && let Some(text) = event.text.as_ref()
                {
                    // Control characters arrive here as text -- enter is
                    // "\r", escape is "\u{1b}" -- and inserting them into a
                    // field puts an invisible character in a room name.
                    let printable: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !printable.is_empty() {
                        self.events.push(egui::Event::Text(printable));
                    }
                }
                self.wants_keyboard()
            }
            _ => false,
        }
    }

    /// Build the frame, upload whatever textures it produced, and hand back
    /// the shapes to draw.
    pub fn run(
        &mut self,
        gpu: &GpuState,
        now: f64,
        build: impl FnOnce(&egui::Context) -> Vec<egui::Rect>,
    ) -> Output {
        if self.start == 0.0 {
            self.start = now;
        }
        let pixels_per_point = gpu.scale_factor;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    gpu.size.0 as f32 / pixels_per_point,
                    gpu.size.1 as f32 / pixels_per_point,
                ),
            )),
            time: Some(now - self.start),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.ctx.set_pixels_per_point(pixels_per_point);

        self.ctx.begin_pass(input);
        self.claimed = build(&self.ctx);
        canary(&self.ctx, gpu, pixels_per_point);
        let mut full = self.ctx.end_pass();

        self.dragging_widget = self.ctx.egui_is_using_pointer();

        let renderer = &mut self.renderer;
        consume_textures(&mut full.textures_delta, |change| match change {
            Change::Set(id, delta) => renderer.update_texture(&gpu.device, &gpu.queue, id, delta),
            Change::Free(id) => renderer.free_texture(&id),
        });

        let primitives = self.ctx.tessellate(full.shapes, pixels_per_point);
        report_geometry(&self.ctx, gpu, pixels_per_point, &primitives);
        Output { primitives, pixels_per_point }
    }

    /// Record the interface into the pass the world was just drawn into.
    pub fn render(
        &mut self,
        gpu: &GpuState,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut wgpu::RenderPass<'static>,
        output: &Output,
    ) {
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.size.0, gpu.size.1],
            pixels_per_point: output.pixels_per_point,
        };
        self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            encoder,
            &output.primitives,
            &descriptor,
        );
        self.renderer.render(pass, &output.primitives, &descriptor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure that took the client down on startup: egui hands over a
    /// `TexturesDelta`, and dropping one that still holds deltas asserts. It
    /// happened on the first frames, when the font atlas arrives and the
    /// surface is still reporting Skip, so nothing had drawn yet.
    ///
    /// No GPU here: the bug is in the bookkeeping, and the bookkeeping is what
    /// this checks.
    #[test]
    fn a_frames_textures_are_consumed_and_the_delta_emptied() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 240.0),
            )),
            ..Default::default()
        });
        egui::Area::new("test".into()).show(&ctx, |ui| ui.label("Player 1"));
        let mut full = ctx.end_pass();

        // Drawing text builds a font atlas, so there is something to lose.
        assert!(
            !full.textures_delta.is_empty(),
            "no textures produced, so this would pass for the wrong reason"
        );

        let mut uploaded = 0;
        consume_textures(&mut full.textures_delta, |change| {
            if matches!(change, Change::Set(..)) {
                uploaded += 1;
            }
        });
        assert!(uploaded > 0, "the delta should have reached the renderer");
        assert!(
            full.textures_delta.is_empty(),
            "handling the deltas is not enough; the delta must be emptied too"
        );
        // Dropping `full` here is the actual assertion: it panics if not empty.
    }

    /// The panels were folded into one rectangle with `Rect::union`, which is
    /// their bounding box. A HUD at the top left and a hotbar at the bottom
    /// centre bound most of the window between them, so the world only ever
    /// received the strip to the right of the hotbar — and every gesture
    /// anywhere else was swallowed with nothing on screen to say why.
    #[test]
    fn panels_claim_themselves_and_not_the_space_between_them() {
        let hud = egui::Rect::from_min_size(egui::pos2(14.0, 14.0), egui::vec2(220.0, 300.0));
        let hotbar = egui::Rect::from_min_size(egui::pos2(600.0, 700.0), egui::vec2(110.0, 50.0));
        let panels = [hud, hotbar];

        assert!(claims(&panels, egui::pos2(100.0, 100.0)), "on the HUD");
        assert!(claims(&panels, egui::pos2(640.0, 720.0)), "on the hotbar");

        // Between the two, and the case the union got wrong.
        assert!(!claims(&panels, egui::pos2(400.0, 400.0)), "open world");
        assert!(!claims(&panels, egui::pos2(100.0, 690.0)), "below the HUD");
        assert!(!claims(&panels, egui::pos2(590.0, 60.0)), "above the hotbar");
        assert!(
            claims(&[hud.union(hotbar)], egui::pos2(400.0, 400.0)),
            "the union swallowed open world, which is the bug this replaced"
        );
    }
}

/// What egui was handed, and what came back — said once, and again whenever it
/// changes.
///
/// A frame can be built, tessellated, uploaded and still reach nobody.
/// `egui-wgpu` drops any primitive whose clip rect, multiplied by
/// `pixels_per_point` and clamped into `size_in_pixels`, comes out zero-sized
/// — see `ScissorRect::new` in its `renderer.rs` — and it does that without a
/// word. So an interface that is entirely scissored away is indistinguishable
/// from one that was never drawn, and neither the console nor a panic hook has
/// anything to say about it.
///
/// These are the numbers that decide it: the surface in physical pixels, the
/// points-per-pixel the two ends of the pair were built with, the rectangle
/// egui laid out in, and whether anything came out the far side. On a change
/// only — they change when the window does and not otherwise, and a line a
/// frame is a line nobody reads.
fn report_geometry(
    ctx: &egui::Context,
    gpu: &GpuState,
    pixels_per_point: f32,
    primitives: &[egui::ClippedPrimitive],
) {
    use std::cell::Cell;
    thread_local! {
        static LAST: Cell<Option<(u32, u32, u32, bool)>> = const { Cell::new(None) };
    }
    // The factor is a float and this is an identity, so it is compared as
    // thousandths rather than by an equality nobody should be writing on a
    // `f32`.
    let now = (gpu.size.0, gpu.size.1, (pixels_per_point * 1000.0) as u32, primitives.is_empty());
    if LAST.with(|last| last.replace(Some(now))) == Some(now) {
        return;
    }
    log::info!(
        "egui: surface {:?} at {pixels_per_point} ppp, screen {:?} points, content {:?}, \
         {} primitive(s), first clip {:?}",
        gpu.size,
        (gpu.size.0 as f32 / pixels_per_point, gpu.size.1 as f32 / pixels_per_point),
        ctx.content_rect(),
        primitives.len(),
        primitives.first().map(|p| p.clip_rect),
    );
}

/// A shape nothing else draws, over everything, when the address says `debug`.
///
/// **For the one question that cannot be answered from the front.** An
/// interface that does not appear is either an interface that was never built
/// or one that was built and did not reach the surface, and the two look
/// identical: a window of clear colour either way. Nothing in the console
/// separates them either — `egui-wgpu` drops a primitive it cannot scissor or
/// cannot find a texture for without a word, and a screen that produced no
/// shapes produces no complaint about it.
///
/// So: one rectangle, in a colour no theme here uses, at `Order::Foreground`,
/// built by the integration rather than by any screen. If it appears and the
/// screen behind it does not, then what egui produces reaches the glass and
/// the fault is in what that screen produced. If it does not appear either,
/// nothing egui produces reaches anything, and the renderer is where to look.
///
/// The numbers ride along so the same load says what the geometry was without
/// anybody opening an inspector, which matters on a machine where opening one
/// is not a given. The bar is painted before the text, because a font atlas
/// that never arrived would take every glyph on the page with it and the bar
/// is the half that does not depend on one.
fn canary(ctx: &egui::Context, gpu: &GpuState, pixels_per_point: f32) {
    if !debugging() {
        return;
    }
    let painter =
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("canary")));
    let content = ctx.content_rect();
    let rect =
        egui::Rect::from_min_size(content.min + egui::vec2(8.0, 8.0), egui::vec2(380.0, 26.0));
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(255, 0, 170));
    painter.text(
        rect.min + egui::vec2(7.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!("{:?} at {pixels_per_point} ppp, content {:?}", gpu.size, content.size()),
        egui::FontId::monospace(11.0),
        egui::Color32::BLACK,
    );

    // **One square per layer order, because the bar above only proves that a
    // painter reaches the glass.** Every panel in this client is an `Area` or
    // a `Window`, which is a different path: egui can render an area invisible
    // of its own accord — a sizing pass does it deliberately, and an
    // unfinished fade does it by arithmetic — and it does none of that to a
    // layer painted directly. So three areas, identical but for their order,
    // with the fade off so that one variable is the only variable.
    //
    // Left to right: background, middle, foreground. All three and areas draw
    // at every order, so a screen that does not is about its own content. Only
    // the right two and `Order::Background` is the fault, which is where the
    // menu used to be and no other panel is. None of them and areas do not
    // draw at all, whatever a bare painter manages.
    for (i, order) in [egui::Order::Background, egui::Order::Middle, egui::Order::Foreground]
        .into_iter()
        .enumerate()
    {
        let at = rect.left_bottom() + egui::vec2(i as f32 * 30.0, 6.0);
        egui::Area::new(egui::Id::new(("canary", i)))
            .order(order)
            .fixed_pos(at)
            .fade_in(false)
            .movable(false)
            .interactable(false)
            .show(ctx, |ui| {
                let (square, _) =
                    ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                ui.painter().rect_filled(square, 0.0, egui::Color32::from_rgb(255, 0, 170));
            });
    }
}

/// Whether this client was asked for the canary. Read once: the client
/// rewrites its own address as you move between screens, and a flag that
/// stopped applying the moment `route::show` dropped the query would be a flag
/// nobody could keep hold of.
fn debugging() -> bool {
    use std::cell::Cell;
    thread_local! {
        static ASKED: Cell<Option<bool>> = const { Cell::new(None) };
    }
    ASKED.with(|asked| {
        if let Some(known) = asked.get() {
            return known;
        }
        let known = asked_for_debug();
        asked.set(Some(known));
        known
    })
}

/// `?debug` anywhere in the query, so it composes with the links that are
/// already in use — `/?room=main&debug` goes where it always went and says so.
#[cfg(target_arch = "wasm32")]
fn asked_for_debug() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|query| query.contains("debug"))
}

/// Native has no address to put it in.
#[cfg(not(target_arch = "wasm32"))]
fn asked_for_debug() -> bool {
    std::env::var_os("CK_DEBUG").is_some()
}
