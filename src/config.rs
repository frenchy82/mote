//! What Mote keeps on disk: `settings.toml` and `connections.toml` in
//! `$XDG_CONFIG_HOME/mote`. Passwords are never here; see `secrets`.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::connection::{Connection, DEFAULT_PORT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SortOrder {
    #[default]
    Name,
    RecentlyUsed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WindowState {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 720,
            maximized: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) color_scheme: ColorScheme,
    pub(crate) sort: SortOrder,
    pub(crate) show_groups: bool,
    pub(crate) show_hosts: bool,
    /// What a new connection starts with.
    pub(crate) default_port: u16,
    pub(crate) default_username: String,
    pub(crate) default_domain: String,
    pub(crate) window: WindowState,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::System,
            sort: SortOrder::Name,
            show_groups: true,
            show_hosts: true,
            default_port: DEFAULT_PORT,
            default_username: String::new(),
            default_domain: String::new(),
            window: WindowState::default(),
        }
    }
}

impl Settings {
    /// A blank connection carrying the defaults from Settings.
    pub(crate) fn new_connection(&self) -> Connection {
        Connection {
            port: self.default_port,
            username: self.default_username.clone(),
            domain: self.default_domain.clone(),
            ..Connection::default()
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct ConnectionsFile {
    #[serde(default, rename = "connection")]
    connections: Vec<Connection>,
}

fn dir() -> PathBuf {
    gtk::glib::user_config_dir().join("mote")
}

pub(crate) fn load_settings() -> Settings {
    load(&dir().join("settings.toml")).unwrap_or_default()
}

pub(crate) fn save_settings(settings: &Settings) {
    save(&dir().join("settings.toml"), settings);
}

pub(crate) fn load_connections() -> Vec<Connection> {
    load::<ConnectionsFile>(&dir().join("connections.toml"))
        .unwrap_or_default()
        .connections
}

pub(crate) fn save_connections(connections: &[Connection]) {
    let file = ConnectionsFile {
        connections: connections.to_vec(),
    };
    save(&dir().join("connections.toml"), &file);
}

/// Read a TOML file. One that doesn't parse (written by a newer version, or
/// edited by hand) is moved aside first, so the next save can't overwrite
/// what the user had.
fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!("reading {}: {e}", path.display());
            return None;
        }
    };
    match toml::from_str(&text) {
        Ok(value) => Some(value),
        Err(e) => {
            let stamp = gtk::glib::DateTime::now_utc()
                .map(|t| t.to_unix())
                .unwrap_or_default();
            let aside = path.with_extension(format!("toml.unreadable-{stamp}"));
            tracing::warn!(
                "{} does not parse ({e}); kept as {}",
                path.display(),
                aside.display()
            );
            let _ = std::fs::rename(path, &aside);
            None
        }
    }
}

/// Write a TOML file whole, through a temporary file and a rename, so a
/// crash mid-write leaves the old one. Owner-only: host and user names are
/// nobody else's business.
fn save<T: Serialize>(path: &Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        let text = toml::to_string_pretty(value).map_err(std::io::Error::other)?;
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let tmp = path.with_extension("toml.tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        tracing::error!("saving {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connections_round_trip() {
        let file = ConnectionsFile {
            connections: vec![Connection {
                name: "Office".into(),
                host: "box".into(),
                ..Connection::default()
            }],
        };
        let text = toml::to_string_pretty(&file).unwrap();
        let back: ConnectionsFile = toml::from_str(&text).unwrap();
        assert_eq!(back.connections, file.connections);
    }

    #[test]
    fn missing_settings_take_defaults() {
        let s: Settings = toml::from_str("sort = \"recently-used\"").unwrap();
        assert_eq!(s.sort, SortOrder::RecentlyUsed);
        assert_eq!(s.default_port, DEFAULT_PORT);
        assert!(s.show_groups);
    }
}
