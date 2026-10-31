# Roadmap

Mote is built in sections. Each section ends with something that builds,
runs and has been tried against a real Linux RDP server before the next one
starts, and each can change shape once the one before it has been used.

## Servers

Every section that touches the protocol is checked against:

- **GNOME Remote Desktop** (GNOME 46 or later), both kinds: Desktop Sharing in
  a user's session, and Remote Login, the system service that signs a user in
  and then hands the client to the new session with a Server Redirection PDU.
  It requires the graphics pipeline, and NLA.
- **xrdp** 0.10 or later, with and without the graphics pipeline (H.264 and
  RemoteFX), and with its PulseAudio and PipeWire sound modules.
- **KRdp** on KDE Plasma 6.

## Protocol engine

[IronRDP](https://github.com/Devolutions/IronRDP), the RDP stack Devolutions
writes in Rust. It has a client connector with TLS and CredSSP (NLA), the
graphics pipeline with OpenH264 decoding, the clipboard, display control,
sound, drive redirection, smart cards and RD Gateway, all as separate crates.
Mote names those crates one by one (`ironrdp-connector` 0.12,
`ironrdp-session` 0.13 and the rest of that release), because the `ironrdp`
umbrella crate pins older versions of them. Each session runs on a thread of
its own, and nothing in the engine needs C bindings.

Where IronRDP falls short today, Mote works around it, and each workaround
is to go upstream:

- GNOME Remote Desktop's Server Redirection PDU carries version 1 in its
  Share Control Header, as everything FreeRDP builds does; IronRDP accepts
  only 0 there. Mote rewrites the version before IronRDP reads the PDU.
- The handover a redirection leads to needs RDSTLS, which IronRDP's
  connector doesn't offer. Mote asks for it and does its exchange, then
  lets the connector carry on.
- IronRDP's sound channel answers the server's Training PDU with eight
  bytes less than the `wPackSize` the server sent; GNOME Remote Desktop
  checks the size and sends no sound after it. Mote answers the training
  itself.
- IronRDP's backend for shared folders reads xrdp's requests more strictly
  than Windows and FreeRDP do. Mote answers the directory listing classes
  it leaves out, turns xrdp's forward slashes into backslashes, lets every
  handle delete, and removes an entry when the handle that marked it
  closes rather than the last one, as xrdp leaves a handle open.

FreeRDP 3, the engine Remmina uses, was the alternative. It supports more
devices (USB, serial and parallel ports), at the cost of a C API surface, a
callback-driven event loop and a larger Flatpak. Where IronRDP comes up short
on something a section needs, that section adds it upstream.

## Sections

| # | Section | State |
| --- | --- | --- |
| 1 | Foundation | Done |
| 2 | Connections | Done |
| 3 | First session | Done |
| 4 | HiDPI and resizing | Testing |
| 5 | Clipboard | Testing |
| 6 | Graphics pipeline | Testing |
| 7 | Audio | Testing |
| 8 | Device redirection | Testing |
| 9 | Network | |
| 10 | Session controls | |
| 11 | Settings and polish | |
| 12 | Packaging | |

### 1. Foundation

The repository, its conventions and tooling, and an app that opens a window:
relm4 on GTK 4 and libadwaita, gettext for translations, tracing for logs.

### 2. Connections

- The connection profile: host, port, user name, domain, and every
  per-connection setting the later sections add, stored as TOML in the user's
  config directory.
- Passwords in the Secret Service (the GNOME keyring, or the secret portal
  inside Flatpak), never in the profile.
- The connection list: search, groups, last-used order, and a search field
  that takes `host[:port]`: Enter on an address nothing matches starts a new
  connection to it. Connecting straight from it comes with section 3.
- The connection editor, as a libadwaita dialog with preferences pages.
- Import of `.rdp` files and of Remmina's `.remmina` profiles, and export
  to `.rdp`.

### 3. First session

- Connecting: TCP, TLS, NLA, and server redirection, which GNOME Remote Login
  depends on.
- Certificate trust on first use, with the fingerprint shown, and a warning
  when a known server's certificate changes.
- A credentials prompt when the profile has none or the server refuses them.
- The session view: frames into a texture, redrawing only damaged regions.
- Keyboard input as scancodes taken from the hardware keycode, so the remote
  layout decides what a key types; pointer, buttons, smooth and discrete
  scrolling; the server's cursor shapes.
- Disconnect, reconnect, and readable errors that name the likely cause on a
  Linux server and where to fix it: GNOME Remote Login without credentials,
  the wrong port, a server without a certificate, xrdp set to the old RDP
  security, outdated TLS, a password from the wrong place, and the reasons
  a server gives for ending a session, such as another connection taking
  it over.
- xrdp signs in with the profile's account, without its own login screen.
- Where a session lives: in the main window beside the sidebar, with the
  connection's row marked; moved to a window of its own and back; and
  fullscreen, with the header shown again at the screen's top edge.

Built, and tried against a headless GNOME Remote Desktop: the certificate
question, sign-in and a refused password, typing and clicking on the remote
desktop, and a server that stops, isn't running, or closes every connection
at once. Then against real servers on Fedora 44: GNOME Remote Login, through
the handover to its login screen and on to the user's session; xrdp 0.10.6
with Xfce, signing in without its login screen, and set to the old RDP
security or without a certificate; and KRdp 6.6.4, which turns the session
down until H.264 arrives in section 6.

GNOME Remote Desktop refuses a client without the graphics pipeline, so its
basics arrived here rather than in section 6: the pipeline's channel, with
the codecs IronRDP decodes without H.264.

### 4. HiDPI and resizing

- The session sized to the window and resized with it (Display Control,
  MS-RDPEDISP).
- Desktop and device scale factors taken from the monitor, fractional scales
  included, and updated when the window moves between monitors.
- Scaling modes: resize the remote desktop, fit to the window, or one remote
  pixel per device pixel.
- Fullscreen.

Built: the desktop is asked for in device pixels with the screen's scale,
and Display Control resizes it with the window, a moment after the window
settles; the pointer is drawn at the same scale. The scaling mode is chosen
per connection, in the editor or the session's menu. Fullscreen arrived with
section 3. Tried at 1×, 1.25× and 2× against a headless GNOME Remote
Desktop, and resizing against GNOME Remote Login and xrdp, which reopens its
graphics channel after a resize. Not yet tried: moving a window between
screens of different scales.

### 5. Clipboard

Text, rich text, images and files in both directions (MS-RDPECLIP), through
GDK's clipboard, with a per-connection switch.

Built on IronRDP's clipboard channel. What is copied on either side is
offered to the other and fetched only when something pastes it: text, HTML,
pictures (PNG here; PNG and DIB on the wire) and files with their folders.
Files pasted here from a remote desktop are fetched into the cache
directory, which the host sees from inside the Flatpak too, and cleared out
when the app next starts. Wayland gives a window the clipboard only while
it has the focus, so something copied elsewhere reaches the server once the
session's window is focused again. The switch is in the editor, and comes
in and out with `.rdp` files (`redirectclipboard`) and Remmina profiles
(`disableclipboard`).

Tried against all three servers, which differ:

| Server | Text | HTML | Pictures | Files |
| --- | --- | --- | --- | --- |
| GNOME Remote Desktop | Both ways | Neither way | Both ways | Both ways, with folders |
| xrdp | Both ways | Neither way | Both ways | Both ways, without folders |
| KRdp | Both ways | Neither way | Neither way | Neither way |

- GNOME Remote Desktop converts HTML with FreeRDP's WinPR, whose conversion
  from Windows' HTML Format to `text/html` can't succeed in WinPR 3.32, the
  version Fedora 44 has: HTML fails for every client, and the plain text
  beside it is what pastes. It fails a request that comes while it answers
  another, so Mote asks for one thing at a time. It gives its applications a
  picture as PNG when the client offers PNG, and as BMP otherwise, so Mote
  offers both.
- xrdp offers text and BMP pictures, nothing richer, and leaves folders out
  when files are copied either way. It passes Windows' CRLF line endings on
  to its applications, where GNOME Remote Desktop and KRdp turn them into
  LF, so Mote sends text with the LF endings it has. When what was copied
  there is gone before it is pasted here, xrdp never answers; a paste waits
  30 seconds at most.
- KRdp shares plain text only, and offers a picture as empty text. It
  announces back what it has just taken from the client; an announcement
  within a second of that, holding nothing new, is taken as the echo and
  leaves the local clipboard alone.

Copying a picture file to GNOME Remote Login froze Carbon's screen. It runs
Copyous, a clipboard history extension that previews a copied picture with
GdkPixbuf on GNOME Shell's own thread. GNOME Remote Desktop serves the
file from a FUSE mount that asks the client for each read as it comes, 4
kilobytes per round trip, and Carbon is on Wi-Fi: the shell waited for
the whole picture to cross. Any client would do the same. Copyous leaves a
file alone that matches its `file-preview-exclusion-patterns`, such as
`file:///run/user/*/gnome-remote-desktop/**`.

Tried in the app against the headless GNOME Remote Desktop, with a GTK
application on each side, and through the engine against all three. Not
yet tried: the Flatpak, where local files come through the file transfer
portal.

### 6. Graphics pipeline

- The graphics pipeline (MS-RDPEGFX), which section 3 started: H.264
  decoding, as AVC420 and AVC444.
- Decoding off the main thread, upload straight to a GPU texture, frames paced
  to the monitor's refresh.
- Quality presets that set color depth, codecs and the server's visual
  effects, with a custom option.
- A measurement harness, so performance changes are compared by numbers.

Built so far: H.264, offered as AVC444 and AVC420, through FFmpeg, the
system's OpenH264, or Cisco's OpenH264 downloaded on request from Settings,
all loaded at run time; FFmpeg is what the Flatpak's GNOME runtime provides,
and was tried inside it. Decoding runs on the session's own thread.

What each server sends, as measured: KRdp sends only AVC420, and turns down
a client without H.264. xrdp sends AVC420 when it has an encoder (Fedora's
Xfce install comes with the stand-in `noopenh264`, and then xrdp sends
lossless Planar for still pictures and RemoteFX for motion). GNOME Remote
Desktop sends AVC444 or AVC420 only where it can encode on the GPU; Carbon
and the headless test server can't, and get progressive RemoteFX whatever
the client offers.

The measurement harness is in place (BUILDING.md, under Measuring
performance). Its first numbers, at 1920×1080 on the development machine:

| Step | Time per frame |
| --- | --- |
| H.264, FFmpeg's own decoder (the Flatpak's) | 2.6 ms |
| H.264, OpenH264 (and Fedora's FFmpeg, which uses it) | 4 ms |
| IronRDP turning the picture into RGB and composing it | 1.5 to 3 ms |
| Progressive RemoteFX, all of it | 4 ms |
| Copying the whole desktop out for the interface | 0.8 ms |

Hardware decoding would save only the decoder's share, and the picture
would have to come back from the GPU for IronRDP to compose it; it waits
until a slower machine shows a need. The copy is gone for most frames:
frames come from a few buffers used in turn, each brought up to date with
only what changed, and a small change costs 0.01 ms. Every frame goes to
GTK as it comes, replacing one not yet painted, so each refresh of the
monitor shows the newest picture.

Quality presets became a choice of codec per connection: automatic (AVC444
where the server has it), AVC420, or RemoteFX. Color depth applies only to
the old bitmap updates, which none of these servers send once the pipeline
is offered, and they ignore the visual effect flags (xrdp reads one, to
leave out its own login screen's background), so neither is offered.

The network type goes to the server too, LAN unless the connection says
otherwise, and comes in and out with `.rdp` and Remmina files. Of these
servers only xrdp acts on it, by setting its H.264 bitrate: at 1920×1080 with
the screen in motion, LAN took 1.8 MB/s at 43 frames a second, low-speed
broadband 0.2 MB/s at 7, a modem 0.1 MB/s at 2.5. Automatic detection isn't
offered: IronRDP can't answer the server's measurements, and xrdp would take
it as LAN.

### 7. Audio

Playback (MS-RDPEA) through PipeWire, the microphone (MS-RDPEAI), and a
per-connection choice of here, on the remote machine, or off.

Built on IronRDP's sound and audio input channels, both the static one xrdp
uses and the dynamic one GNOME Remote Desktop uses. Sound is played, and the
microphone recorded, through PulseAudio's simple API as PipeWire serves it:
it is what a Flatpak reaches with `--socket=pulseaudio`, where PipeWire's
own socket is out of reach. Each stream runs on a thread of its own, about
100 ms ahead of the speakers. Sound comes as Opus where the server offers
it, decoded with libopus, and as PCM otherwise: from GNOME Remote Desktop,
14 kilobytes a second against 176. The microphone goes as PCM.

Sound and the microphone are two switches in the editor, the microphone off
until turned on. Sound played on the remote machine and no sound at all,
which Windows tells apart, are the same to these servers: what isn't sent
stays where it is, so the choice is here or not. Both come in and out with
`.rdp` files (`audiomode`, `audiocapturemode`) and Remmina profiles
(`sound`, `microphone`).

Tried against the three servers, through a private PipeWire so nothing
reached the speakers:

| Server | Sound | Microphone |
| --- | --- | --- |
| GNOME Remote Desktop | Opus, or PCM | PCM |
| xrdp | PCM | PCM |
| KRdp | No | No |

xrdp needs `pipewire-module-xrdp` (or the PulseAudio one) in the session,
which Fedora doesn't package; built from neutrinolabs' source on the test
machine, it gave both. KRdp opens no sound channel. Not yet tried in the
app itself, or in the Flatpak, which needs `--socket=pulseaudio`.

### 8. Device redirection

Shared folders (MS-RDPEFS), smart cards and printers. USB, serial and
parallel port redirection are not in IronRDP; this section decides whether
they are worth adding.

Built: one shared folder per connection, chosen in the editor and read from
Remmina profiles (`sharefolder`), through IronRDP's backend for Unix, which
resolves every path beneath the folder and follows no symbolic links. One
folder is what that backend serves, and what Remmina offers. A folder that
is gone when connecting isn't offered.

Of the three servers only xrdp opens the device channel; GNOME Remote
Desktop and KRdp never do. With xrdp the folder is in `~/thinclient_drives`,
and was tried there by listing, reading, writing, renaming, copying and
deleting files and folders, and by trying to climb out of it with `..`.
xrdp's requests needed the workarounds listed under Protocol engine.

Decided against, for these servers:

- Printers: xrdp turns a redirected printer down, and the others don't open
  the channel. IronRDP's spooler, which hands jobs to `lp`, is left out.
- Smart cards: IronRDP serves them only on Windows. A PC/SC backend for
  Linux would help xrdp alone; it waits until someone needs it.
- USB, serial and parallel ports: none of the three servers takes them.

### 9. Network

- SSH tunnels, with keys from the SSH agent: the usual way into a Linux box
  that exposes only SSH.
- RD Gateway.
- Wake-on-LAN before connecting.
- Automatic reconnection after a dropped network.

### 10. Session controls

- Keyboard grab that keeps GNOME's own shortcuts out of the way while the
  session has focus, with a key to release it.
- Sending key combinations the local desktop would otherwise take:
  Ctrl+Alt+Del, Ctrl+Alt+F1 to F12, Super.
- Screenshots, view-only mode, multiple monitors.
- Commands run before connecting and after disconnecting.

### 11. Settings and polish

App preferences, default connection settings, a keyboard shortcuts dialog,
accessibility review, and translations.

### 12. Packaging

A Flatpak on the GNOME runtime, AppStream metainfo, the app icon, a desktop
file, and an RPM. The Flatpak needs the codecs-extra extension for H.264
and `--socket=pulseaudio` for sound and the microphone.

## Remmina parity

What Remmina's RDP plugin offers, and the section that brings it to Mote.

| Remmina | Section |
| --- | --- |
| Profiles, groups, search, quick connect | 2 |
| Passwords in the keyring | 2 |
| `.rdp` import and export | 2 |
| Security negotiation: RDP, TLS, NLA | 3 |
| Certificate checks | 3 |
| Server redirection and load-balance info | 3 |
| Client name, start-up program, console session | 3 |
| Resolution: client, custom, dynamic | 4 |
| Scaling and fullscreen | 4 |
| Clipboard, with the option to turn it off | 5 |
| Codec choice (GFX AVC444, AVC420, RemoteFX); color depth is moot | 6 |
| Quality presets and visual effect flags, which Linux servers ignore | 6 |
| Network type hint, which only xrdp acts on | 6 |
| Sound: here, remote or off (the last two alike on Linux servers); microphone | 7 |
| Shared folders (one per connection; xrdp only) | 8 |
| Printers, smart cards: no Linux server takes them | 8 (not done) |
| Serial and parallel ports, USB: no Linux server takes them | 8 (not done) |
| SSH tunnel | 9 |
| RD Gateway | 9 |
| Wake-on-LAN | 9 |
| Auto-reconnect | 9 |
| Tabs, floating toolbar, scrolled and fullscreen views | 10 |
| Keyboard grab, send Ctrl+Alt+Del | 10 |
| Screenshot, view only | 10 |
| Multiple monitors, span | 10 |
| Pre and post commands | 10 |
| Keyboard layout, left-handed mouse | 3, 11 |
