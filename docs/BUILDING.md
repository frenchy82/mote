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
`connect:<name>` connects to the connection of that name and pictures its
session window.
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

With `MOTE_TEST_PASSWORD` set to anything else, it expects the server to
refuse the password.
