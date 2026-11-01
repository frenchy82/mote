//! The commands a connection runs before connecting and after
//! disconnecting: through `sh -c`, with the connection in the environment
//! rather than pasted into the command, where a name could break it.

use gtk::gio;

use crate::connection::Connection;

fn spawn(
    command: &str,
    connection: &Connection,
    flags: gio::SubprocessFlags,
) -> Result<gio::Subprocess, String> {
    let launcher = gio::SubprocessLauncher::new(flags);
    for (name, value) in [
        ("MOTE_NAME", connection.title()),
        ("MOTE_GROUP", connection.group.as_str()),
        ("MOTE_HOST", connection.host.as_str()),
        ("MOTE_PORT", &connection.port.to_string()),
        ("MOTE_USER", connection.username.as_str()),
    ] {
        launcher.setenv(name, value, true);
    }
    launcher
        .spawn(&["sh".as_ref(), "-c".as_ref(), command.as_ref()])
        .map_err(|e| e.message().to_owned())
}

/// Run the command before connecting, and wait for it. `Err` holds what
/// went wrong: the last line it wrote to stderr, or its exit status.
pub(super) async fn before(command: String, connection: Connection) -> Result<(), String> {
    let process = spawn(&command, &connection, gio::SubprocessFlags::STDERR_PIPE)?;
    let (_, stderr) = process
        .communicate_utf8_future(None)
        .await
        .map_err(|e| e.message().to_owned())?;
    if process.is_successful() {
        return Ok(());
    }
    let said = stderr
        .as_deref()
        .and_then(|text| text.lines().rev().find(|l| !l.trim().is_empty()))
        .map(|line| line.trim().to_owned());
    Err(said.unwrap_or_else(|| {
        if process.has_exited() {
            format!("exit status {}", process.exit_status())
        } else {
            format!("signal {}", process.term_sig())
        }
    }))
}

/// Start the command after disconnecting, without waiting for it.
pub(super) fn after(command: &str, connection: &Connection) {
    if let Err(e) = spawn(command, connection, gio::SubprocessFlags::NONE) {
        tracing::warn!("the command after disconnecting: {e}");
    }
}
