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
writes in Rust. At 0.17 it has a client connector with TLS and CredSSP (NLA),
the graphics pipeline with OpenH264 decoding, the clipboard, display control,
sound, drive redirection, smart cards and RD Gateway, all as separate crates.
Being Rust, it runs on the session's own async runtime and needs no C bindings.

FreeRDP 3, the engine Remmina uses, was the alternative. It supports more
devices (USB, serial and parallel ports), at the cost of a C API surface, a
callback-driven event loop and a larger Flatpak. Where IronRDP comes up short
on something a section needs, that section adds it upstream.

## Sections

| # | Section | State |
| --- | --- | --- |
| 1 | Foundation | Done |
| 2 | Connections | Done |
| 3 | First session | Next |
| 4 | HiDPI and resizing | |
| 5 | Clipboard | |
| 6 | Graphics pipeline | |
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
- Disconnect, reconnect, and readable errors.

### 4. HiDPI and resizing

- The session sized to the window and resized with it (Display Control,
  MS-RDPEDISP).
- Desktop and device scale factors taken from the monitor, fractional scales
  included, and updated when the window moves between monitors.
- Scaling modes: resize the remote desktop, fit to the window, or one remote
  pixel per device pixel.
- Fullscreen.

### 5. Clipboard

Text, rich text, images and files in both directions (MS-RDPECLIP), through
GDK's clipboard, with a per-connection switch.

### 6. Graphics pipeline

- The graphics pipeline (MS-RDPEGFX): progressive RemoteFX, AVC420 and
  AVC444.
- Decoding off the main thread, upload straight to a GPU texture, frames paced
  to the monitor's refresh.
- Quality presets that set color depth, codecs and the server's visual
  effects, with a custom option.
- A measurement harness, so performance changes are compared by numbers.

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

- Sessions in tabs (AdwTabView) or in windows of their own.
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
