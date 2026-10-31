# Building Mote

## Dependencies

- Rust 1.94 or later, through [rustup](https://rustup.rs) or the
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

Mote decodes H.264 with a library it loads at run time rather than building
in, so that the patent licenses stay with whoever provides the library. It
takes the first it finds:

1. FFmpeg's libavcodec, version 5 to 8. The Flatpak's GNOME runtime has it
   in its codecs-extra extension.
2. Cisco's OpenH264 2.6 from the system (`libopenh264.so.8`).
3. Cisco's OpenH264 downloaded from Cisco, from Settings, under Video. The
   download is checked against Cisco's published hash before it is kept.

Without any of them, servers send RemoteFX instead, which blurs small text,
and KRdp, which sends only H.264, refuses the session.

On Fedora, the system's OpenH264 comes from Fedora's Cisco repository;
`noopenh264`, a stand-in that decodes nothing, is installed by default:

```sh
sudo dnf swap noopenh264 openh264
```

On Debian and Ubuntu:

```sh
sudo apt install libopenh264-8
```

`MOTE_H264` set to `ffmpeg`, `openh264` or `cisco` tries that source alone,
`MOTE_FFMPEG_DIR` names a directory to load FFmpeg from, and `MOTE_OPENH264`
another OpenH264 library.

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

### The clipboard

`MOTE_TEST_CLIPBOARD=<text>` makes the live test offer the server that
text, with HTML and a picture beside it, and fetch whatever is copied on
the remote desktop while it runs (`MOTE_TEST_SECONDS`), files into
`MOTE_TEST_FETCH` (`/tmp/mote-fetch` by default). `MOTE_TEST_FILES` offers
files instead, as paths split by `:`. `MOTE_LOG=mote=debug` logs the
formats each side offers.

### Measuring performance

`MOTE_LOG=mote::stats=info`, for the app or the live test, logs what a
session's pictures cost every five seconds and when it ends: frames and
kilobytes a second, and as mean/p95/max milliseconds the time to decode an
update, to copy it out for the interface, and from decoding to the paint
that shows it. Without it nothing is timed.

A desktop that changes all the time, the same way every run, makes the
numbers comparable. The private GNOME Remote Desktop shows any program
given in `GRD_CLIENT`, such as GStreamer's moving test pattern:

```sh
GRD_CLIENT='gst-launch-1.0 videotestsrc pattern=smpte horizontal-speed=4 is-live=true
  ! video/x-raw,width=1920,height=1080,framerate=60/1 ! waylandsink fullscreen=true' \
  systemd-run --user --collect --unit=mote-grd -E GRD_CLIENT temp/grd/run.sh
MOTE_TEST_SERVER=localhost:3399 MOTE_TEST_SIZE=1920x1080 MOTE_TEST_SECONDS=12 \
  MOTE_LOG=mote::stats=info cargo test -- --ignored live --nocapture
```

`MOTE_TEST_SIZE` is the desktop to ask for, `MOTE_TEST_SECONDS` how long to
stay. `pattern=ball` changes a small part of the screen instead of all of it.
`MOTE_TEST_QUALITY` is the connection's codec choice: `best`, `avc420` or
`remotefx`. The log names the codec the server picked; the private GNOME
Remote Desktop has no GPU to encode H.264 on and sends progressive RemoteFX,
so H.264 is measured against xrdp or KRdp.
