# Mote

A remote desktop client for GNOME, for Linux machines that run GNOME Remote
Desktop, xrdp or KRdp. Each section below is what a release brought; the
detail is in the commit history.

## What's new in 1.3.0-beta.2

### New features:

- Scan Network covers additional subnets

### Fixes:

- Flatpak bundles never received updates

## What's new in 1.3.0-beta.1

### New features:

- Scan Network lists the RDP servers on the local network
- Adding a scanned server opens the connection editor
- Delete button in the connection editor
- Beta builds in every package format

### Fixes:

- deleting a connection left its session running

## What's new in 1.2.0

### New features:

- Open a connection fullscreen
- Open a connection in a new window
- Window defaults for new connections in Settings
- Option to hide account avatars in the sidebar
- Option to hide the large account avatar on the connection page

## What's new in 1.1.0

### New features:

- Advice on making GNOME Desktop Sharing resize with the window
- Option to turn off the Desktop Sharing size notice
- Flatpak shares connections and settings with the other installs
- Flatpak shares saved passwords with the other installs
- Notice that the server can read the clipboard on a connection's first session
- Copied files shared only when the connection allows it
- Clipboard sharing off by default for new connections

### Fixes:

- Passwords wiped from memory once used
- Startup warning when session logging can record passwords

## What's new in 1.0.0

### New features:

- GNOME Remote Desktop support for Desktop Sharing and Remote Login
- xrdp and KRdp support
- Fractional scaling supported
- Automatic window resizing
- H.264 decoding through FFmpeg or OpenH264
- H.264 decoder choice in Settings
- Advice on what to install when a server sends no H.264
- Two-way clipboard for text and rich text
- Two-way clipboard for images and files
- Remote desktop sound played on this computer
- Microphone sent to the remote desktop
- Shared folder for the remote desktop
- SSH tunnels
- RD Gateway support
- Wake-on-LAN
- Automatic reconnection when the network drops
- Fullscreen on one monitor or on all of them
- Desktop shortcuts held back while a session has focus
- Keyboard returned to this computer when the pointer leaves the session
- Right Ctrl on its own releases the keyboard and shows the header
- Import from .rdp files and Remmina
- Export to .rdp files
- Connection groups in the sidebar
- Sorting and custom ordering in the sidebar
- Robot avatars for connections
- Flatpak and AppImage builds
- RPM and Debian packages
- Builds for x86_64 and ARM64
