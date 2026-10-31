# Mote

A remote desktop client for GNOME, written in Rust with GTK 4 and libadwaita,
for connecting to Linux machines that run an RDP server: GNOME Remote
Desktop, xrdp or KRdp.

Mote is in early development and does not connect to anything yet. It is
built in sections, each one finished and tested before the next starts; the
order and what each section holds is in [docs/ROADMAP.md](docs/ROADMAP.md).

## Goals

- Everything Remmina's RDP plugin does, in a GNOME-native interface.
- Sharp on HiDPI displays: the remote desktop follows the local scale factor,
  fractional scaling included, and resizes with the window.
- Clipboard sharing in both directions: text, rich text, images and files.
- Fast: hardware-friendly decoding of the RDP graphics pipeline, and no work
  on the main thread that the display does not need.
- Settings per connection, with defaults that suit a Linux server.

## Building

See [docs/BUILDING.md](docs/BUILDING.md). In short, with GTK 4 and
libadwaita development files installed:

```sh
cargo build
./target/debug/mote
```

## Documentation

Everything else is in [docs/](docs/README.md). To help, see
[docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).

## AI notice

Mote is built by a human maintainer who uses generative AI as a development
tool. The maintainer decides what gets built, reviews the results, tests every
release and signs off on everything that ships. The app itself contains no AI
and makes no requests to AI services.

Bug reports and pull requests are welcome whether or not AI tools were
involved, and everything merged is reviewed by a person.

## License

Mote is free software under the
[GNU Affero General Public License v3.0 or later](LICENSE).
