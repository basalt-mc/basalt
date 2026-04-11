//! String manipulation utilities and filesystem helpers.
//!
//! Shared across the codegen pipeline for converting between naming
//! conventions (snake_case, PascalCase) and managing workspace paths.

use std::fs;
use std::path::{Path, PathBuf};

/// Converts a `snake_case` string to `PascalCase`.
///
/// Each segment separated by `_` is capitalized independently.
/// Empty segments (from consecutive underscores) are skipped.
pub(crate) fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

/// Converts a `camelCase` or `PascalCase` string to `snake_case`.
///
/// Handles consecutive uppercase letters correctly (e.g.,
/// "playerUUID" → "player_uuid"). Rust keywords (`type`, `match`)
/// are prefixed with `r#` to produce valid identifiers.
pub(crate) fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() {
            let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if prev_is_lower || (i > 0 && chars[i - 1].is_uppercase() && next_is_lower) {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    match result.as_str() {
        "type" => "r#type".to_string(),
        "match" => "r#match".to_string(),
        _ => result,
    }
}

/// Removes the direction prefix and state name to get a short enum variant.
///
/// For example, "ServerboundLoginEncryptionBegin" with state "Login"
/// becomes "EncryptionBegin". Used for concise enum variant names
/// in the direction dispatch enums.
pub(crate) fn short_variant_name(full_name: &str, state_pascal: &str) -> String {
    let without_dir = full_name
        .strip_prefix("Serverbound")
        .or_else(|| full_name.strip_prefix("Clientbound"))
        .unwrap_or(full_name);
    without_dir
        .strip_prefix(state_pascal)
        .unwrap_or(without_dir)
        .to_string()
}

/// Finds the workspace root by walking up from the current directory
/// until a `Cargo.toml` with a `[workspace]` section is found.
pub(crate) fn find_workspace_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("Failed to get current directory");
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = fs::read_to_string(&cargo_toml).unwrap_or_default();
            if content.contains("[workspace]") {
                return dir;
            }
        }
        if !dir.pop() {
            panic!("Could not find workspace root (no Cargo.toml with [workspace])");
        }
    }
}

/// Runs rustfmt on a generated file to ensure it matches the project's
/// formatting standards. This way the codegen output is commit-ready
/// without a separate `cargo fmt` step.
pub(crate) fn format_file(path: &Path) {
    let status = std::process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .arg(path)
        .status()
        .unwrap_or_else(|e| panic!("Failed to run rustfmt on {}: {e}", path.display()));
    if !status.success() {
        eprintln!(
            "Warning: rustfmt failed on {} (exit code {:?})",
            path.display(),
            status.code()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- to_snake_case --

    #[test]
    fn snake_case_simple() {
        assert_eq!(to_snake_case("serverHost"), "server_host");
    }

    #[test]
    fn snake_case_consecutive_uppercase() {
        assert_eq!(to_snake_case("playerUUID"), "player_uuid");
    }

    #[test]
    fn snake_case_all_lowercase() {
        assert_eq!(to_snake_case("username"), "username");
    }

    #[test]
    fn snake_case_leading_uppercase() {
        assert_eq!(to_snake_case("ServerHost"), "server_host");
    }

    #[test]
    fn snake_case_single_char() {
        assert_eq!(to_snake_case("x"), "x");
    }

    #[test]
    fn snake_case_keyword_type() {
        assert_eq!(to_snake_case("type"), "r#type");
    }

    #[test]
    fn snake_case_keyword_match() {
        assert_eq!(to_snake_case("match"), "r#match");
    }

    #[test]
    fn snake_case_camel_multi() {
        assert_eq!(to_snake_case("shouldAuthenticate"), "should_authenticate");
    }

    #[test]
    fn snake_case_message_id() {
        assert_eq!(to_snake_case("messageId"), "message_id");
    }

    // -- to_pascal_case --

    #[test]
    fn pascal_case_simple() {
        assert_eq!(to_pascal_case("set_protocol"), "SetProtocol");
    }

    #[test]
    fn pascal_case_single_word() {
        assert_eq!(to_pascal_case("login"), "Login");
    }

    #[test]
    fn pascal_case_multiple_words() {
        assert_eq!(
            to_pascal_case("login_plugin_response"),
            "LoginPluginResponse"
        );
    }

    #[test]
    fn pascal_case_already_capitalized() {
        assert_eq!(to_pascal_case("Login"), "Login");
    }

    #[test]
    fn pascal_case_with_numbers() {
        assert_eq!(
            to_pascal_case("legacy_server_list_ping"),
            "LegacyServerListPing"
        );
    }

    // -- short_variant_name --

    #[test]
    fn short_variant_strips_direction_and_state() {
        assert_eq!(
            short_variant_name("ServerboundLoginEncryptionBegin", "Login"),
            "EncryptionBegin"
        );
    }

    #[test]
    fn short_variant_clientbound() {
        assert_eq!(
            short_variant_name("ClientboundStatusServerInfo", "Status"),
            "ServerInfo"
        );
    }

    #[test]
    fn short_variant_no_prefix() {
        assert_eq!(short_variant_name("SomePacket", "Login"), "SomePacket");
    }
}
