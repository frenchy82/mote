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

## Tests

```sh
cargo test
cargo clippy --all-targets
```
