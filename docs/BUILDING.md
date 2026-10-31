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
`settings` or `settings:<page>` opens Settings and pictures that window, and
`about` opens About.
Point `XDG_CONFIG_HOME` at a scratch directory to keep it away from your own
connections.

## The app icon

The icon's master is `data/icons/src/default.png`, a 1024 pixel square drawn
edge to edge. After changing it, run `tools/gen-app-icons.py` (it needs
ImageMagick): it writes the hicolor sizes, with the margin GNOME's icon grid
leaves around a square icon, and the README artwork in `data/repo/`. The
build embeds the hicolor sizes in the binary, so About shows the icon without
an install.

## Tests

```sh
cargo test
cargo clippy --all-targets
```
