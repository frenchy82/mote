//! Connection passwords, in the Secret Service: the GNOME keyring on the
//! host, the secret portal inside Flatpak. Each item is found by the
//! connection's id, so renaming a connection keeps its password.
//!
//! Everything here is async and runs on relm4's command runtime, never on
//! the GTK main thread: an unlock prompt can take as long as the user does.

use crate::APP_ID;

fn attributes(id: &str) -> [(&'static str, &str); 2] {
    [("application", APP_ID), ("connection", id)]
}

/// The saved password, or `None` when there is none.
pub(crate) async fn password(id: String) -> Result<Option<String>, String> {
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    let items = keyring
        .search_items(&attributes(&id))
        .await
        .map_err(|e| e.to_string())?;
    let Some(item) = items.first() else {
        return Ok(None);
    };
    let secret = item.secret().await.map_err(|e| e.to_string())?;
    String::from_utf8(secret.to_vec())
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Save `password` for the connection, or forget it when it is empty.
pub(crate) async fn set_password(
    id: String,
    label: String,
    password: String,
) -> Result<(), String> {
    if password.is_empty() {
        return delete_password(id).await;
    }
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    keyring
        .create_item(&label, &attributes(&id), password.as_str(), true)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) async fn delete_password(id: String) -> Result<(), String> {
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    keyring
        .delete(&attributes(&id))
        .await
        .map_err(|e| e.to_string())
}

/// The name the keyring shows for a connection's password.
pub(crate) fn label(title: &str) -> String {
    format!("{} ({})", title, crate::APP_NAME)
}
