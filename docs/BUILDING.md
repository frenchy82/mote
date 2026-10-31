# Building Mote

## Dependencies

- Rust 1.88 or later, through [rustup](https://rustup.rs) or the
  distribution.
- GTK 4.18 or later and libadwaita 1.7 or later, with their development
  files.
- gettext.

On Fedora:

```sh
sudo dnf install gcc gtk4-devel libadwaita-devel gettext-devel
```

On Debian and Ubuntu:

```sh
sudo apt install build-essential libgtk-4-dev libadwaita-1-dev gettext
```

### H.264

Mote decodes H.264 with Cisco's OpenH264 library, which it loads at run time
rather than building in, so that Cisco's patent license covers it. Without
it, servers send RemoteFX instead, which blurs small text, and KRdp, which
sends only H.264, refuses the session. Mote needs OpenH264 2.6
(`libopenh264.so.8`); `MOTE_OPENH264` names another library to load.

On Fedora, from Fedora's Cisco repository (`noopenh264`, a stand-in that
decodes nothing, is installed in its place by default):

```sh
sudo dnf swap noopenh264 openh264
```

On Debian and Ubuntu:

```sh
sudo apt install libopenh264-8
```

## Building and running

```sh
cargo build
./target/debug/mote
```

`MOTE_LOG` takes a [tracing filter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html),
for example `MOTE_LOG=mote=debug`.

A release build (`cargo build --release`) is slower to make and is only
needed for packaging or for judging performance.

## Pictures of the interface

With `MOTE_SHOWCASE=<file.png>` set, the app stages a view, writes a picture
of its window and quits, so a change to the interface can be checked without
anyone at the keyboard. `MOTE_SHOWCASE_OPEN` takes comma-separated steps:
`first` selects the first connection, `editor` opens the editor,
`settings` or `settings:<page>` opens Settings and pictures that window,
`about` opens About, `search:<text>` searches for that text, and
`connect:<name>` connects to the connection of that name, after which
`detach` moves its session to a window of its own, `fullscreen` makes it
fullscreen, and `scaling:resize`, `scaling:fit` or `scaling:actual` sets how
it is scaled. `delay:<seconds>` makes the steps after it wait.
Point `XDG_CONFIG_HOME` at a scratch directory to keep it away from your own
connections.

## The app icon

The icon's master is `data/icons/src/default.svg`, drawn on GNOME's 128 pixel
icon grid. After changing it, run `tools/gen-app-icons.py` (it needs
python3-gobject with librsvg and pycairo): it copies it into hicolor as the
scalable icon and renders the 256 and 512 pixel PNGs and the README artwork
in `data/repo/`. The symbolic icon in `data/icons/hicolor/symbolic/apps` is
drawn by hand. The build embeds both SVGs in the binary, so About shows the
icon without an install.

## Tests

```sh
cargo test
cargo clippy --all-targets
```

One test connects to a real server and is skipped unless asked for. It
expects a GNOME Remote Desktop, or any RDP server, with the user name and
password `mote`:

```sh
MOTE_TEST_SERVER=localhost:3399 cargo test -- --ignored live
```

`MOTE_TEST_USER` and `MOTE_TEST_PASSWORD` give another account, and
`MOTE_TEST_EXPECT=refused` expects the server to turn it down. `MOTE_LOG`
sets what it logs; `MOTE_LOG=mote::wire=trace` adds the start of every
message sent and received.
