//! Stable identifier for crafting recipes.
//!
//! [`RecipeId`] mirrors Mojang's resource location convention
//! (`namespace:path`). Vanilla recipes use the `minecraft` namespace;
//! plugin recipes typically use the plugin's identifier.
//!
//! Two recipes with the same `result_id` are still distinguishable by
//! their `RecipeId` — for example, two pickaxe recipes with different
//! ingredient layouts have different ids.

use std::fmt;

/// Stable identifier for a crafting recipe.
///
/// The wire form is `"namespace:path"`. Both segments are
/// non-empty UTF-8 strings; [`RecipeId::parse`] enforces this on
/// input. Equality is by exact namespace + path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecipeId {
    /// Namespace segment (e.g. `"minecraft"`, `"my_plugin"`).
    pub namespace: String,
    /// Path segment (e.g. `"oak_planks"`, `"shaped_42"`).
    pub path: String,
}

impl RecipeId {
    /// Constructs a [`RecipeId`] from explicit namespace and path.
    ///
    /// Both arguments are taken via `Into<String>` so callers can
    /// pass `&str`, owned `String`, or any other convertible type.
    pub fn new(namespace: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            path: path.into(),
        }
    }

    /// Constructs a vanilla [`RecipeId`] under the `"minecraft"` namespace.
    ///
    /// Shorthand for [`RecipeId::new("minecraft", path)`](Self::new).
    pub fn vanilla(path: impl Into<String>) -> Self {
        Self::new("minecraft", path)
    }

    /// Parses a `"namespace:path"` string into a [`RecipeId`].
    ///
    /// Returns `None` if the input does not contain exactly one colon
    /// or if either segment is empty. The split is on the **first**
    /// colon, so paths with embedded colons are not supported (and
    /// no current Mojang recipe has one).
    pub fn parse(input: &str) -> Option<Self> {
        let (ns, path) = input.split_once(':')?;
        if ns.is_empty() || path.is_empty() {
            return None;
        }
        Some(Self::new(ns, path))
    }
}

impl fmt::Display for RecipeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_owns_strings() {
        let id = RecipeId::new("plugin", "magic_sword");
        assert_eq!(id.namespace, "plugin");
        assert_eq!(id.path, "magic_sword");
    }

    #[test]
    fn vanilla_uses_minecraft_namespace() {
        let id = RecipeId::vanilla("oak_planks");
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "oak_planks");
    }

    #[test]
    fn display_round_trips_with_parse() {
        let id = RecipeId::vanilla("crafting_table");
        let s = id.to_string();
        assert_eq!(s, "minecraft:crafting_table");
        assert_eq!(RecipeId::parse(&s), Some(id));
    }

    #[test]
    fn parse_rejects_missing_colon() {
        assert_eq!(RecipeId::parse("oak_planks"), None);
    }

    #[test]
    fn parse_rejects_empty_namespace() {
        assert_eq!(RecipeId::parse(":oak_planks"), None);
    }

    #[test]
    fn parse_rejects_empty_path() {
        assert_eq!(RecipeId::parse("minecraft:"), None);
    }

    #[test]
    fn parse_takes_first_colon() {
        // The first colon delimits the namespace; the rest is the
        // path verbatim. No current vanilla recipe has a colon in
        // its path, but the contract is documented.
        let id = RecipeId::parse("ns:a:b").unwrap();
        assert_eq!(id.namespace, "ns");
        assert_eq!(id.path, "a:b");
    }

    #[test]
    fn equality_is_exact() {
        assert_eq!(
            RecipeId::vanilla("oak_planks"),
            RecipeId::new("minecraft", "oak_planks")
        );
        assert_ne!(
            RecipeId::vanilla("oak_planks"),
            RecipeId::vanilla("birch_planks")
        );
    }

    #[test]
    fn hashable_in_collections() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(RecipeId::vanilla("oak_planks"));
        assert!(set.contains(&RecipeId::new("minecraft", "oak_planks")));
    }
}
