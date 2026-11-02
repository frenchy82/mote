//! What to install on a server so it sends H.264, by server, distribution
//! and, for GNOME Remote Desktop, graphics card. Checked against each
//! distribution's packages in November 2026; when a release changes what it
//! ships, this is the place to change.

use crate::i18n::{i18n, i18n_f};
use crate::rdp::server::{Distro, Graphics, Server};

/// A paragraph, and the commands it asks for, if any.
pub(super) struct Step {
    pub(super) text: String,
    pub(super) commands: Vec<&'static str>,
}

fn step(text: String, commands: &[&'static str]) -> Step {
    Step {
        text,
        commands: commands.to_vec(),
    }
}

const SWAP_OPENH264: &str = "sudo dnf swap noopenh264 openh264";
const RPM_FUSION: &str = "sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm";

pub(super) fn distro_name(distro: Distro) -> &'static str {
    match distro {
        Distro::Fedora => "Fedora",
        Distro::Ubuntu => "Ubuntu",
        Distro::Debian => "Debian",
        Distro::Arch => "Arch Linux",
        Distro::Opensuse => "openSUSE",
    }
}

pub(super) fn server_name(server: Server) -> String {
    match server {
        Server::GnomeDesktop => i18n("GNOME Desktop Sharing"),
        Server::GnomeLogin => i18n("GNOME Remote Login"),
        Server::Krdp => "KRdp".to_owned(),
        Server::Xrdp => "xrdp".to_owned(),
    }
}

pub(super) fn graphics_name(graphics: Graphics) -> String {
    match graphics {
        Graphics::Intel => "Intel".to_owned(),
        Graphics::Amd => "AMD".to_owned(),
        Graphics::Nvidia => "NVIDIA".to_owned(),
        Graphics::Virtual => i18n("Virtual Machine or None"),
    }
}

/// How the server starts again with what was installed.
pub(super) enum Restart {
    Command(&'static str),
    /// A new graphics driver takes a restart of the computer.
    Computer,
    /// Nothing was installed.
    Nothing,
}

/// The steps for a server on a distribution; `graphics` counts only for
/// GNOME Remote Desktop.
pub(super) fn advice(server: Server, distro: Distro, graphics: Graphics) -> (Vec<Step>, Restart) {
    match server {
        Server::Xrdp => (xrdp(distro), xrdp_restart(distro)),
        Server::Krdp => (
            krdp(distro),
            Restart::Command("systemctl --user restart app-org.kde.krdpserver.service"),
        ),
        Server::GnomeDesktop | Server::GnomeLogin => {
            let mut steps = vec![step(
                i18n("GNOME Remote Desktop sends H.264 only when the graphics card encodes it."),
                &[],
            )];
            let (more, restart) = gnome(distro, graphics);
            steps.extend(more);
            let restart = match restart {
                Restart::Command(_) if server == Server::GnomeLogin => {
                    Restart::Command("sudo systemctl restart gnome-remote-desktop")
                }
                Restart::Command(_) => {
                    Restart::Command("systemctl --user restart gnome-remote-desktop")
                }
                other => other,
            };
            (steps, restart)
        }
    }
}

fn xrdp(distro: Distro) -> Vec<Step> {
    match distro {
        Distro::Fedora => vec![step(
            i18n(
                "Fedora’s xrdp encodes H.264 with OpenH264, which Fedora ships as an empty placeholder. Swap in Cisco’s build, and install the Xorg back end that H.264 needs:",
            ),
            &[SWAP_OPENH264, "sudo dnf install xorgxrdp"],
        )],
        Distro::Ubuntu => vec![step(
            i18n(
                "Ubuntu’s xrdp is older than 0.10.2, the first version to send H.264, so no package adds it.",
            ),
            &[],
        )],
        Distro::Debian => vec![step(
            i18n(
                "Debian 13’s xrdp is older than 0.10.2, the first version to send H.264. The one in trixie-backports sends it:",
            ),
            &[
                "echo 'deb http://deb.debian.org/debian trixie-backports main' | sudo tee /etc/apt/sources.list.d/trixie-backports.list",
                "sudo apt update",
                "sudo apt install -t trixie-backports xrdp xorgxrdp",
            ],
        )],
        Distro::Arch => vec![step(
            i18n(
                "The AUR’s xrdp sends H.264 to sessions that use its Xorg back end, from the AUR’s xorgxrdp package.",
            ),
            &[],
        )],
        Distro::Opensuse => vec![step(
            i18n("openSUSE’s xrdp is built without an H.264 encoder, so no package adds it."),
            &[],
        )],
    }
}

fn xrdp_restart(distro: Distro) -> Restart {
    match distro {
        Distro::Fedora | Distro::Debian => {
            Restart::Command("sudo systemctl restart xrdp xrdp-sesman")
        }
        _ => Restart::Nothing,
    }
}

fn krdp(distro: Distro) -> Vec<Step> {
    match distro {
        Distro::Fedora => vec![step(
            i18n(
                "Fedora’s FFmpeg encodes H.264 for KRdp with OpenH264, which Fedora ships as an empty placeholder. Swap in Cisco’s build:",
            ),
            &[SWAP_OPENH264],
        )],
        Distro::Ubuntu | Distro::Debian => vec![step(
            i18n_f(
                "{distro}’s FFmpeg has the x264 encoder KRdp uses, so nothing should be missing. Bring the system up to date:",
                &[("distro", &distro_name(distro))],
            ),
            &["sudo apt update && sudo apt full-upgrade"],
        )],
        Distro::Arch => vec![step(
            i18n_f(
                "{distro}’s FFmpeg has the x264 encoder KRdp uses, so nothing should be missing. Bring the system up to date:",
                &[("distro", &distro_name(distro))],
            ),
            &["sudo pacman -Syu"],
        )],
        Distro::Opensuse => vec![step(
            i18n(
                "openSUSE’s FFmpeg encodes H.264 for KRdp only with OpenH264, from openSUSE’s codecs repository:",
            ),
            &["sudo zypper install libopenh264-8"],
        )],
    }
}

fn gnome(distro: Distro, graphics: Graphics) -> (Vec<Step>, Restart) {
    let service = Restart::Command("");
    match (graphics, distro) {
        (Graphics::Virtual, _) => (
            vec![step(
                i18n(
                    "A virtual machine has no graphics card to encode with, so it sends RemoteFX.",
                ),
                &[],
            )],
            Restart::Nothing,
        ),
        (Graphics::Nvidia, Distro::Fedora) => (
            vec![step(
                i18n("On an NVIDIA card it needs NVIDIA’s own driver, from RPM Fusion:"),
                &[
                    RPM_FUSION,
                    "sudo dnf install akmod-nvidia xorg-x11-drv-nvidia-cuda-libs",
                ],
            )],
            Restart::Computer,
        ),
        (Graphics::Nvidia, Distro::Ubuntu) => (
            vec![step(
                i18n("On an NVIDIA card it needs NVIDIA’s own driver:"),
                &["sudo ubuntu-drivers install"],
            )],
            Restart::Computer,
        ),
        (Graphics::Nvidia, Distro::Debian) => (
            vec![step(
                i18n(
                    "On an NVIDIA card it needs NVIDIA’s own driver, from Debian’s non-free section, once that is turned on in its package sources:",
                ),
                &["sudo apt install nvidia-driver"],
            )],
            Restart::Computer,
        ),
        (Graphics::Nvidia, Distro::Arch) => (
            vec![step(
                i18n("On an NVIDIA card it needs NVIDIA’s own driver:"),
                &["sudo pacman -S nvidia-open nvidia-utils"],
            )],
            Restart::Computer,
        ),
        (Graphics::Nvidia, Distro::Opensuse) => (
            vec![step(
                i18n(
                    "On an NVIDIA card it needs NVIDIA’s own driver, from NVIDIA’s repository for openSUSE.",
                ),
                &[],
            )],
            Restart::Computer,
        ),
        (_, Distro::Fedora) => (
            vec![step(
                i18n(
                    "On Fedora 44 and later it encodes H.264 on Intel and AMD cards with the VA-API driver from RPM Fusion, as Fedora’s own leaves H.264 out:",
                ),
                &[
                    RPM_FUSION,
                    if graphics == Graphics::Amd {
                        "sudo dnf swap mesa-va-drivers mesa-va-drivers-freeworld"
                    } else {
                        "sudo dnf install intel-media-driver"
                    },
                ],
            )],
            service,
        ),
        (_, Distro::Ubuntu | Distro::Debian) => (
            vec![
                step(
                    i18n_f(
                        "It encodes H.264 on Intel and AMD cards from version 50, which {distro} may not have yet. This shows the version:",
                        &[("distro", &distro_name(distro))],
                    ),
                    &["dpkg-query -W gnome-remote-desktop"],
                ),
                step(
                    i18n("From version 50, it needs the VA-API driver:"),
                    &[if graphics == Graphics::Amd {
                        "sudo apt install mesa-va-drivers"
                    } else {
                        "sudo apt install intel-media-va-driver-non-free"
                    }],
                ),
            ],
            service,
        ),
        (_, Distro::Arch) => (
            vec![step(
                i18n("It needs the VA-API and Vulkan drivers:"),
                &[if graphics == Graphics::Amd {
                    "sudo pacman -S vulkan-radeon"
                } else {
                    "sudo pacman -S intel-media-driver vulkan-intel"
                }],
            )],
            service,
        ),
        (Graphics::Amd, Distro::Opensuse) => (
            vec![step(
                i18n("On an AMD card it needs Mesa’s VA-API driver with H.264, from Packman."),
                &[],
            )],
            service,
        ),
        (_, Distro::Opensuse) => (
            vec![step(
                i18n("It needs the VA-API driver:"),
                &["sudo zypper install intel-media-driver"],
            )],
            service,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_has_advice_and_commands_need_no_typing_in() {
        for server in Server::ALL {
            for distro in Distro::ALL {
                for graphics in Graphics::ALL {
                    let (steps, restart) = advice(server, distro, graphics);
                    assert!(!steps.is_empty());
                    for command in steps.iter().flat_map(|s| &s.commands) {
                        assert!(!command.contains('<'), "{command}");
                    }
                    if let Restart::Command(command) = restart {
                        assert!(!command.is_empty(), "{server:?} {distro:?} {graphics:?}");
                    }
                }
            }
        }
    }
}
