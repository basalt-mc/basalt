//! Skin texture fetching from the Mojang API.
//!
//! When a player connects in offline mode, we don't have their skin
//! textures. This module fetches them from the Mojang session server
//! using the player's username, then provides the `properties` data
//! needed for the `PlayerInfo` packet so other players see the correct
//! skin.

use serde::Deserialize;

/// A profile property from the Mojang API (typically skin textures).
///
/// These are sent in the `PlayerInfo` packet's add_player action
/// as part of the game profile. The client uses the `textures`
/// property to download and render the player's skin.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProfileProperty {
    /// Property name (always "textures" for skins).
    pub name: String,
    /// Base64-encoded JSON containing the skin/cape URLs.
    pub value: String,
    /// Mojang signature for the property (base64-encoded).
    #[serde(default)]
    pub signature: Option<String>,
}

/// Response from the Mojang username-to-UUID API.
#[derive(Deserialize)]
struct UsernameResponse {
    #[allow(dead_code)]
    id: String,
}

/// Response from the Mojang session server profile API.
#[derive(Deserialize)]
struct ProfileResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    properties: Vec<ProfileProperty>,
}

/// Fetches the skin textures for a player from the Mojang API.
///
/// Makes two HTTP requests:
/// 1. `api.mojang.com/users/profiles/minecraft/<username>` → UUID
/// 2. `sessionserver.mojang.com/session/minecraft/profile/<uuid>?unsigned=false` → textures
///
/// Returns the profile properties (typically one entry named "textures")
/// or an empty vec if the player doesn't have a Mojang account or the
/// API is unreachable. Errors are logged but not propagated — skins
/// are optional and should never prevent a player from joining.
pub(crate) async fn fetch_skin_properties(username: &str) -> Vec<ProfileProperty> {
    match fetch_skin_inner(username).await {
        Ok(props) => {
            println!("[skin] Fetched {} properties for {username}", props.len());
            props
        }
        Err(e) => {
            println!("[skin] Failed to fetch skin for {username}: {e}");
            Vec::new()
        }
    }
}

/// Inner implementation that returns Result for error handling.
async fn fetch_skin_inner(
    username: &str,
) -> Result<Vec<ProfileProperty>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    // Step 1: username → UUID
    let url = format!("https://api.mojang.com/users/profiles/minecraft/{username}");
    let resp: UsernameResponse = client.get(&url).send().await?.json().await?;
    let uuid = resp.id;

    // Step 2: UUID → profile with textures
    let url =
        format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}?unsigned=false");
    let profile: ProfileResponse = client.get(&url).send().await?.json().await?;

    Ok(profile.properties)
}
