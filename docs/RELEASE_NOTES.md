# Mote

A remote desktop client for GNOME, for Linux machines that run GNOME Remote
Desktop, xrdp or KRdp. Each section below is what a release brought; the
detail is in the commit history.

## What's new in 1.0.0

### New features:

- Sessions with GNOME Remote Desktop, both Desktop Sharing and Remote Login
- Sessions with xrdp and KRdp
- A sharp picture at any scale, fractional ones included, resized with the window
- H.264 from FFmpeg, the system's OpenH264 or Cisco's, chosen in Settings
- Advice on what to install when a server sends no H.264
- The clipboard both ways: text, rich text, images and files
- Sound from the remote desktop, and the microphone to it
- A shared folder, lent to the remote desktop
- SSH tunnels, RD Gateways and Wake-on-LAN
- Reconnecting by itself when the network drops
- Every monitor, or one, in fullscreen
- The desktop's shortcuts held back while a session has focus
- The keyboard back to this computer once the pointer leaves the remote desktop
- Right Ctrl, alone, lets go of the keyboard and shows the header in fullscreen
- Connections imported from .rdp files and Remmina, and exported as .rdp files
- Groups, sorting and a custom order in the sidebar, each connection with a robot
- Flatpak, AppImage, RPM and Debian packages for x86_64 and ARM64
