//! Saving and loading a server's world.
//!
//! A hand-rolled binary format rather than a serialisation crate: a chunk is
//! already a flat byte array, so the file is mostly a memcpy, and owning the
//! format means a version byte can gate a migration later.

use std::io::{self, Read, Write};
use std::path::Path;

use crate::sim::{Chunk, Player, PlayerId, World, WorldKind, CHUNK_N};

const MAGIC: &[u8; 4] = b"CKW\0";
/// Bumped to 3 when a cell went from four bytes to two: the chunk bytes are a
/// raw cast, so a version 2 file read as version 3 is not a corrupt world but
/// a plausible one, twice as large and wrong in every cell. The version is
/// what turns that into a refusal.
///
/// And to 4 when a player record gained its token. Without it a restart
/// would hand every returning player a new number and leave their ground
/// standing there, theirs and unreachable.
/// Bumped to 5 when ownership on a dead cell went from a flag to a level: the
/// owner byte's split moved, so a version 4 file read as version 5 is a
/// plausible world with every square owned by the wrong player at the wrong
/// strength. There is no honest migration -- a flag carries no level.
const VERSION: u8 = 5;

const KIND_INFINITE: u8 = 0;
const KIND_TOROIDAL: u8 = 1;

pub struct Snapshot {
    pub world: World,
    pub players: Vec<Player>,
    pub tick: u64,
}

/// Refuse a file whose cells are a different width from this build's, rather
/// than reading it as garbage.
fn check_shape(chunk_n: u32, cell_bytes: u8) -> io::Result<()> {
    if chunk_n != CHUNK_N as u32 || cell_bytes as usize != size_of::<crate::sim::Cell>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "saved with {chunk_n}x{chunk_n} chunks of {cell_bytes}-byte cells; \
                 this build uses {CHUNK_N}x{CHUNK_N} of {}",
                size_of::<crate::sim::Cell>()
            ),
        ));
    }
    Ok(())
}

pub fn save(path: &Path, world: &World, players: &[Player], tick: u64) -> io::Result<()> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(CHUNK_N as u8);
    out.push(size_of::<crate::sim::Cell>() as u8);

    match world.kind() {
        WorldKind::Infinite => {
            out.push(KIND_INFINITE);
        }
        WorldKind::Toroidal { rows, cols } => {
            out.push(KIND_TOROIDAL);
            out.extend_from_slice(&rows.to_le_bytes());
            out.extend_from_slice(&cols.to_le_bytes());
        }
    }
    out.extend_from_slice(&tick.to_le_bytes());

    // Only chunks holding life are worth writing; the rest is implied.
    let mut chunks: Vec<_> = world
        .stored()
        .into_iter()
        .filter(|(_, c)| !c.is_empty())
        .collect();
    chunks.sort_unstable_by_key(|&(coord, _)| coord);

    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for (coord, chunk) in chunks {
        out.extend_from_slice(&coord.0.to_le_bytes());
        out.extend_from_slice(&coord.1.to_le_bytes());
        out.extend_from_slice(chunk.as_bytes());
    }

    out.extend_from_slice(&(players.len() as u32).to_le_bytes());
    for p in players {
        out.push(p.id.0);
        let name = p.name.as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&p.last_seen.to_le_bytes());
        out.extend_from_slice(&p.value.to_le_bytes());
        let token = p.token.as_bytes();
        out.extend_from_slice(&(token.len() as u16).to_le_bytes());
        out.extend_from_slice(token);
    }

    // Write beside the target and rename, so a crash mid-write cannot leave a
    // half-written world where the real one was.
    let tmp = path.with_extension("tmp");
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::File::create(&tmp)?.write_all(&out)?;
    std::fs::rename(&tmp, path)
}

pub fn load(path: &Path) -> io::Result<Snapshot> {
    let mut buf = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut buf)?;
    let mut r = Reader { buf: &buf, at: 0 };

    if r.take(4)? != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a world file"));
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world file version {version}, expected {VERSION}"),
        ));
    }
    let chunk_n = r.u8()? as u32;
    let cell_bytes = r.u8()?;
    check_shape(chunk_n, cell_bytes)?;

    let mut world = match r.u8()? {
        KIND_INFINITE => World::infinite_empty(),
        // `_empty`, and the asymmetry with the line above is how this hid: the
        // infinite arm was already empty and the toroidal one was not.
        // `World::toroidal` seeds a glider, and only chunks holding something
        // are written, so every chunk the file did not mention kept it --
        // which meant loading a saved torus invented five cells nobody placed,
        // in chunk zero, on every restart.
        KIND_TOROIDAL => World::toroidal_empty(r.i32()?, r.i32()?),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown world kind {other}"),
            ))
        }
    };
    let tick = r.u64()?;

    let chunk_bytes = CHUNK_N * CHUNK_N * size_of::<crate::sim::Cell>();
    let count = r.u32()?;
    for _ in 0..count {
        let coord = (r.i32()?, r.i32()?);
        let cells = r.take(chunk_bytes)?;
        let chunk: &Chunk = bytemuck::from_bytes(cells);
        world.put_chunk(coord, *chunk);
    }

    let mut players = Vec::new();
    for _ in 0..r.u32()? {
        let id = PlayerId(r.u8()?);
        let len = r.u16()? as usize;
        let name = String::from_utf8_lossy(r.take(len)?).into_owned();
        let mut p = Player::new(id, name);
        p.last_seen = r.u64()?;
        p.value = r.i32()?;
        let len = r.u16()? as usize;
        p.token = String::from_utf8_lossy(r.take(len)?).into_owned();
        // Nobody is connected to a world that has just been read off a disk.
        //
        // `Player::new` is what a player *joins* with, and joining means being
        // online, so a player rebuilt from a file came back marked connected
        // and stayed that way. A player who is online cannot be returned to by
        // their token -- that check is what stops two tabs being one player --
        // so every player who was in the room when it was saved found their
        // token refused on the next run and joined as somebody new, beside
        // territory they could see and could not build on.
        //
        // Set here rather than at the call site because this is where a
        // `Player` who never joined is built, and it is the file's business to
        // hand back what it holds: the file holds a player's standing in a
        // world, not the state of a connection that ended when the process
        // did. `online` is not in the format for the same reason.
        p.online = false;
        players.push(p);
    }

    Ok(Snapshot { world, players, tick })
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.buf.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "world file ends mid-record")
        })?;
        let out = &self.buf[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Cell, PlayerId};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join(format!("ck-persist-{tag}-{}.ckw", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A world comes back as what was written, and nothing else.
    ///
    /// Only chunks holding something are saved, so anything a fresh world
    /// starts with survives in every chunk the file did not mention. Building
    /// the torus with `World::toroidal`, which seeds a glider, therefore made
    /// every load invent five cells nobody had placed.
    #[test]
    fn a_loaded_world_holds_only_what_was_saved() {
        for (tag, world) in [
            ("infinite", World::infinite_empty()),
            ("torus", World::toroidal_empty(4, 4)),
        ] {
            let path = scratch(tag);
            save(&path, &world, &[], 0).unwrap();
            let back = load(&path).unwrap();
            assert_eq!(back.world.live_cells(), Vec::<(i32, i32)>::new(), "{tag}");
            assert_eq!(back.world.kind(), world.kind(), "{tag}");
            let _ = std::fs::remove_file(&path);
        }
    }

    /// And what *was* saved comes back, so the check above is not passing by
    /// losing everything.
    #[test]
    fn what_was_saved_comes_back() {
        let path = scratch("roundtrip");
        let mut world = World::toroidal_empty(3, 3);
        world.set_cell_at(20, 5, Cell::alive(PlayerId(2)));
        save(&path, &world, &[Player::new(PlayerId(2), "alice")], 77).unwrap();

        let back = load(&path).unwrap();
        assert_eq!(back.tick, 77);
        assert_eq!(back.world.live_cells(), vec![(20, 5)]);
        assert_eq!(back.world.cell_at(20, 5).unwrap().player(), PlayerId(2));
        assert_eq!(back.players.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    /// A player read out of a file is not connected, whatever they were doing
    /// when it was written. The flag is not in the format at all — it is a
    /// fact about a socket, and the socket ended with the process.
    #[test]
    fn a_player_restored_from_a_file_is_not_online() {
        let path = scratch("offline");
        let mut alice = Player::new(PlayerId(2), "alice");
        alice.token = "aaaa".into();
        assert!(alice.online, "a player who joins is online");
        save(&path, &World::toroidal_empty(3, 3), &[alice], 0).unwrap();

        let back = load(&path).unwrap();
        assert!(!back.players[0].online, "and one read off a disk is not");
        assert_eq!(back.players[0].token, "aaaa", "but the token still is theirs");
        let _ = std::fs::remove_file(&path);
    }
}
