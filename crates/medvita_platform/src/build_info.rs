//! What the binary knows about itself: name, identifiers, version and the build-time defaults.

/// Product name shown to people.
pub const APP_NAME: &str = "Medvita";

/// Stable application identifier (also the Windows AppUserModelID prefix).
pub const APP_ID: &str = "io.katua.medvita.desktop";

/// OIDC client id registered on the server and the client id of the update channel.
pub const CLIENT_ID: &str = "medvita-desktop";

/// Publisher shown in the installer and in "Sobre".
pub const PUBLISHER: &str = "Katuá Tech";

/// Directory name under `%APPDATA%` and `%LOCALAPPDATA%` (the Velopack `packId` too).
pub const DIR_NAME: &str = "Medvita";

/// Version of the app: the workspace version, written by the release automation.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Commit the binary was built from, when the build set `GIT_HASH` (CI does).
pub fn commit() -> Option<&'static str> {
    option_env!("GIT_HASH").filter(|value| !value.is_empty())
}

/// Server URL compiled into release builds (`MEDVITA_DEFAULT_SERVER_URL`); `None` asks at first run.
pub fn default_server_url() -> Option<&'static str> {
    option_env!("MEDVITA_DEFAULT_SERVER_URL").filter(|value| !value.is_empty())
}

/// `User-Agent` sent on every request, e.g. `Medvita-Desktop/0.1.0 (windows; x86_64)`.
pub fn user_agent() -> String {
    format!(
        "Medvita-Desktop/{} ({}; {})",
        version(),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_the_product_and_version() {
        let agent = user_agent();
        assert!(agent.starts_with("Medvita-Desktop/"));
        assert!(agent.contains(version()));
    }
}
