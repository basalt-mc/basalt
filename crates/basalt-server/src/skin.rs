//! Skin texture fetching from the Mojang API.
//!
//! When a player connects in offline mode, we don't have their skin
//! textures. This module fetches them from the Mojang session server
//! using the player's username, then provides the `properties` data
//! needed for the `PlayerInfo` packet so other players see the correct
//! skin.

use basalt_api::ProfileProperty;
use serde::Deserialize;

/// Mojang API profile property (deserializable).
///
/// Converted to [`basalt_api::ProfileProperty`] after fetching.
#[derive(Deserialize)]
struct MojangProperty {
    name: String,
    value: String,
    #[serde(default)]
    signature: Option<String>,
}

impl From<MojangProperty> for ProfileProperty {
    fn from(p: MojangProperty) -> Self {
        Self {
            name: p.name,
            value: p.value,
            signature: p.signature,
        }
    }
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
    properties: Vec<MojangProperty>,
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
            log::debug!(target: "basalt::skin", "Fetched {} properties for {username}", props.len());
            props
        }
        Err(e) => {
            log::warn!(target: "basalt::skin", "Failed to fetch skin for {username}: {e}");
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

    Ok(profile.properties.into_iter().map(Into::into).collect())
}
