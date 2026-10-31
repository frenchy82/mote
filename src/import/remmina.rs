//! Remmina's connection profiles: one `.remmina` key file per connection,
//! in its data directory (or the Flatpak's). Only RDP profiles come across.
//! Remmina keeps passwords encrypted with its own key or in the keyring
//! under its own schema, so they are not imported: the first connection
//! asks.

use std::path::PathBuf;

use crate::connection::{Connection, Network, Quality, split_address};

/// Every directory Remmina may have kept profiles in.
fn dirs() -> Vec<PathBuf> {
    let home = gtk::glib::home_dir();
    vec![
        gtk::glib::user_data_dir().join("remmina"),
        home.join(".var/app/org.remmina.Remmina/data/remmina"),
        home.join(".remmina"),
    ]
}

/// The RDP connections in Remmina's profiles, and whether any profile
/// directory was found at all.
pub(crate) fn find() -> (Vec<Connection>, bool) {
    let mut found = Vec::new();
    let mut any_dir = false;
    for dir in dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        any_dir = true;
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "remmina"))
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Some(conn) = parse(&text)
            {
                found.push(conn);
            }
        }
    }
    (found, any_dir)
}

/// The connection in one profile, if it is an RDP one.
pub(crate) fn parse(text: &str) -> Option<Connection> {
    let mut in_section = false;
    let mut conn = Connection::default();
    let mut rdp = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[remmina]";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "protocol" => rdp = value.eq_ignore_ascii_case("rdp"),
            "name" => conn.name = value.to_owned(),
            "group" => conn.group = value.to_owned(),
            "server" => {
                let (host, port) = split_address(value)?;
                conn.host = host;
                if let Some(port) = port {
                    conn.port = port;
                }
            }
            "username" => conn.username = value.to_owned(),
            "domain" => conn.domain = value.to_owned(),
            "network" => conn.network = Network::from_remmina(value),
            // Remmina's color depth list holds its codec choice too.
            "colordepth" => {
                conn.quality = match value {
                    "65" => Quality::Avc420,
                    "0" | "63" | "64" => Quality::RemoteFx,
                    _ => Quality::Best,
                }
            }
            "disableclipboard" => conn.clipboard = value != "1",
            _ => {}
        }
    }
    (rdp && !conn.host.is_empty()).then(|| conn.normalized())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_rdp_profile() {
        let text = "[remmina]\nname=Lab\nprotocol=RDP\nserver=lab.lan:3390\nusername=jo\ngroup=Work\npassword=.\n";
        let c = parse(text).unwrap();
        assert_eq!(
            (c.name.as_str(), c.host.as_str(), c.port),
            ("Lab", "lab.lan", 3390)
        );
        assert_eq!((c.username.as_str(), c.group.as_str()), ("jo", "Work"));
    }

    #[test]
    fn reads_the_network_codec_and_clipboard() {
        let text =
            "[remmina]\nprotocol=RDP\nserver=lab\nnetwork=wan\ncolordepth=65\ndisableclipboard=1\n";
        let c = parse(text).unwrap();
        assert_eq!((c.network, c.quality), (Network::Wan, Quality::Avc420));
        assert!(!c.clipboard);
    }

    #[test]
    fn skips_other_protocols() {
        assert!(parse("[remmina]\nprotocol=VNC\nserver=box\n").is_none());
    }
}
