//! Connection passwords, in the Secret Service: the GNOME keyring on the
//! host, the secret portal inside Flatpak. Each item is found by the
//! connection's id, so renaming a connection keeps its password.
//!
//! Everything here is async and runs on relm4's command runtime, never on
//! the GTK main thread: an unlock prompt can take as long as the user does.

use crate::APP_ID;

/// Which of a connection's passwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Secret {
    /// The account on the remote desktop.
    Account,
    /// The account on the SSH server the connection goes through.
    Ssh,
    /// The account on the RD Gateway.
    Gateway,
}

impl Secret {
    pub(crate) const ALL: [Self; 3] = [Self::Account, Self::Ssh, Self::Gateway];

    /// The attribute naming the connection. Each kind has its own, so a
    /// search for one never finds another: the account's is the one
    /// passwords were saved under before the others existed.
    fn attribute(self) -> &'static str {
        match self {
            Self::Account => "connection",
            Self::Ssh => "ssh-tunnel",
            Self::Gateway => "rd-gateway",
        }
    }

    /// The name the keyring shows for it.
    pub(crate) fn label(self, title: &str) -> String {
        match self {
            Self::Account => format!("{} ({})", title, crate::APP_NAME),
            Self::Ssh => format!("{}, SSH tunnel ({})", title, crate::APP_NAME),
            Self::Gateway => format!("{}, RD Gateway ({})", title, crate::APP_NAME),
        }
    }
}

fn attributes(id: &str, kind: Secret) -> [(&'static str, &str); 2] {
    [("application", APP_ID), (kind.attribute(), id)]
}

/// The saved password, or `None` when there is none.
pub(crate) async fn password(id: String, kind: Secret) -> Result<Option<String>, String> {
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    let items = keyring
        .search_items(&attributes(&id, kind))
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
    kind: Secret,
    title: String,
    password: String,
) -> Result<(), String> {
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    if password.is_empty() {
        return keyring
            .delete(&attributes(&id, kind))
            .await
            .map_err(|e| e.to_string());
    }
    keyring
        .create_item(
            &kind.label(&title),
            &attributes(&id, kind),
            password.as_str(),
            true,
        )
        .await
        .map_err(|e| e.to_string())
}

/// Forget every password the connection has.
pub(crate) async fn delete_passwords(id: String) -> Result<(), String> {
    let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
    for kind in Secret::ALL {
        keyring
            .delete(&attributes(&id, kind))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Give the connection `to` the passwords `from` has.
pub(crate) async fn copy_passwords(from: String, to: String, title: String) -> Result<(), String> {
    for kind in Secret::ALL {
        if let Some(password) = password(from.clone(), kind).await? {
            set_password(to.clone(), kind, title.clone(), password).await?;
        }
    }
    Ok(())
}
