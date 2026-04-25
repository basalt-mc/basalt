//! Recipe codegen: reads minecraft-data recipes JSON and generates static
//! Rust data for all vanilla shaped and shapeless crafting recipes.
//!
//! Output is written to `crates/basalt-recipes/src/generated.rs` with
//! pre-allocated `&'static` slices — zero heap allocation at runtime.

use std::fmt::Write as _;
use std::fs;

use serde_json::Value;

use crate::helpers::{find_workspace_root, format_file};

/// Minecraft version to read recipes from.
const VERSION: &str = "1.21.4";

/// Path to minecraft-data submodule relative to workspace root.
const MINECRAFT_DATA_PATH: &str = "minecraft-data/data/pc";

/// Output path for generated recipe data relative to workspace root.
const OUTPUT_PATH: &str = "crates/basalt-recipes/src/generated.rs";

/// A parsed shaped recipe ready for codegen.
struct Shaped {
    width: u8,
    height: u8,
    /// Row-major ingredient grid; `None` = empty slot.
    ingredients: Vec<Option<i32>>,
    result_id: i32,
    result_count: u8,
}

/// A parsed shapeless recipe ready for codegen.
struct Shapeless {
    /// Sorted ingredient item IDs (ascending).
    ingredients: Vec<i32>,
    result_id: i32,
    result_count: u8,
}

/// Reads `recipes.json`, classifies each variant, and writes the generated
/// Rust source file with static recipe data.
pub fn run_recipes() {
    let workspace_root = find_workspace_root();
    let json_path = workspace_root
        .join(MINECRAFT_DATA_PATH)
        .join(VERSION)
        .join("recipes.json");

    println!("Reading recipe data from {}", json_path.display());
    let raw = fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", json_path.display()));

    let root: Value = serde_json::from_str(&raw).expect("Failed to parse recipes JSON");
    let map = root.as_object().expect("Top-level value must be an object");

    let mut shaped: Vec<Shaped> = Vec::new();
    let mut shapeless: Vec<Shapeless> = Vec::new();

    for (_key, variants) in map {
        let variants = variants.as_array().expect("Recipe value must be an array");
        for variant in variants {
            let result = &variant["result"];
            let result_id = result["id"].as_i64().expect("result.id must be int") as i32;
            let result_count = result["count"].as_i64().expect("result.count must be int") as u8;

            if let Some(in_shape) = variant.get("inShape") {
                shaped.push(parse_shaped(in_shape, result_id, result_count));
            } else if let Some(ingredients) = variant.get("ingredients") {
                shapeless.push(parse_shapeless(ingredients, result_id, result_count));
            }
        }
    }

    // Deterministic output: sort by (result_id, result_count, ingredients)
    shaped.sort_by(|a, b| {
        a.result_id
            .cmp(&b.result_id)
            .then(a.result_count.cmp(&b.result_count))
            .then(a.ingredients.len().cmp(&b.ingredients.len()))
    });
    shapeless.sort_by(|a, b| {
        a.result_id
            .cmp(&b.result_id)
            .then(a.result_count.cmp(&b.result_count))
            .then(a.ingredients.cmp(&b.ingredients))
    });

    println!(
        "Parsed {} shaped + {} shapeless = {} total recipes",
        shaped.len(),
        shapeless.len(),
        shaped.len() + shapeless.len()
    );

    let code = generate_source(&shaped, &shapeless);

    let output_path = workspace_root.join(OUTPUT_PATH);
    println!("Writing generated recipes to {}", output_path.display());
    fs::write(&output_path, &code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", output_path.display()));

    format_file(&output_path);
    println!("Done.");
}

/// Parses a shaped recipe from the `inShape` JSON value.
///
/// The grid is a 2D array where `null` represents an empty slot and
/// integers represent item state IDs. Width is the maximum row length
/// across all rows.
fn parse_shaped(in_shape: &Value, result_id: i32, result_count: u8) -> Shaped {
    let rows = in_shape.as_array().expect("inShape must be an array");
    let height = rows.len() as u8;
    let width = rows
        .iter()
        .map(|row| row.as_array().expect("inShape row must be an array").len())
        .max()
        .unwrap_or(0) as u8;

    let mut ingredients = Vec::with_capacity(width as usize * height as usize);
    for row in rows {
        let cells = row.as_array().expect("inShape row must be an array");
        for col in 0..width as usize {
            if col < cells.len() {
                if cells[col].is_null() {
                    ingredients.push(None);
                } else {
                    ingredients.push(Some(
                        cells[col].as_i64().expect("item ID must be int") as i32
                    ));
                }
            } else {
                // Pad shorter rows with empty slots
                ingredients.push(None);
            }
        }
    }

    Shaped {
        width,
        height,
        ingredients,
        result_id,
        result_count,
    }
}

/// Parses a shapeless recipe from the `ingredients` JSON array.
///
/// Ingredient IDs are sorted ascending so that matching can use a
/// simple O(n) comparison after sorting the crafting grid contents.
fn parse_shapeless(ingredients: &Value, result_id: i32, result_count: u8) -> Shapeless {
    let arr = ingredients
        .as_array()
        .expect("ingredients must be an array");
    let mut ids: Vec<i32> = arr
        .iter()
        .map(|v| v.as_i64().expect("ingredient must be int") as i32)
        .collect();
    ids.sort_unstable();

    Shapeless {
        ingredients: ids,
        result_id,
        result_count,
    }
}

/// Generates the complete Rust source file for `generated.rs`.
fn generate_source(shaped: &[Shaped], shapeless: &[Shapeless]) -> String {
    let mut out = String::with_capacity(256 * 1024);

    // Module doc comment
    writeln!(
        out,
        "//! Codegen'd static recipe data for vanilla Minecraft."
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(
        out,
        "//! This module is generated by `cargo xt recipes` from"
    )
    .unwrap();
    writeln!(
        out,
        "//! PrismarineJS/minecraft-data recipe JSON. Do not edit by hand."
    )
    .unwrap();
    writeln!(
        out,
        "//! The structs use `&'static` slices so all recipe data lives in"
    )
    .unwrap();
    writeln!(
        out,
        "//! read-only program memory with zero heap allocation."
    )
    .unwrap();
    writeln!(out).unwrap();

    // Struct definitions
    emit_shaped_struct(&mut out);
    emit_shapeless_struct(&mut out);

    // Per-recipe ingredient statics
    for (i, recipe) in shaped.iter().enumerate() {
        emit_shaped_ingredients(&mut out, i, recipe);
    }
    writeln!(out).unwrap();

    for (i, recipe) in shapeless.iter().enumerate() {
        emit_shapeless_ingredients(&mut out, i, recipe);
    }
    writeln!(out).unwrap();

    // Main static slices
    emit_shaped_slice(&mut out, shaped);
    emit_shapeless_slice(&mut out, shapeless);

    // Tests
    emit_tests(&mut out, shaped, shapeless);

    out
}

/// Emits the `ShapedRecipe` struct definition.
fn emit_shaped_struct(out: &mut String) {
    writeln!(
        out,
        "/// A shaped crafting recipe with a fixed grid pattern."
    )
    .unwrap();
    writeln!(out, "///").unwrap();
    writeln!(
        out,
        "/// The grid is stored as a flat slice in row-major order. `None` entries"
    )
    .unwrap();
    writeln!(
        out,
        "/// represent empty slots; `Some(id)` entries represent required item"
    )
    .unwrap();
    writeln!(
        out,
        "/// state IDs. The `width` and `height` fields define the grid dimensions"
    )
    .unwrap();
    writeln!(out, "/// (1-3 for standard crafting tables).").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]").unwrap();
    writeln!(out, "pub struct ShapedRecipe {{").unwrap();
    writeln!(
        out,
        "    /// Stable resource-location identifier (`namespace:path`)."
    )
    .unwrap();
    writeln!(out, "    ///").unwrap();
    writeln!(
        out,
        "    /// Vanilla ids are synthetic `minecraft:shaped_<n>` placeholders"
    )
    .unwrap();
    writeln!(
        out,
        "    /// derived from codegen sort order — minecraft-data does not"
    )
    .unwrap();
    writeln!(out, "    /// carry real recipe names.").unwrap();
    writeln!(out, "    pub id: &'static str,").unwrap();
    writeln!(
        out,
        "    /// Grid width (1-3 for standard crafting table recipes)."
    )
    .unwrap();
    writeln!(out, "    pub width: u8,").unwrap();
    writeln!(
        out,
        "    /// Grid height (1-3 for standard crafting table recipes)."
    )
    .unwrap();
    writeln!(out, "    pub height: u8,").unwrap();
    writeln!(
        out,
        "    /// Flat grid of ingredient item IDs in row-major order."
    )
    .unwrap();
    writeln!(out, "    ///").unwrap();
    writeln!(
        out,
        "    /// Length is always `width * height`. `None` means the slot must be"
    )
    .unwrap();
    writeln!(
        out,
        "    /// empty; `Some(id)` means the slot requires that item state ID."
    )
    .unwrap();
    writeln!(out, "    pub ingredients: &'static [Option<i32>],").unwrap();
    writeln!(out, "    /// The item state ID of the crafted result.").unwrap();
    writeln!(out, "    pub result_id: i32,").unwrap();
    writeln!(out, "    /// How many items are produced per craft.").unwrap();
    writeln!(out, "    pub result_count: u8,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// Emits the `ShapelessRecipe` struct definition.
fn emit_shapeless_struct(out: &mut String) {
    writeln!(
        out,
        "/// A shapeless crafting recipe with an unordered set of ingredients."
    )
    .unwrap();
    writeln!(out, "///").unwrap();
    writeln!(
        out,
        "/// The ingredients can appear in any arrangement on the crafting grid."
    )
    .unwrap();
    writeln!(
        out,
        "/// Order does not matter; only the multiset of item IDs must match."
    )
    .unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]").unwrap();
    writeln!(out, "pub struct ShapelessRecipe {{").unwrap();
    writeln!(
        out,
        "    /// Stable resource-location identifier (`namespace:path`)."
    )
    .unwrap();
    writeln!(out, "    ///").unwrap();
    writeln!(
        out,
        "    /// Vanilla ids are synthetic `minecraft:shapeless_<n>` placeholders"
    )
    .unwrap();
    writeln!(
        out,
        "    /// derived from codegen sort order — minecraft-data does not"
    )
    .unwrap();
    writeln!(out, "    /// carry real recipe names.").unwrap();
    writeln!(out, "    pub id: &'static str,").unwrap();
    writeln!(
        out,
        "    /// Unordered set of required ingredient item state IDs."
    )
    .unwrap();
    writeln!(out, "    ///").unwrap();
    writeln!(
        out,
        "    /// Duplicates are allowed (e.g., two planks for pressure plate)."
    )
    .unwrap();
    writeln!(out, "    pub ingredients: &'static [i32],").unwrap();
    writeln!(out, "    /// The item state ID of the crafted result.").unwrap();
    writeln!(out, "    pub result_id: i32,").unwrap();
    writeln!(out, "    /// How many items are produced per craft.").unwrap();
    writeln!(out, "    pub result_count: u8,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// Emits a named static array for a single shaped recipe's ingredients.
fn emit_shaped_ingredients(out: &mut String, index: usize, recipe: &Shaped) {
    let len = recipe.ingredients.len();
    write!(out, "static SHAPED_{index}_INGR: [Option<i32>; {len}] = [").unwrap();
    for (j, slot) in recipe.ingredients.iter().enumerate() {
        if j > 0 {
            write!(out, ", ").unwrap();
        }
        match slot {
            Some(id) => write!(out, "Some({id})").unwrap(),
            None => write!(out, "None").unwrap(),
        }
    }
    writeln!(out, "];").unwrap();
}

/// Emits a named static array for a single shapeless recipe's ingredients.
fn emit_shapeless_ingredients(out: &mut String, index: usize, recipe: &Shapeless) {
    let len = recipe.ingredients.len();
    write!(out, "static SHAPELESS_{index}_INGR: [i32; {len}] = [").unwrap();
    for (j, id) in recipe.ingredients.iter().enumerate() {
        if j > 0 {
            write!(out, ", ").unwrap();
        }
        write!(out, "{id}").unwrap();
    }
    writeln!(out, "];").unwrap();
}

/// Emits the `SHAPED_RECIPES` static slice referencing all shaped recipes.
fn emit_shaped_slice(out: &mut String, shaped: &[Shaped]) {
    writeln!(
        out,
        "/// All vanilla shaped recipes, indexed for grid-pattern matching."
    )
    .unwrap();
    writeln!(out, "pub static SHAPED_RECIPES: &[ShapedRecipe] = &[").unwrap();
    for (i, recipe) in shaped.iter().enumerate() {
        writeln!(
            out,
            "    ShapedRecipe {{ id: \"minecraft:shaped_{}\", width: {}, height: {}, ingredients: &SHAPED_{}_INGR, result_id: {}, result_count: {} }},",
            i, recipe.width, recipe.height, i, recipe.result_id, recipe.result_count
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();
}

/// Emits the `SHAPELESS_RECIPES` static slice referencing all shapeless recipes.
fn emit_shapeless_slice(out: &mut String, shapeless: &[Shapeless]) {
    writeln!(
        out,
        "/// All vanilla shapeless recipes, indexed for ingredient-set matching."
    )
    .unwrap();
    writeln!(out, "pub static SHAPELESS_RECIPES: &[ShapelessRecipe] = &[").unwrap();
    for (i, recipe) in shapeless.iter().enumerate() {
        writeln!(
            out,
            "    ShapelessRecipe {{ id: \"minecraft:shapeless_{}\", ingredients: &SHAPELESS_{}_INGR, result_id: {}, result_count: {} }},",
            i, i, recipe.result_id, recipe.result_count
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
}

/// Emits the test module with count verification and spot-checks.
fn emit_tests(out: &mut String, shaped: &[Shaped], shapeless: &[Shapeless]) {
    // Find the oak-plank stick recipe for spot-check: result_id 879, count 4,
    // 1x2 grid with two identical plank IDs
    let stick_idx = shaped
        .iter()
        .position(|r| {
            r.result_id == 879
                && r.result_count == 4
                && r.width == 1
                && r.height == 2
                && r.ingredients.iter().all(|s| s.is_some())
                && r.ingredients[0] == r.ingredients[1]
        })
        .expect("Could not find oak-plank stick recipe for spot-check");

    let stick = &shaped[stick_idx];
    let stick_ingredient = stick.ingredients[0].unwrap();

    writeln!(out).unwrap();
    writeln!(out, "#[cfg(test)]").unwrap();
    writeln!(out, "mod tests {{").unwrap();
    writeln!(out, "    use super::*;").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    #[test]").unwrap();
    writeln!(out, "    fn shaped_count() {{").unwrap();
    writeln!(
        out,
        "        assert_eq!(SHAPED_RECIPES.len(), {});",
        shaped.len()
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    #[test]").unwrap();
    writeln!(out, "    fn shapeless_count() {{").unwrap();
    writeln!(
        out,
        "        assert_eq!(SHAPELESS_RECIPES.len(), {});",
        shapeless.len()
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    /// Spot-check: two planks vertically produce 4 sticks."
    )
    .unwrap();
    writeln!(out, "    #[test]").unwrap();
    writeln!(out, "    fn stick_recipe() {{").unwrap();
    writeln!(
        out,
        "        let recipe = SHAPED_RECIPES.iter().find(|r| r.result_id == 879 && r.result_count == 4 && r.width == 1 && r.height == 2).expect(\"stick recipe not found\");"
    )
    .unwrap();
    writeln!(
        out,
        "        assert_eq!(recipe.ingredients, &[Some({stick_ingredient}), Some({stick_ingredient})]);"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    /// Every shapeless recipe has its ingredients sorted ascending."
    )
    .unwrap();
    writeln!(out, "    #[test]").unwrap();
    writeln!(out, "    fn shapeless_ingredients_sorted() {{").unwrap();
    writeln!(out, "        for recipe in SHAPELESS_RECIPES {{").unwrap();
    writeln!(
        out,
        "            assert!(recipe.ingredients.windows(2).all(|w| w[0] <= w[1]),"
    )
    .unwrap();
    writeln!(
        out,
        "                \"ingredients not sorted for result_id {{}}\", recipe.result_id);"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}
