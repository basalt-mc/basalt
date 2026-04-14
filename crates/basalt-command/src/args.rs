//! Command argument types, parsing, and validation.
//!
//! Plugins declare command arguments with types, and the framework
//! handles parsing, validation, error messages, DeclareCommands
//! generation, and TabComplete responses.

use std::collections::HashMap;

/// Argument type for a command parameter.
///
/// Determines how the argument is parsed, validated, and presented
/// in client-side tab-completion (Brigadier tree).
#[derive(Debug, Clone)]
pub enum Arg {
    // --- Brigadier built-in parsers ---
    /// Boolean (true/false). Parser ID 0.
    Boolean,
    /// 64-bit floating point number. Parser ID 2.
    Double,
    /// 64-bit integer. Parser ID 3.
    Integer,
    /// Free-form single-word string. Parser ID 5, mode SINGLE_WORD.
    String,

    // --- Minecraft-specific parsers ---
    /// Entity selector (@a, @p, @r, @e, @s) or player name. Parser ID 6.
    Entity,
    /// Game profile (player name with tab-completion). Parser ID 7.
    GameProfile,
    /// Block position (integer coordinates, supports ~). Parser ID 42.
    BlockPos,
    /// Column position (x z integers). Parser ID 43.
    ColumnPos,
    /// 3D coordinates (supports ~ and ^). Parser ID 40.
    Vec3,
    /// 2D coordinates (x z, supports ~ and ^). Parser ID 41.
    Vec2,
    /// Block state (e.g., `stone`, `oak_planks[axis=x]`). Parser ID 44.
    BlockState,
    /// Item stack (e.g., `diamond_sword{Damage:10}`). Parser ID 46.
    ItemStack,
    /// Chat message (like GreedyString but with @mention support). Parser ID 48.
    Message,
    /// JSON text component. Parser ID 10.
    Component,
    /// Resource location (namespace:path). Parser ID 35.
    ResourceLocation,
    /// UUID. Parser ID 11.
    Uuid,
    /// Yaw and pitch rotation. Parser ID 39.
    Rotation,

    // --- Basalt extensions ---
    /// Fixed set of choices with tab-completion. Uses string parser
    /// with `minecraft:ask_server` suggestions.
    Options(Vec<std::string::String>),
    /// Player name — tab-completes with connected player names.
    /// Uses `minecraft:game_profile` parser (ID 7).
    Player,
}

/// Validation behavior for an argument.
///
/// Controls whether the framework validates the argument before
/// calling the handler, and what error message is shown on failure.
#[derive(Debug, Clone)]
pub enum Validation {
    /// Framework validates and sends a default error message.
    Auto,
    /// Framework validates and sends the custom error message.
    Custom(std::string::String),
    /// No validation — tab-completion still works, but the handler
    /// receives the raw value and manages errors itself.
    Disabled,
}

/// A declared command argument.
#[derive(Debug, Clone)]
pub struct CommandArg {
    /// Argument name shown in the client UI and used as a key.
    pub name: std::string::String,
    /// The argument type (determines parsing and Brigadier node).
    pub arg_type: Arg,
    /// Validation behavior.
    pub validation: Validation,
    /// Whether this argument is required.
    pub required: bool,
}

/// A parsed argument value.
#[derive(Debug, Clone)]
pub enum ArgValue {
    /// A string value.
    String(std::string::String),
    /// A parsed integer.
    Integer(i64),
    /// A parsed double.
    Double(f64),
}

/// Parsed command arguments accessible by name.
///
/// Built by the framework after validating the raw argument string
/// against the command's declared arguments.
#[derive(Debug)]
pub struct CommandArgs {
    values: HashMap<std::string::String, ArgValue>,
    raw: std::string::String,
}

impl CommandArgs {
    /// Creates a new empty argument map.
    pub fn new(raw: std::string::String) -> Self {
        Self {
            values: HashMap::new(),
            raw,
        }
    }

    /// Inserts a parsed value.
    pub fn insert(&mut self, name: std::string::String, value: ArgValue) {
        self.values.insert(name, value);
    }

    /// Gets a string argument by name.
    pub fn get_string(&self, name: &str) -> Option<&str> {
        match self.values.get(name) {
            Some(ArgValue::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Gets an integer argument by name.
    pub fn get_integer(&self, name: &str) -> Option<i64> {
        match self.values.get(name) {
            Some(ArgValue::Integer(v)) => Some(*v),
            _ => None,
        }
    }

    /// Gets a double argument by name.
    pub fn get_double(&self, name: &str) -> Option<f64> {
        match self.values.get(name) {
            Some(ArgValue::Double(v)) => Some(*v),
            _ => None,
        }
    }

    /// Returns the raw argument string before parsing.
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

/// Returns how many tokens a given arg type consumes.
fn token_count(arg: &Arg) -> usize {
    match arg {
        Arg::Vec3 | Arg::BlockPos => 3,
        Arg::Vec2 | Arg::ColumnPos | Arg::Rotation => 2,
        Arg::Message => 0, // greedy — consumes the rest
        _ => 1,
    }
}

/// Parses a raw argument string, trying variants if defined.
///
/// If `variants` is non-empty, tries each variant in order and
/// returns the first successful parse. If all fail, returns the
/// error from the last variant.
pub fn parse_command_args(
    raw: &str,
    schema: &[CommandArg],
    variants: &[Vec<CommandArg>],
) -> Result<CommandArgs, std::string::String> {
    if variants.is_empty() {
        return parse_args(raw, schema);
    }

    // Sort variants by total token count descending — most specific
    // (most tokens consumed) first. This ensures "10 64 -5" matches
    // Vec3 before matching as a Player name.
    let mut sorted: Vec<&Vec<CommandArg>> = variants.iter().collect();
    sorted.sort_by(|a, b| {
        let count_a: usize = a.iter().map(|arg| token_count(&arg.arg_type)).sum();
        let count_b: usize = b.iter().map(|arg| token_count(&arg.arg_type)).sum();
        count_b.cmp(&count_a)
    });

    let mut last_err = String::new();
    for variant in sorted {
        match parse_args(raw, variant) {
            Ok(args) => return Ok(args),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Parses a raw argument string against declared arguments.
pub fn parse_args(raw: &str, schema: &[CommandArg]) -> Result<CommandArgs, std::string::String> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut args = CommandArgs::new(raw.to_string());

    let required_count = schema.iter().filter(|a| a.required).count();
    if tokens.len() < required_count {
        let names: Vec<&str> = schema.iter().map(|a| a.name.as_str()).collect();
        let usage = names
            .iter()
            .map(|n| format!("<{n}>"))
            .collect::<Vec<_>>()
            .join(" ");
        return Err(format!("Usage: {usage}"));
    }

    let mut tok = 0; // current token position

    for arg_def in schema {
        // Message consumes everything from this position onward
        if matches!(arg_def.arg_type, Arg::Message) {
            let remainder: String = tokens[tok..].join(" ");
            if remainder.is_empty() && arg_def.required {
                return Err(format!("Missing required argument: {}", arg_def.name));
            }
            if !remainder.is_empty() {
                args.insert(arg_def.name.clone(), ArgValue::String(remainder));
            }
            break;
        }

        // How many tokens this arg consumes
        let count = match &arg_def.arg_type {
            Arg::Vec3 | Arg::BlockPos => 3,
            Arg::Vec2 | Arg::ColumnPos | Arg::Rotation => 2,
            _ => 1,
        };

        if tok >= tokens.len() {
            if arg_def.required {
                return Err(format!("Missing required argument: {}", arg_def.name));
            }
            continue;
        }

        // Multi-token types: join tokens into a single string value
        if count > 1 {
            if tok + count > tokens.len() {
                if arg_def.required {
                    return Err(format!(
                        "Not enough values for '{}' (expected {count})",
                        arg_def.name
                    ));
                }
                continue;
            }
            let value = tokens[tok..tok + count].join(" ");
            args.insert(arg_def.name.clone(), ArgValue::String(value));
            tok += count;
            continue;
        }

        let token = tokens[tok];
        tok += 1;

        if matches!(arg_def.validation, Validation::Disabled) {
            args.insert(arg_def.name.clone(), ArgValue::String(token.to_string()));
            continue;
        }

        match &arg_def.arg_type {
            Arg::String
            | Arg::Player
            | Arg::Entity
            | Arg::GameProfile
            | Arg::BlockState
            | Arg::ItemStack
            | Arg::Component
            | Arg::ResourceLocation
            | Arg::Uuid => {
                args.insert(arg_def.name.clone(), ArgValue::String(token.to_string()));
            }
            Arg::Integer => match token.parse::<i64>() {
                Ok(v) => {
                    args.insert(arg_def.name.clone(), ArgValue::Integer(v));
                }
                Err(_) => {
                    return Err(match &arg_def.validation {
                        Validation::Custom(msg) => msg.clone(),
                        _ => format!("Expected an integer for '{}'", arg_def.name),
                    });
                }
            },
            Arg::Double => match token.parse::<f64>() {
                Ok(v) => {
                    args.insert(arg_def.name.clone(), ArgValue::Double(v));
                }
                Err(_) => {
                    return Err(match &arg_def.validation {
                        Validation::Custom(msg) => msg.clone(),
                        _ => format!("Expected a number for '{}'", arg_def.name),
                    });
                }
            },
            Arg::Options(choices) => {
                if choices.iter().any(|c| c == token) {
                    args.insert(arg_def.name.clone(), ArgValue::String(token.to_string()));
                } else {
                    return Err(match &arg_def.validation {
                        Validation::Custom(msg) => msg.clone(),
                        _ => {
                            let opts = choices.join(", ");
                            format!("Invalid '{}'. Options: {opts}", arg_def.name)
                        }
                    });
                }
            }
            Arg::Boolean => match token {
                "true" | "false" => {
                    args.insert(arg_def.name.clone(), ArgValue::String(token.to_string()));
                }
                _ => {
                    return Err(match &arg_def.validation {
                        Validation::Custom(msg) => msg.clone(),
                        _ => format!("Expected true/false for '{}'", arg_def.name),
                    });
                }
            },
            // Multi-token and greedy types handled above
            Arg::Vec3
            | Arg::Vec2
            | Arg::BlockPos
            | Arg::ColumnPos
            | Arg::Rotation
            | Arg::Message => {
                unreachable!()
            }
        }
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(name: &str, arg_type: Arg) -> CommandArg {
        CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: true,
        }
    }

    #[test]
    fn parse_double_args() {
        let schema = vec![
            arg("x", Arg::Double),
            arg("y", Arg::Double),
            arg("z", Arg::Double),
        ];
        let result = parse_args("10.5 64.0 -5.0", &schema).unwrap();
        assert_eq!(result.get_double("x"), Some(10.5));
        assert_eq!(result.get_double("y"), Some(64.0));
        assert_eq!(result.get_double("z"), Some(-5.0));
    }

    #[test]
    fn parse_integer_args() {
        let schema = vec![arg("count", Arg::Integer)];
        let result = parse_args("42", &schema).unwrap();
        assert_eq!(result.get_integer("count"), Some(42));
    }

    #[test]
    fn parse_string_arg() {
        let schema = vec![arg("name", Arg::String)];
        let result = parse_args("Steve", &schema).unwrap();
        assert_eq!(result.get_string("name"), Some("Steve"));
    }

    #[test]
    fn parse_options_valid() {
        let schema = vec![arg(
            "mode",
            Arg::Options(vec!["survival".into(), "creative".into()]),
        )];
        let result = parse_args("creative", &schema).unwrap();
        assert_eq!(result.get_string("mode"), Some("creative"));
    }

    #[test]
    fn parse_options_invalid() {
        let schema = vec![arg(
            "mode",
            Arg::Options(vec!["survival".into(), "creative".into()]),
        )];
        let err = parse_args("hardcore", &schema).unwrap_err();
        assert!(err.contains("Invalid 'mode'"));
    }

    #[test]
    fn parse_options_custom_error() {
        let schema = vec![CommandArg {
            name: "mode".into(),
            arg_type: Arg::Options(vec!["survival".into(), "creative".into()]),
            validation: Validation::Custom("Nope, bad mode".into()),
            required: true,
        }];
        let err = parse_args("hardcore", &schema).unwrap_err();
        assert_eq!(err, "Nope, bad mode");
    }

    #[test]
    fn parse_double_invalid() {
        let schema = vec![arg("x", Arg::Double)];
        let err = parse_args("abc", &schema).unwrap_err();
        assert!(err.contains("Expected a number"));
    }

    #[test]
    fn parse_too_few_args() {
        let schema = vec![
            arg("x", Arg::Double),
            arg("y", Arg::Double),
            arg("z", Arg::Double),
        ];
        let err = parse_args("10.5", &schema).unwrap_err();
        assert!(err.contains("Usage:"));
    }

    #[test]
    fn parse_validation_disabled() {
        let schema = vec![CommandArg {
            name: "value".into(),
            arg_type: Arg::Double,
            validation: Validation::Disabled,
            required: true,
        }];
        let result = parse_args("abc", &schema).unwrap();
        assert_eq!(result.get_string("value"), Some("abc"));
    }

    #[test]
    fn parse_optional_arg_missing() {
        let schema = vec![CommandArg {
            name: "target".into(),
            arg_type: Arg::String,
            validation: Validation::Auto,
            required: false,
        }];
        let result = parse_args("", &schema).unwrap();
        assert_eq!(result.get_string("target"), None);
    }

    #[test]
    fn parse_greedy_string() {
        let schema = vec![arg("msg", Arg::Message)];
        let result = parse_args("hello world foo", &schema).unwrap();
        assert_eq!(result.get_string("msg"), Some("hello world foo"));
    }

    #[test]
    fn parse_boolean_valid() {
        let schema = vec![arg("flag", Arg::Boolean)];
        let result = parse_args("true", &schema).unwrap();
        assert_eq!(result.get_string("flag"), Some("true"));
    }

    #[test]
    fn parse_boolean_invalid() {
        let schema = vec![arg("flag", Arg::Boolean)];
        let err = parse_args("maybe", &schema).unwrap_err();
        assert!(err.contains("Expected true/false"));
    }

    #[test]
    fn parse_player_arg() {
        let schema = vec![arg("target", Arg::Player)];
        let result = parse_args("Steve", &schema).unwrap();
        assert_eq!(result.get_string("target"), Some("Steve"));
    }

    #[test]
    fn parse_variants_first_match() {
        let v1 = vec![arg("x", Arg::Double), arg("y", Arg::Double)];
        let v2 = vec![arg("name", Arg::String)];
        let result = parse_command_args("10.5 20.0", &[], &[v1, v2]).unwrap();
        assert_eq!(result.get_double("x"), Some(10.5));
    }

    #[test]
    fn parse_variants_second_match() {
        let v1 = vec![arg("x", Arg::Double), arg("y", Arg::Double)];
        let v2 = vec![arg("name", Arg::Player)];
        let result = parse_command_args("Steve", &[], &[v1, v2]).unwrap();
        assert_eq!(result.get_string("name"), Some("Steve"));
    }

    #[test]
    fn raw_preserved() {
        let schema = vec![arg("msg", Arg::String)];
        let result = parse_args("hello world", &schema).unwrap();
        assert_eq!(result.raw(), "hello world");
    }
}
