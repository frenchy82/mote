//! Where Mote keeps its files: `~/.config/mote` and `~/.local/share/mote`
//! for every kind of install (outside Flatpak, wherever `XDG_CONFIG_HOME`
//! and `XDG_DATA_HOME` point). The Flatpak would otherwise get a tree of
//! its own under `~/.var/app`, which a package or an AppImage never sees.
//! Its manifest grants it the two by their home paths, with `:create` so
//! they exist even on a first install: the `xdg-config/mote` form would
//! also mount them over the sandbox's own `mote` directories, hiding what
//! an earlier version saved there before it could be moved.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gtk::glib;

const NAME: &str = "mote";

/// The settings, the connections and the trusted certificates.
pub(crate) fn config_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| shared(glib::user_config_dir(), ".config"))
        .clone()
}

/// The connections' pictures and Cisco's OpenH264.
pub(crate) fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| shared(glib::user_data_dir(), ".local/share"))
        .clone()
}

fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// Inside Flatpak, the directory under the home that the manifest grants;
/// outside, `own` (the XDG one). Flatpak skips a grant whose directory is
/// missing, leaving the path on a throwaway tmpfs, so the granted one
/// counts only if it is there; decided once, before anything of Mote's
/// own could create it.
fn shared(own: PathBuf, under_home: &str) -> PathBuf {
    if in_flatpak() {
        let host = glib::home_dir().join(under_home).join(NAME);
        if host.is_dir() {
            return host;
        }
        tracing::warn!(
            "{} is not reachable; using the sandbox's own directory",
            host.display()
        );
    }
    own.join(NAME)
}

/// Inside Flatpak, move what an earlier version saved in the sandbox's
/// own directories to the shared ones. Connections and trusted
/// certificates are joined with those already there; of two settings
/// files the newer wins. The old directory is renamed, not deleted, so
/// the move happens once.
pub(crate) fn move_from_sandbox() {
    if !in_flatpak() {
        return;
    }
    for (old, new) in [
        (glib::user_config_dir().join(NAME), config_dir()),
        (glib::user_data_dir().join(NAME), data_dir()),
    ] {
        if old == new || !old.is_dir() {
            continue;
        }
        match merge(&old, &new) {
            Ok(()) => {
                let moved = old.with_file_name(format!("{NAME}.moved"));
                match std::fs::rename(&old, &moved) {
                    Ok(()) => tracing::info!("moved {} to {}", old.display(), new.display()),
                    Err(e) => tracing::warn!("renaming {}: {e}", old.display()),
                }
            }
            Err(e) => tracing::warn!("moving {} to {}: {e}", old.display(), new.display()),
        }
    }
}

/// Copy every file under `from` into `to`: the lists of connections and
/// trusted certificates are joined, and of two other files the newer is
/// kept.
fn merge(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            merge(&source, &target)?;
        } else if target.exists() && crate::config::merge_list(&source, &target) {
            // Joined, entry by entry.
        } else if newer(&source, &target) {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// `source` is newer than `target`, or `target` is missing.
fn newer(source: &Path, target: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (modified(source), modified(target)) {
        (_, None) => true,
        (Some(s), Some(t)) => s > t,
        (None, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_keeps_the_newer_file_and_adds_missing_ones() {
        let root = std::env::temp_dir().join(format!("mote-paths-{}", std::process::id()));
        let (old, new) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(old.join("pictures")).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("settings.toml"), "old").unwrap();
        std::fs::write(old.join("trust.toml"), "old").unwrap();
        std::fs::write(old.join("pictures/a.png"), "picture").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Saved since, by a package: kept.
        std::fs::write(new.join("settings.toml"), "new").unwrap();

        merge(&old, &new).unwrap();
        let read = |p: &str| std::fs::read_to_string(new.join(p)).unwrap();
        assert_eq!(read("settings.toml"), "new");
        assert_eq!(read("trust.toml"), "old");
        assert_eq!(read("pictures/a.png"), "picture");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
