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
| 5 | Clipboard | |
| 6 | Graphics pipeline | In progress |
| 7 | Audio | |
| 8 | Device redirection | |
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
and was tried inside it. GNOME Remote Desktop chooses AVC444 and xrdp AVC420,
both sharper than the RemoteFX they used before, and KRdp, which sends only
H.264, now connects. Decoding already runs on the session's own thread; the
rest of the section is still to come.

### 7. Audio

Playback (MS-RDPEA) through PipeWire, the microphone (MS-RDPEAI), and a
per-connection choice of here, on the remote machine, or off.

### 8. Device redirection

Shared folders (MS-RDPEFS), smart cards and printers. USB, serial and
parallel port redirection are not in IronRDP; this section decides whether
they are worth adding.

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
file, and an RPM.

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
| Color depth and codec choice (GFX AVC444, AVC420, RemoteFX) | 6 |
| Quality presets and visual effect flags | 6 |
| Network type hint | 6 |
| Sound: local, remote or off; microphone | 7 |
| Shared folders | 8 |
| Printers, smart cards | 8 |
| Serial and parallel ports, USB | 8 (to decide) |
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
