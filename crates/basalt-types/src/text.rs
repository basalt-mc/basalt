use crate::nbt::{NbtCompound, NbtList, NbtTag};
use crate::{Decode, Encode, EncodedSize, Error, Result};

/// A rich text component used for chat messages, disconnect reasons,
/// action bars, titles, and all UI text in the Minecraft protocol.
///
/// TextComponent is a recursive tree structure: each component has content,
/// optional styling, and an optional list of child components (`extra`).
/// Children inherit their parent's style unless they override specific fields.
///
/// Since Minecraft 1.20.3, TextComponent is encoded as NBT on the wire
/// (previously JSON). The `Encode`/`Decode` implementations convert
/// to/from `NbtCompound` using the network NBT format.
#[derive(Debug, Clone, PartialEq)]
pub struct TextComponent {
    /// The content of this text component (text, translation, keybind, etc.).
    pub content: TextContent,

    /// Styling applied to this component. `None` fields inherit from the parent.
    pub style: TextStyle,

    /// Child components appended after this one, inheriting its style.
    pub extra: Vec<TextComponent>,
}

/// The content payload of a text component.
///
/// Determines what text is displayed. Only one content type is active
/// per component. The most common is `Text` for literal strings and
/// `Translate` for server-side localization.
#[derive(Debug, Clone, PartialEq)]
pub enum TextContent {
    /// A literal text string displayed as-is.
    Text(String),

    /// A translation key resolved by the client's language file.
    /// `with` contains substitution arguments inserted into the template.
    Translate {
        /// The translation key (e.g., `chat.type.text`).
        key: String,
        /// Substitution arguments for the translation template.
        with: Vec<TextComponent>,
    },

    /// A keybind name resolved to the player's current key binding
    /// (e.g., `key.jump` displays whatever key the player has bound to jump).
    Keybind(String),

    /// A scoreboard value resolved by the server.
    Score {
        /// The entity selector or player name whose score to display.
        name: String,
        /// The scoreboard objective to read from.
        objective: String,
    },

    /// An entity selector resolved by the server into matching entity names.
    Selector(String),
}

/// Visual styling for a text component.
///
/// All fields are `Option` — `None` means the value is inherited from the
/// parent component. The root component inherits from the client's default
/// chat style (typically white, non-bold, non-italic).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextStyle {
    /// The text color. Overrides the parent's color when set.
    pub color: Option<TextColor>,
    /// Bold formatting. Renders the text with a thicker stroke.
    pub bold: Option<bool>,
    /// Italic formatting. Renders the text at a slant.
    pub italic: Option<bool>,
    /// Underlined formatting. Draws a line beneath the text.
    pub underlined: Option<bool>,
    /// Strikethrough formatting. Draws a line through the text.
    pub strikethrough: Option<bool>,
    /// Obfuscated formatting. Rapidly cycles through random characters.
    pub obfuscated: Option<bool>,
    /// Text inserted into the chat input when the component is shift-clicked.
    pub insertion: Option<String>,
    /// Action triggered when the component is clicked.
    pub click_event: Option<ClickEvent>,
    /// Content displayed when the component is hovered.
    pub hover_event: Option<HoverEvent>,
}

/// Text color, either a named Minecraft color or an arbitrary hex RGB value.
#[derive(Debug, Clone, PartialEq)]
pub enum TextColor {
    /// One of the 16 built-in Minecraft chat colors.
    Named(NamedColor),
    /// An arbitrary RGB color specified as a `#RRGGBB` hex string.
    /// Stored as the raw 24-bit integer (0x000000 to 0xFFFFFF).
    Hex(u32),
}

/// The 16 built-in Minecraft chat colors plus reset.
///
/// Each color maps to a specific RGB value in the client's rendering.
/// These names are used in the NBT `color` field as lowercase strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedColor {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
}

impl NamedColor {
    /// Returns the NBT string representation of this color.
    fn as_str(&self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::DarkBlue => "dark_blue",
            Self::DarkGreen => "dark_green",
            Self::DarkAqua => "dark_aqua",
            Self::DarkRed => "dark_red",
            Self::DarkPurple => "dark_purple",
            Self::Gold => "gold",
            Self::Gray => "gray",
            Self::DarkGray => "dark_gray",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Aqua => "aqua",
            Self::Red => "red",
            Self::LightPurple => "light_purple",
            Self::Yellow => "yellow",
            Self::White => "white",
        }
    }

    /// Parses a named color from its NBT string representation.
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "black" => Some(Self::Black),
            "dark_blue" => Some(Self::DarkBlue),
            "dark_green" => Some(Self::DarkGreen),
            "dark_aqua" => Some(Self::DarkAqua),
            "dark_red" => Some(Self::DarkRed),
            "dark_purple" => Some(Self::DarkPurple),
            "gold" => Some(Self::Gold),
            "gray" => Some(Self::Gray),
            "dark_gray" => Some(Self::DarkGray),
            "blue" => Some(Self::Blue),
            "green" => Some(Self::Green),
            "aqua" => Some(Self::Aqua),
            "red" => Some(Self::Red),
            "light_purple" => Some(Self::LightPurple),
            "yellow" => Some(Self::Yellow),
            "white" => Some(Self::White),
            _ => None,
        }
    }
}

/// An action triggered when the player clicks a text component.
#[derive(Debug, Clone, PartialEq)]
pub enum ClickEvent {
    /// Opens the given URL in the player's browser.
    OpenUrl(String),
    /// Sends the given string as a chat command.
    RunCommand(String),
    /// Inserts the given string into the chat input without sending.
    SuggestCommand(String),
    /// Copies the given string to the clipboard.
    CopyToClipboard(String),
}

/// Content displayed when the player hovers over a text component.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverEvent {
    /// Shows another text component as a tooltip.
    ShowText(Box<TextComponent>),
    /// Shows an item tooltip with ID, count, and optional NBT tag.
    ShowItem {
        /// The item identifier (e.g., `minecraft:diamond_sword`).
        id: String,
        /// The item stack count.
        count: i32,
        /// Optional item NBT data as a serialized string.
        tag: Option<String>,
    },
    /// Shows an entity tooltip with UUID, type, and optional custom name.
    ShowEntity {
        /// The entity's UUID as a string.
        id: String,
        /// The entity type identifier (e.g., `minecraft:creeper`).
        type_id: String,
        /// The entity's custom name, if any.
        name: Option<Box<TextComponent>>,
    },
}

// -- Convenience constructors --

impl TextComponent {
    /// Creates a plain text component with no styling.
    ///
    /// This is the most common way to create a text component for simple
    /// messages. The text is displayed as-is with the default chat style.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: TextContent::Text(text.into()),
            style: TextStyle::default(),
            extra: Vec::new(),
        }
    }

    /// Creates a translation component with substitution arguments.
    ///
    /// The client resolves the key against its language file and inserts
    /// the `with` components at the template's substitution points.
    pub fn translate(key: impl Into<String>, with: Vec<TextComponent>) -> Self {
        Self {
            content: TextContent::Translate {
                key: key.into(),
                with,
            },
            style: TextStyle::default(),
            extra: Vec::new(),
        }
    }

    /// Sets the text color. Returns self for builder-style chaining.
    pub fn color(mut self, color: TextColor) -> Self {
        self.style.color = Some(color);
        self
    }

    /// Sets bold formatting. Returns self for builder-style chaining.
    pub fn bold(mut self, bold: bool) -> Self {
        self.style.bold = Some(bold);
        self
    }

    /// Sets italic formatting. Returns self for builder-style chaining.
    pub fn italic(mut self, italic: bool) -> Self {
        self.style.italic = Some(italic);
        self
    }

    /// Sets underlined formatting. Returns self for builder-style chaining.
    pub fn underlined(mut self, underlined: bool) -> Self {
        self.style.underlined = Some(underlined);
        self
    }

    /// Sets strikethrough formatting. Returns self for builder-style chaining.
    pub fn strikethrough(mut self, strikethrough: bool) -> Self {
        self.style.strikethrough = Some(strikethrough);
        self
    }

    /// Sets obfuscated formatting. Returns self for builder-style chaining.
    pub fn obfuscated(mut self, obfuscated: bool) -> Self {
        self.style.obfuscated = Some(obfuscated);
        self
    }

    /// Sets the click event. Returns self for builder-style chaining.
    pub fn click_event(mut self, event: ClickEvent) -> Self {
        self.style.click_event = Some(event);
        self
    }

    /// Sets the hover event. Returns self for builder-style chaining.
    pub fn hover_event(mut self, event: HoverEvent) -> Self {
        self.style.hover_event = Some(event);
        self
    }

    /// Appends a child component. Returns self for builder-style chaining.
    pub fn append(mut self, child: TextComponent) -> Self {
        self.extra.push(child);
        self
    }
}

// -- NBT conversion --

/// Converts a TextComponent into an NbtCompound for wire encoding.
fn component_to_nbt(component: &TextComponent) -> NbtCompound {
    let mut nbt = NbtCompound::new();

    // Content
    match &component.content {
        TextContent::Text(text) => {
            nbt.insert("text", NbtTag::String(text.clone()));
        }
        TextContent::Translate { key, with } => {
            nbt.insert("translate", NbtTag::String(key.clone()));
            if !with.is_empty() {
                let list_tags: Vec<NbtTag> = with
                    .iter()
                    .map(|c| NbtTag::Compound(component_to_nbt(c)))
                    .collect();
                let list = NbtList::from_tags(list_tags).unwrap();
                nbt.insert("with", NbtTag::List(list));
            }
        }
        TextContent::Keybind(key) => {
            nbt.insert("keybind", NbtTag::String(key.clone()));
        }
        TextContent::Score { name, objective } => {
            let mut score = NbtCompound::new();
            score.insert("name", NbtTag::String(name.clone()));
            score.insert("objective", NbtTag::String(objective.clone()));
            nbt.insert("score", NbtTag::Compound(score));
        }
        TextContent::Selector(selector) => {
            nbt.insert("selector", NbtTag::String(selector.clone()));
        }
    }

    // Style
    let style = &component.style;
    if let Some(color) = &style.color {
        let color_str = match color {
            TextColor::Named(named) => named.as_str().to_string(),
            TextColor::Hex(rgb) => format!("#{rgb:06x}"),
        };
        nbt.insert("color", NbtTag::String(color_str));
    }
    if let Some(bold) = style.bold {
        nbt.insert("bold", NbtTag::Byte(bold as i8));
    }
    if let Some(italic) = style.italic {
        nbt.insert("italic", NbtTag::Byte(italic as i8));
    }
    if let Some(underlined) = style.underlined {
        nbt.insert("underlined", NbtTag::Byte(underlined as i8));
    }
    if let Some(strikethrough) = style.strikethrough {
        nbt.insert("strikethrough", NbtTag::Byte(strikethrough as i8));
    }
    if let Some(obfuscated) = style.obfuscated {
        nbt.insert("obfuscated", NbtTag::Byte(obfuscated as i8));
    }
    if let Some(insertion) = &style.insertion {
        nbt.insert("insertion", NbtTag::String(insertion.clone()));
    }
    if let Some(click) = &style.click_event {
        let mut event = NbtCompound::new();
        let (action, value) = match click {
            ClickEvent::OpenUrl(url) => ("open_url", url.clone()),
            ClickEvent::RunCommand(cmd) => ("run_command", cmd.clone()),
            ClickEvent::SuggestCommand(cmd) => ("suggest_command", cmd.clone()),
            ClickEvent::CopyToClipboard(text) => ("copy_to_clipboard", text.clone()),
        };
        event.insert("action", NbtTag::String(action.into()));
        event.insert("value", NbtTag::String(value));
        nbt.insert("clickEvent", NbtTag::Compound(event));
    }
    if let Some(hover) = &style.hover_event {
        let mut event = NbtCompound::new();
        match hover {
            HoverEvent::ShowText(text) => {
                event.insert("action", NbtTag::String("show_text".into()));
                event.insert("contents", NbtTag::Compound(component_to_nbt(text)));
            }
            HoverEvent::ShowItem { id, count, tag } => {
                event.insert("action", NbtTag::String("show_item".into()));
                let mut contents = NbtCompound::new();
                contents.insert("id", NbtTag::String(id.clone()));
                contents.insert("count", NbtTag::Int(*count));
                if let Some(tag) = tag {
                    contents.insert("tag", NbtTag::String(tag.clone()));
                }
                event.insert("contents", NbtTag::Compound(contents));
            }
            HoverEvent::ShowEntity { id, type_id, name } => {
                event.insert("action", NbtTag::String("show_entity".into()));
                let mut contents = NbtCompound::new();
                contents.insert("type", NbtTag::String(type_id.clone()));
                contents.insert("id", NbtTag::String(id.clone()));
                if let Some(name) = name {
                    contents.insert("name", NbtTag::Compound(component_to_nbt(name)));
                }
                event.insert("contents", NbtTag::Compound(contents));
            }
        }
        nbt.insert("hoverEvent", NbtTag::Compound(event));
    }

    // Extra
    if !component.extra.is_empty() {
        let list_tags: Vec<NbtTag> = component
            .extra
            .iter()
            .map(|c| NbtTag::Compound(component_to_nbt(c)))
            .collect();
        let list = NbtList::from_tags(list_tags).unwrap();
        nbt.insert("extra", NbtTag::List(list));
    }

    nbt
}

/// Parses a TextComponent from an NbtCompound.
fn component_from_nbt(nbt: &NbtCompound) -> Result<TextComponent> {
    // Content — determine type by which key is present
    let content = if let Some(NbtTag::String(text)) = nbt.get("text") {
        TextContent::Text(text.clone())
    } else if let Some(NbtTag::String(key)) = nbt.get("translate") {
        let with = if let Some(NbtTag::List(list)) = nbt.get("with") {
            let mut components = Vec::new();
            for tag in &list.elements {
                if let NbtTag::Compound(c) = tag {
                    components.push(component_from_nbt(c)?);
                }
            }
            components
        } else {
            Vec::new()
        };
        TextContent::Translate {
            key: key.clone(),
            with,
        }
    } else if let Some(NbtTag::String(key)) = nbt.get("keybind") {
        TextContent::Keybind(key.clone())
    } else if let Some(NbtTag::Compound(score)) = nbt.get("score") {
        let name = match score.get("name") {
            Some(NbtTag::String(s)) => s.clone(),
            _ => return Err(Error::Nbt("score missing 'name'".into())),
        };
        let objective = match score.get("objective") {
            Some(NbtTag::String(s)) => s.clone(),
            _ => return Err(Error::Nbt("score missing 'objective'".into())),
        };
        TextContent::Score { name, objective }
    } else if let Some(NbtTag::String(selector)) = nbt.get("selector") {
        TextContent::Selector(selector.clone())
    } else {
        // Default to empty text if no content key is found
        TextContent::Text(String::new())
    };

    // Style
    let mut style = TextStyle::default();

    if let Some(NbtTag::String(color_str)) = nbt.get("color") {
        if let Some(named) = NamedColor::from_str(color_str) {
            style.color = Some(TextColor::Named(named));
        } else if let Some(hex) = color_str.strip_prefix('#')
            && let Ok(rgb) = u32::from_str_radix(hex, 16)
        {
            style.color = Some(TextColor::Hex(rgb));
        }
    }

    fn read_bool(nbt: &NbtCompound, key: &str) -> Option<bool> {
        match nbt.get(key) {
            Some(NbtTag::Byte(v)) => Some(*v != 0),
            _ => None,
        }
    }

    style.bold = read_bool(nbt, "bold");
    style.italic = read_bool(nbt, "italic");
    style.underlined = read_bool(nbt, "underlined");
    style.strikethrough = read_bool(nbt, "strikethrough");
    style.obfuscated = read_bool(nbt, "obfuscated");

    if let Some(NbtTag::String(insertion)) = nbt.get("insertion") {
        style.insertion = Some(insertion.clone());
    }

    if let Some(NbtTag::Compound(event)) = nbt.get("clickEvent")
        && let (Some(NbtTag::String(action)), Some(NbtTag::String(value))) =
            (event.get("action"), event.get("value"))
    {
        style.click_event = match action.as_str() {
            "open_url" => Some(ClickEvent::OpenUrl(value.clone())),
            "run_command" => Some(ClickEvent::RunCommand(value.clone())),
            "suggest_command" => Some(ClickEvent::SuggestCommand(value.clone())),
            "copy_to_clipboard" => Some(ClickEvent::CopyToClipboard(value.clone())),
            _ => None,
        };
    }

    if let Some(NbtTag::Compound(event)) = nbt.get("hoverEvent")
        && let Some(NbtTag::String(action)) = event.get("action")
    {
        style.hover_event = match action.as_str() {
            "show_text" => {
                if let Some(NbtTag::Compound(contents)) = event.get("contents") {
                    Some(HoverEvent::ShowText(Box::new(component_from_nbt(
                        contents,
                    )?)))
                } else {
                    None
                }
            }
            "show_item" => {
                if let Some(NbtTag::Compound(contents)) = event.get("contents") {
                    let id = match contents.get("id") {
                        Some(NbtTag::String(s)) => s.clone(),
                        _ => return Err(Error::Nbt("show_item missing 'id'".into())),
                    };
                    let count = match contents.get("count") {
                        Some(NbtTag::Int(n)) => *n,
                        _ => 1,
                    };
                    let tag = match contents.get("tag") {
                        Some(NbtTag::String(s)) => Some(s.clone()),
                        _ => None,
                    };
                    Some(HoverEvent::ShowItem { id, count, tag })
                } else {
                    None
                }
            }
            "show_entity" => {
                if let Some(NbtTag::Compound(contents)) = event.get("contents") {
                    let id = match contents.get("id") {
                        Some(NbtTag::String(s)) => s.clone(),
                        _ => return Err(Error::Nbt("show_entity missing 'id'".into())),
                    };
                    let type_id = match contents.get("type") {
                        Some(NbtTag::String(s)) => s.clone(),
                        _ => return Err(Error::Nbt("show_entity missing 'type'".into())),
                    };
                    let name = if let Some(NbtTag::Compound(name_nbt)) = contents.get("name") {
                        Some(Box::new(component_from_nbt(name_nbt)?))
                    } else {
                        None
                    };
                    Some(HoverEvent::ShowEntity { id, type_id, name })
                } else {
                    None
                }
            }
            _ => None,
        };
    }

    // Extra
    let extra = if let Some(NbtTag::List(list)) = nbt.get("extra") {
        let mut children = Vec::new();
        for tag in &list.elements {
            if let NbtTag::Compound(c) = tag {
                children.push(component_from_nbt(c)?);
            }
        }
        children
    } else {
        Vec::new()
    };

    Ok(TextComponent {
        content,
        style,
        extra,
    })
}

/// Encodes a TextComponent as network NBT (compound tag).
///
/// Converts the component tree into an NbtCompound, then encodes it
/// using the network NBT format (1.20.3+). The resulting bytes can be
/// used directly in protocol packets for chat, disconnect, title, etc.
impl Encode for TextComponent {
    /// Serializes the component to network NBT format.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        let nbt = component_to_nbt(self);
        nbt.encode(buf)
    }
}

/// Decodes a TextComponent from network NBT (compound tag).
///
/// Reads a network NBT compound, then parses it into a TextComponent tree.
/// Handles all content types (text, translate, keybind, score, selector),
/// all style fields, click/hover events, and recursive extra children.
impl Decode for TextComponent {
    /// Deserializes a TextComponent from network NBT format.
    ///
    /// Fails with `Error::Nbt` if required fields are missing or have
    /// unexpected types.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let nbt = NbtCompound::decode(buf)?;
        component_from_nbt(&nbt)
    }
}

/// Computes the wire size of a TextComponent as network NBT.
///
/// Converts to NbtCompound and delegates to its EncodedSize. This involves
/// building the full NBT tree, so it is not free — use sparingly or cache
/// the result when encoding multiple times.
impl EncodedSize for TextComponent {
    /// Returns the byte count of the network NBT encoding.
    fn encoded_size(&self) -> usize {
        let nbt = component_to_nbt(self);
        nbt.encoded_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(component: &TextComponent) {
        let mut buf = Vec::with_capacity(component.encoded_size());
        component.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), component.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = TextComponent::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, *component);
    }

    // -- Plain text --

    #[test]
    fn plain_text() {
        roundtrip(&TextComponent::text("hello"));
    }

    #[test]
    fn empty_text() {
        roundtrip(&TextComponent::text(""));
    }

    // -- Styled text --

    #[test]
    fn bold_red_text() {
        let tc = TextComponent::text("warning")
            .bold(true)
            .color(TextColor::Named(NamedColor::Red));
        roundtrip(&tc);
    }

    #[test]
    fn all_formatting() {
        let tc = TextComponent::text("styled")
            .bold(true)
            .italic(true)
            .underlined(true)
            .strikethrough(true)
            .obfuscated(true);
        roundtrip(&tc);
    }

    #[test]
    fn hex_color() {
        let tc = TextComponent::text("custom color").color(TextColor::Hex(0xFF5500));
        roundtrip(&tc);
    }

    #[test]
    fn all_named_colors() {
        let colors = [
            NamedColor::Black,
            NamedColor::DarkBlue,
            NamedColor::DarkGreen,
            NamedColor::DarkAqua,
            NamedColor::DarkRed,
            NamedColor::DarkPurple,
            NamedColor::Gold,
            NamedColor::Gray,
            NamedColor::DarkGray,
            NamedColor::Blue,
            NamedColor::Green,
            NamedColor::Aqua,
            NamedColor::Red,
            NamedColor::LightPurple,
            NamedColor::Yellow,
            NamedColor::White,
        ];
        for color in colors {
            let tc = TextComponent::text("test").color(TextColor::Named(color));
            roundtrip(&tc);
        }
    }

    #[test]
    fn insertion() {
        let tc = TextComponent {
            content: TextContent::Text("click me".into()),
            style: TextStyle {
                insertion: Some("/help".into()),
                ..Default::default()
            },
            extra: Vec::new(),
        };
        roundtrip(&tc);
    }

    // -- Extra children --

    #[test]
    fn with_extra() {
        let tc = TextComponent::text("hello ").append(TextComponent::text("world").bold(true));
        roundtrip(&tc);
    }

    #[test]
    fn nested_extra() {
        let tc = TextComponent::text("a")
            .append(TextComponent::text("b").append(TextComponent::text("c").italic(true)));
        roundtrip(&tc);
    }

    // -- Content types --

    #[test]
    fn translate_no_args() {
        let tc = TextComponent::translate("multiplayer.disconnect.kicked", vec![]);
        roundtrip(&tc);
    }

    #[test]
    fn translate_with_args() {
        let tc = TextComponent::translate(
            "chat.type.text",
            vec![
                TextComponent::text("Player1"),
                TextComponent::text("Hello!"),
            ],
        );
        roundtrip(&tc);
    }

    #[test]
    fn keybind() {
        let tc = TextComponent {
            content: TextContent::Keybind("key.jump".into()),
            style: TextStyle::default(),
            extra: Vec::new(),
        };
        roundtrip(&tc);
    }

    #[test]
    fn score() {
        let tc = TextComponent {
            content: TextContent::Score {
                name: "Player1".into(),
                objective: "kills".into(),
            },
            style: TextStyle::default(),
            extra: Vec::new(),
        };
        roundtrip(&tc);
    }

    #[test]
    fn selector() {
        let tc = TextComponent {
            content: TextContent::Selector("@a[distance=..10]".into()),
            style: TextStyle::default(),
            extra: Vec::new(),
        };
        roundtrip(&tc);
    }

    // -- Click events --

    #[test]
    fn click_open_url() {
        let tc = TextComponent::text("click here")
            .click_event(ClickEvent::OpenUrl("https://minecraft.net".into()));
        roundtrip(&tc);
    }

    #[test]
    fn click_run_command() {
        let tc = TextComponent::text("run")
            .click_event(ClickEvent::RunCommand("/gamemode creative".into()));
        roundtrip(&tc);
    }

    #[test]
    fn click_suggest_command() {
        let tc =
            TextComponent::text("suggest").click_event(ClickEvent::SuggestCommand("/tp ".into()));
        roundtrip(&tc);
    }

    #[test]
    fn click_copy() {
        let tc =
            TextComponent::text("copy").click_event(ClickEvent::CopyToClipboard("secret".into()));
        roundtrip(&tc);
    }

    // -- Hover events --

    #[test]
    fn hover_show_text() {
        let tc = TextComponent::text("hover me").hover_event(HoverEvent::ShowText(Box::new(
            TextComponent::text("tooltip").color(TextColor::Named(NamedColor::Yellow)),
        )));
        roundtrip(&tc);
    }

    #[test]
    fn hover_show_item() {
        let tc = TextComponent::text("item").hover_event(HoverEvent::ShowItem {
            id: "minecraft:diamond_sword".into(),
            count: 1,
            tag: Some("{Damage:10}".into()),
        });
        roundtrip(&tc);
    }

    #[test]
    fn hover_show_item_no_tag() {
        let tc = TextComponent::text("item").hover_event(HoverEvent::ShowItem {
            id: "minecraft:stone".into(),
            count: 64,
            tag: None,
        });
        roundtrip(&tc);
    }

    #[test]
    fn hover_show_entity() {
        let tc = TextComponent::text("entity").hover_event(HoverEvent::ShowEntity {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            type_id: "minecraft:creeper".into(),
            name: Some(Box::new(
                TextComponent::text("Bob").color(TextColor::Named(NamedColor::Green)),
            )),
        });
        roundtrip(&tc);
    }

    #[test]
    fn hover_show_entity_no_name() {
        let tc = TextComponent::text("entity").hover_event(HoverEvent::ShowEntity {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            type_id: "minecraft:zombie".into(),
            name: None,
        });
        roundtrip(&tc);
    }

    // -- Complex --

    #[test]
    fn complex_component() {
        let tc = TextComponent::text("[")
            .color(TextColor::Named(NamedColor::Gray))
            .append(
                TextComponent::text("Server")
                    .color(TextColor::Named(NamedColor::Gold))
                    .bold(true),
            )
            .append(TextComponent::text("] ").color(TextColor::Named(NamedColor::Gray)))
            .append(
                TextComponent::text("Welcome!")
                    .color(TextColor::Named(NamedColor::White))
                    .click_event(ClickEvent::RunCommand("/help".into()))
                    .hover_event(HoverEvent::ShowText(Box::new(TextComponent::text(
                        "Click for help",
                    )))),
            );
        roundtrip(&tc);
    }
}
