//! A saved connection: where it goes and who it signs in as. The password is
//! never part of it; that lives in the keyring (see `secrets`).

use serde::{Deserialize, Serialize};

/// The port RDP listens on unless told otherwise.
pub(crate) const DEFAULT_PORT: u16 = 3389;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Connection {
    /// Stable across renames: the keyring item and the sidebar row find the
    /// connection by it.
    pub(crate) id: String,
    /// Empty means the host is the name.
    pub(crate) name: String,
    pub(crate) group: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) domain: String,
    /// Unix seconds of the last session, 0 for never.
    pub(crate) last_used: i64,
    pub(crate) scaling: Scaling,
}

/// How the remote desktop meets the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scaling {
    /// The remote desktop takes the window's size and the screen's scale,
    /// and follows both.
    #[default]
    Resize,
    /// The remote desktop keeps its size; its picture is scaled to fit.
    Fit,
    /// One remote pixel to each pixel of the screen, scrolled when larger
    /// than the window.
    Actual,
}

impl Scaling {
    pub(crate) const ALL: [Self; 3] = [Self::Resize, Self::Fit, Self::Actual];

    /// The name an action state carries.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Resize => "resize",
            Self::Fit => "fit",
            Self::Actual => "actual",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            group: String::new(),
            host: String::new(),
            port: DEFAULT_PORT,
            username: String::new(),
            domain: String::new(),
            last_used: 0,
            scaling: Scaling::default(),
        }
    }
}

pub(crate) fn new_id() -> String {
    gtk::glib::uuid_string_random().to_string()
}

impl Connection {
    /// What the connection is called on screen.
    pub(crate) fn title(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.host
        } else {
            &self.name
        }
    }

    /// The host, with the port only when it isn't RDP's own.
    pub(crate) fn address(&self) -> String {
        if self.port == DEFAULT_PORT {
            self.host.clone()
        } else if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The account as Windows writes it, `DOMAIN\user`, or `None` when the
    /// connection names no user.
    pub(crate) fn account(&self) -> Option<String> {
        match (self.username.is_empty(), self.domain.is_empty()) {
            (true, _) => None,
            (false, true) => Some(self.username.clone()),
            (false, false) => Some(format!("{}\\{}", self.domain, self.username)),
        }
    }

    /// Whether a search for `query` (already lower case) finds this.
    pub(crate) fn matches(&self, query: &str) -> bool {
        [&self.name, &self.host, &self.username, &self.group]
            .iter()
            .any(|f| f.to_lowercase().contains(query))
    }

    /// Whether `other` reaches the same account on the same machine, which
    /// is what makes an import a duplicate.
    pub(crate) fn same_target(&self, other: &Connection) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.port == other.port
            && self.username.eq_ignore_ascii_case(&other.username)
    }

    /// Fields as they should be stored: trimmed, a port typed into the host
    /// field moved to the port, and `DOMAIN\user` split.
    pub(crate) fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_owned();
        self.group = self.group.trim().to_owned();
        self.domain = self.domain.trim().to_owned();
        self.username = self.username.trim().to_owned();
        if let Some((host, port)) = split_address(&self.host) {
            self.host = host;
            if let Some(port) = port {
                self.port = port;
            }
        } else {
            self.host = self.host.trim().to_owned();
        }
        if self.domain.is_empty()
            && let Some((domain, user)) = self.username.split_once('\\')
        {
            self.domain = domain.to_owned();
            self.username = user.to_owned();
        }
        self
    }
}

/// Split what someone typed as an address into a host and, if one was
/// given, a port: `host`, `host:port`, `[v6]:port`, a bare IPv6 address, or
/// any of those after `rdp://`. `None` when it isn't an address at all.
pub(crate) fn split_address(text: &str) -> Option<(String, Option<u16>)> {
    let text = text.trim();
    let text = text.strip_prefix("rdp://").unwrap_or(text);
    let text = text.trim_end_matches('/');
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(rest) = text.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse().ok().filter(|&p| p != 0)?),
            None if after.is_empty() => None,
            None => return None,
        };
        return (!host.is_empty()).then(|| (host.to_owned(), port));
    }
    match text.matches(':').count() {
        1 => {
            let (host, port) = text.split_once(':')?;
            let port = port.parse().ok().filter(|&p| p != 0)?;
            (!host.is_empty()).then(|| (host.to_owned(), Some(port)))
        }
        // None, or several: an IPv6 address written without brackets, which
        // can't carry a port.
        _ => Some((text.to_owned(), None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_split_into_host_and_port() {
        let s = |t| split_address(t);
        assert_eq!(s("box"), Some(("box".into(), None)));
        assert_eq!(s(" box.lan:3390 "), Some(("box.lan".into(), Some(3390))));
        assert_eq!(
            s("rdp://10.0.0.5:4000/"),
            Some(("10.0.0.5".into(), Some(4000)))
        );
        assert_eq!(s("[fe80::1]:3390"), Some(("fe80::1".into(), Some(3390))));
        assert_eq!(s("[fe80::1]"), Some(("fe80::1".into(), None)));
        assert_eq!(s("fe80::1"), Some(("fe80::1".into(), None)));
        assert_eq!(s("box:http"), None);
        assert_eq!(s("box:0"), None);
        assert_eq!(s("two words"), None);
        assert_eq!(s(""), None);
    }

    #[test]
    fn normalizing_moves_port_and_domain() {
        let c = Connection {
            host: " box:3390 ".into(),
            username: "CORP\\jo".into(),
            ..Connection::default()
        }
        .normalized();
        assert_eq!((c.host.as_str(), c.port), ("box", 3390));
        assert_eq!((c.domain.as_str(), c.username.as_str()), ("CORP", "jo"));
        assert_eq!(c.account().as_deref(), Some("CORP\\jo"));
    }

    #[test]
    fn address_shows_port_only_when_unusual() {
        let mut c = Connection {
            host: "box".into(),
            ..Connection::default()
        };
        assert_eq!(c.address(), "box");
        c.port = 3390;
        assert_eq!(c.address(), "box:3390");
        c.host = "fe80::1".into();
        assert_eq!(c.address(), "[fe80::1]:3390");
    }
}
