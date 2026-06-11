use std::fs;
use std::path::PathBuf;

/// Get the Chronoverse config directory (~/.crv/).
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".crv")
}

/// Read the stored auth ticket, if any.
pub fn load_ticket() -> Option<String> {
    let path = config_dir().join("ticket");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Store an auth ticket to ~/.crv/ticket.
pub fn save_ticket(ticket: &str) {
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("ticket"), ticket);
}

/// Remove the stored auth ticket.
pub fn clear_ticket() {
    let path = config_dir().join("ticket");
    let _ = fs::remove_file(path);
}

/// Load the current client/workspace name, if set.
pub fn load_client_name() -> Option<String> {
    let path = config_dir().join("client");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Store the current client/workspace name.
pub fn save_client_name(name: &str) {
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("client"), name);
}
