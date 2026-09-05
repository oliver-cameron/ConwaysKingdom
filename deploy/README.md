# Deploying

Everything a host needs to run this, and nothing that picks one. The shape is settled and written up in [docs/server.md](../docs/server.md#deploying): the Rust server serves the page, the module and the websocket from **one origin**, and `cloudflared` beside it makes that origin reachable as `conwayskingdom.com` — one outbound connection to Cloudflare, TLS at the edge, no port forwarded and nothing of this listening on the open internet. Which machine it runs on is not decided, and nothing here decides it.

| | |
|---|---|
| [build.sh](build.sh) | the shipping build: the browser client, then the server, and it stops at the first failure |
| [conwayskingdom.service](conwayskingdom.service) | the systemd unit — a `conway` user, the state directory, SIGTERM and the save it triggers |
| [cloudflared.yml](cloudflared.yml) | the tunnel's one ingress rule, and the 404 under it |
| [env.example](env.example) | the API token, which is the only secret there is |

## From nothing

Anything that runs systemd and has a couple of gigabytes to build in. The commands below are Debian's; the shape is the same anywhere.

**A user for it, and a place to put it.** The service reads the checkout and writes one directory, so `conway` needs no home and no shell:

```
sudo useradd --system --shell /usr/sbin/nologin conway
sudo mkdir -p /opt/conwayskingdom && sudo chown "$USER" /opt/conwayskingdom
git clone https://github.com/oliver-cameron/ConwaysKingdom /opt/conwayskingdom
```

Owned by whoever builds rather than by `conway`, because building is not something the service does: it runs a binary and reads a checkout, and reading is all it ever needs.

**Rust and wasm-pack.** [rustup](https://rustup.rs), because the toolchain a distribution ships is usually older than the 1.87 `Cargo.toml` asks for, and a version too old is refused by name rather than by a parse error in the middle of the crate:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**Build.** Two minutes, most of it `wasm-opt` over the whole module:

```
cd /opt/conwayskingdom && deploy/build.sh
```

It prints the size of both things it made. The module is what every visitor downloads and should be around 7.5 MB; a binary whose size did not move between deploys is usually one that was not rebuilt.

**The token.** Only if something is going to play through the API — without it `/api` is not mounted at all, which is the right default for a server nobody is writing an engine against:

```
sudo install -D -m 600 -o root -g root deploy/env.example /etc/conwayskingdom/env
openssl rand -hex 32                  # the token
sudoedit /etc/conwayskingdom/env      # paste it in after CK_API_TOKEN=
```

**The unit.** Read it before installing it: the paths in it are the checkout and the state directory, and everything else is explained where it stands.

```
sudo cp deploy/conwayskingdom.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now conwayskingdom
journalctl -u conwayskingdom -f
```

The startup log is worth reading once. It names the rooms it found, their shapes and what is in them, says whether `/api` is mounted, and **warns that the bind is loopback only** — which is the intention here and nowhere else. The warning exists because that flag is otherwise indistinguishable from a network fault; behind a tunnel it is what stops anything but the tunnel reaching the port, and it is why `CF-Connecting-IP` is worth believing at all.

```
curl -sS localhost:8080/healthz          # ok: 1 rooms, 0 connections
```

**The tunnel.** `cloudflared` is Cloudflare's own package. The tunnel is created once and its credentials stay on the host:

```
cloudflared tunnel login
cloudflared tunnel create conwayskingdom
cloudflared tunnel route dns conwayskingdom conwayskingdom.com
sudo cp deploy/cloudflared.yml /etc/cloudflared/config.yml
sudoedit /etc/cloudflared/config.yml      # the tunnel id, in both places
sudo cloudflared service install
```

`route dns` is the one step here that touches DNS, and it is a command somebody runs rather than anything this repository does. The credentials file it writes is the tunnel: whoever has it can be this origin, so it stays on the host and out of the repository.

```
curl -sS https://conwayskingdom.com/healthz
```

Then open the page. If `navigator.gpu` exists — it does over HTTPS and does not over plain IP — the client gets WebGPU rather than falling back to WebGL2, which is half of why the edge is worth having.

## Rooms

A room is declared on the command line or made from the menu, and under systemd there is no console to type `new` at: `StandardInput=null` means the server runs headless, which is the ordinary case for a server nobody is sitting at. So a room that should always exist goes in the unit as another `--room NAME`, and everything else people make for themselves, capped at `--max-rooms`. The API can seat bots and play them but cannot make a room.

## Deploying a change

```
git -C /opt/conwayskingdom pull && /opt/conwayskingdom/deploy/build.sh
sudo systemctl restart conwayskingdom
```

The restart is a SIGTERM, which is a save of every room and then an exit, so a restart is not a thing to be nervous about. Players see the socket close and their client carries on offline until they reconnect.

What a visitor sees is a page that is `no-cache` and files under `pkg/` and `assets/` that are held for an hour — so the page is new immediately and the module can be up to an hour stale unless the edge is told otherwise. **Purge the cache after a deploy**, from the Cloudflare dashboard or the API, or wait the hour out. The reasoning is in [docs/server.md](../docs/server.md#deploying).

Nothing has to be configured at the edge for the loading bar. Cloudflare may compress the module on the way through, which takes the `Content-Length` the bar wants; the server sends the same number again as `X-Content-Length`, which is what the page reads first. Turning compression off for `.wasm` in a Compression Rule is a fine thing to do for the bytes' sake, and the bar works either way.

## What it keeps, and how to back it up

Everything is under `/var/lib/conwayskingdom/rooms`: one `<name>.ckw` a world, and four tables beside them — `people.jsonl`, `profiles.jsonl`, `stamps.jsonl`, `games.jsonl`. Those four are the part that cannot be rebuilt. A world can be started again; a person's identity, their name, the patterns they saved and what they have played cannot, and `people.jsonl` holds the secrets clients present to be themselves, so it is the file an attacker who reached the disk would want.

A nightly tarball is enough:

```
tar -czf "/var/backups/ck-$(date +%F).tar.gz" -C /var/lib/conwayskingdom rooms
```

Taken while the server runs, which is safe for the worlds and *nearly* safe for the tables. A `.ckw` is written beside itself and renamed, so a tarball catches the old file or the new one and never half of one; the four `.jsonl` tables are written in place, so a backup taken in the middle of one — a window of milliseconds, a few times an hour — can catch it short. Take the tarball when nobody is on, or stop the service for it, if that matters more than the convenience. What a backup misses either way is up to thirty seconds of play, which is the periodic save's interval and the same thing a power cut costs.

A restore is the tarball put back with the server stopped, and stopped matters: a running server holds every world in memory and its next save would write straight over what you restored.

```
sudo systemctl stop conwayskingdom
sudo tar -xzf /var/backups/ck-2026-09-05.tar.gz -C /var/lib/conwayskingdom
sudo chown -R conway:conway /var/lib/conwayskingdom/rooms
sudo systemctl start conwayskingdom
```

## What is not here

No machine is provisioned and no DNS record is written by anything in this directory. The host is still an open question — the Beelink at home or a small VPS in Sydney — and the tunnel is what makes that question postponable: the same files bring either one up, and nothing in the client knows where the server is.
