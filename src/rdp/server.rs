//! What the computer at the other end is, as far as a session can tell:
//! which RDP server from its certificate, and, through an SSH tunnel that
//! ends on it, which distribution and graphics card. It picks the advice
//! given when a server sends no H.264.

use serde::{Deserialize, Serialize};
use x509_cert::der::Decode;

/// The RDP server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Server {
    /// GNOME Remote Desktop sharing a signed-in desktop.
    #[serde(rename = "gnome-desktop-sharing")]
    GnomeDesktop,
    /// GNOME Remote Desktop's Remote Login, run by the system.
    #[serde(rename = "gnome-remote-login")]
    GnomeLogin,
    #[serde(rename = "krdp")]
    Krdp,
    #[serde(rename = "xrdp")]
    Xrdp,
}

impl Server {
    pub(crate) const ALL: [Self; 4] =
        [Self::GnomeDesktop, Self::GnomeLogin, Self::Krdp, Self::Xrdp];

    /// A guess from the certificate's subject, which each server makes the
    /// same way when it makes its own: GNOME's settings name it "GNOME",
    /// Fedora's xrdp "XRDP", and KRdp leaves OpenSSL's defaults. A
    /// certificate set up by hand says nothing.
    pub(crate) fn from_certificate(der: &[u8]) -> Option<Self> {
        let certificate = x509_cert::Certificate::from_der(der).ok()?;
        Self::from_subject(&certificate.tbs_certificate().subject().to_string())
    }

    fn from_subject(subject: &str) -> Option<Self> {
        let parts: Vec<&str> = subject.split(',').map(str::trim).collect();
        if parts.contains(&"CN=GNOME") {
            Some(Self::GnomeDesktop)
        } else if parts.contains(&"CN=XRDP") {
            Some(Self::Xrdp)
        } else if parts.contains(&"O=Default Company Ltd")
            && !parts.iter().any(|p| p.starts_with("CN="))
        {
            Some(Self::Krdp)
        } else {
            None
        }
    }
}

/// The name in a certificate that names its computer, as those made by
/// Debian's and Ubuntu's packages do; the ones [`Server`] knows by their
/// subject name the program instead, or nothing.
pub(crate) fn certificate_name(der: &[u8]) -> Option<String> {
    let certificate = x509_cert::Certificate::from_der(der).ok()?;
    name_in_subject(&certificate.tbs_certificate().subject().to_string())
}

fn name_in_subject(subject: &str) -> Option<String> {
    if Server::from_subject(subject).is_some() {
        return None;
    }
    subject
        .split(',')
        .find_map(|part| part.trim().strip_prefix("CN="))
        .map(str::to_owned)
}

/// The distribution the server runs on, by the family its packages come
/// from: Linux Mint counts as Ubuntu, Manjaro as Arch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Distro {
    Fedora,
    Ubuntu,
    Debian,
    Arch,
    Opensuse,
}

impl Distro {
    pub(crate) const ALL: [Self; 5] = [
        Self::Fedora,
        Self::Ubuntu,
        Self::Debian,
        Self::Arch,
        Self::Opensuse,
    ];

    /// The distribution `/etc/os-release` names, by its ID, or else by the
    /// first ID it is like that is one of these.
    pub(crate) fn from_os_release(text: &str) -> Option<Self> {
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
                .map(|value| value.trim().trim_matches('"').to_lowercase())
                .unwrap_or_default()
        };
        let (id, like) = (field("ID"), field("ID_LIKE"));
        std::iter::once(id.as_str())
            .chain(like.split_whitespace())
            .find_map(|id| match id {
                "fedora" => Some(Self::Fedora),
                "ubuntu" => Some(Self::Ubuntu),
                "debian" => Some(Self::Debian),
                "arch" => Some(Self::Arch),
                "suse" => Some(Self::Opensuse),
                id if id.starts_with("opensuse") => Some(Self::Opensuse),
                _ => None,
            })
    }
}

/// The graphics card, which decides whether GNOME Remote Desktop can
/// encode H.264.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Graphics {
    Intel,
    Amd,
    Nvidia,
    /// A virtual machine's, or none: nothing to encode with.
    Virtual,
}

impl Graphics {
    pub(crate) const ALL: [Self; 4] = [Self::Intel, Self::Amd, Self::Nvidia, Self::Virtual];

    /// From the PCI vendor IDs of the cards in `/sys/class/drm`, one to a
    /// line. With several, the one that encodes best: NVIDIA, then AMD,
    /// then Intel.
    pub(crate) fn from_vendors(text: &str) -> Option<Self> {
        let found: Vec<Self> = text
            .lines()
            .filter_map(|line| match line.trim() {
                "0x8086" => Some(Self::Intel),
                "0x1002" => Some(Self::Amd),
                "0x10de" => Some(Self::Nvidia),
                // virtio, QEMU, VMware, VirtualBox, Hyper-V.
                "0x1af4" | "0x1234" | "0x15ad" | "0x80ee" | "0x1414" => Some(Self::Virtual),
                _ => None,
            })
            .collect();
        [Self::Nvidia, Self::Amd, Self::Intel, Self::Virtual]
            .into_iter()
            .find(|g| found.contains(g))
    }
}

/// What SSH found on the computer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct System {
    pub(crate) distro: Option<Distro>,
    pub(crate) graphics: Option<Graphics>,
}

/// Run on the computer to find its [`System`]; the two parts come apart at
/// [`SPLIT`].
pub(crate) const PROBE: &str =
    "cat /etc/os-release; echo ---mote---; cat /sys/class/drm/card*/device/vendor 2>/dev/null";
const SPLIT: &str = "---mote---";

impl System {
    pub(crate) fn from_probe(output: &str) -> Self {
        let (release, vendors) = output.split_once(SPLIT).unwrap_or((output, ""));
        Self {
            distro: Distro::from_os_release(release),
            graphics: Graphics::from_vendors(vendors),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_are_known_by_their_own_certificates() {
        assert_eq!(
            Server::from_subject("CN=GNOME,C=US"),
            Some(Server::GnomeDesktop)
        );
        assert_eq!(Server::from_subject("CN=XRDP"), Some(Server::Xrdp));
        assert_eq!(
            Server::from_subject("C=XX,L=Default City,O=Default Company Ltd"),
            Some(Server::Krdp)
        );
        assert_eq!(Server::from_subject("CN=build01.lab"), None);
        assert_eq!(
            Server::from_subject("CN=host,O=Default Company Ltd"),
            None,
            "a name set by hand"
        );
    }

    #[test]
    fn a_certificate_names_a_computer_only_when_made_for_one() {
        assert_eq!(
            name_in_subject("CN=build01.lab,O=Lab"),
            Some("build01.lab".to_owned())
        );
        assert_eq!(name_in_subject("CN=GNOME,C=US"), None);
        assert_eq!(name_in_subject("CN=XRDP"), None);
        assert_eq!(name_in_subject("O=Lab"), None);
    }

    #[test]
    fn systems_are_read_from_the_probe() {
        let fedora = "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=44\n---mote---\n0x1af4\n";
        assert_eq!(
            System::from_probe(fedora),
            System {
                distro: Some(Distro::Fedora),
                graphics: Some(Graphics::Virtual),
            }
        );
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n---mote---\n0x8086\n0x10de\n";
        assert_eq!(
            System::from_probe(mint),
            System {
                distro: Some(Distro::Ubuntu),
                graphics: Some(Graphics::Nvidia),
            }
        );
        let tumbleweed = "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n---mote---\n";
        assert_eq!(
            System::from_probe(tumbleweed).distro,
            Some(Distro::Opensuse)
        );
        assert_eq!(System::from_probe("ID=gentoo\n"), System::default());
    }
}
