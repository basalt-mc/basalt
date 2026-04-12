use basalt_derive::{Decode, Encode, EncodedSize, packet};
use basalt_types::{Decode as _, Encode as _, EncodedSize as _, Error, VarInt};

// -- Basic struct --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct SimpleStruct {
    x: i32,
    y: i32,
    z: i32,
}

#[test]
fn simple_struct_roundtrip() {
    let original = SimpleStruct {
        x: 100,
        y: -200,
        z: 300,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = SimpleStruct::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Packet ID --

#[derive(Debug, PartialEq)]
#[packet(id = 0x00)]
struct HandshakePacket {
    #[field(varint)]
    protocol_version: i32,
    server_address: String,
    server_port: u16,
    #[field(varint)]
    next_state: i32,
}

#[test]
fn packet_id_constant() {
    assert_eq!(HandshakePacket::PACKET_ID, 0x00);
}

#[test]
fn packet_with_id_roundtrip() {
    let original = HandshakePacket {
        protocol_version: 763,
        server_address: "localhost".into(),
        server_port: 25565,
        next_state: 1,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    // Packet ID is NOT in the wire format — only fields are encoded
    let mut cursor = buf.as_slice();
    let decoded = HandshakePacket::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- VarInt field --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct VarIntFields {
    #[field(varint)]
    small: i32,
    #[field(varint)]
    large: i32,
}

#[test]
fn varint_field_roundtrip() {
    let original = VarIntFields {
        small: 1,
        large: 300000,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), 4);
    assert_eq!(original.encoded_size(), 4);

    let mut cursor = buf.as_slice();
    let decoded = VarIntFields::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Optional field --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct OptionalFields {
    #[field(optional)]
    name: Option<String>,
    value: i32,
}

#[test]
fn optional_present_roundtrip() {
    let original = OptionalFields {
        name: Some("hello".into()),
        value: 42,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = OptionalFields::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

#[test]
fn optional_absent_roundtrip() {
    let original = OptionalFields {
        name: None,
        value: 42,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = OptionalFields::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Length-prefixed Vec --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct LengthPrefixed {
    #[field(length = "varint")]
    items: Vec<i32>,
}

#[test]
fn length_prefixed_roundtrip() {
    let original = LengthPrefixed {
        items: vec![10, 20, 30],
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = LengthPrefixed::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

#[test]
fn length_prefixed_empty_roundtrip() {
    let original = LengthPrefixed { items: vec![] };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = LengthPrefixed::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Element VarInt Vec --

/// Tests `#[field(length = "varint", element = "varint")]` which
/// encodes each element as a VarInt instead of using the default
/// big-endian i32 encoding.
#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct VarIntArray {
    #[field(length = "varint", element = "varint")]
    ids: Vec<i32>,
}

#[test]
fn element_varint_roundtrip() {
    let original = VarIntArray {
        ids: vec![1, 127, 128, 25565, -1],
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    // Verify wire format: VarInt(5) + 5 VarInts, NOT 5 * 4-byte i32s
    // VarInt(1)=1 byte, VarInt(127)=1, VarInt(128)=2, VarInt(25565)=3, VarInt(-1)=5
    // Total = 1 (length) + 1 + 1 + 2 + 3 + 5 = 13 bytes
    assert_eq!(buf.len(), 13);

    let mut cursor = buf.as_slice();
    let decoded = VarIntArray::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

#[test]
fn element_varint_empty() {
    let original = VarIntArray { ids: vec![] };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), 1); // just VarInt(0)

    let mut cursor = buf.as_slice();
    let decoded = VarIntArray::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Rest field --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct WithRest {
    header: u8,
    #[field(rest)]
    data: Vec<u8>,
}

#[test]
fn rest_field_roundtrip() {
    let original = WithRest {
        header: 0xFF,
        data: vec![1, 2, 3, 4, 5],
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = WithRest::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

#[test]
fn rest_field_empty() {
    let original = WithRest {
        header: 0x01,
        data: vec![],
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = WithRest::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Enum with unit variants --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
enum NextState {
    Status,
    Login,
    Transfer,
}

#[test]
fn enum_unit_roundtrip() {
    for (variant, expected_id) in [
        (NextState::Status, 0),
        (NextState::Login, 1),
        (NextState::Transfer, 2),
    ] {
        let mut buf = Vec::new();
        variant.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let id = VarInt::decode(&mut check).unwrap();
        assert_eq!(id.0, expected_id);

        let mut cursor = buf.as_slice();
        let decoded = NextState::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, variant);
    }
}

// -- Enum with explicit discriminants --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
enum GameMode {
    #[variant(id = -1)]
    Undefined,
    #[variant(id = 0)]
    Survival,
    #[variant(id = 1)]
    Creative,
    #[variant(id = 2)]
    Adventure,
    #[variant(id = 3)]
    Spectator,
}

#[test]
fn enum_explicit_id_roundtrip() {
    for (variant, expected_id) in [
        (GameMode::Undefined, -1),
        (GameMode::Survival, 0),
        (GameMode::Creative, 1),
        (GameMode::Adventure, 2),
        (GameMode::Spectator, 3),
    ] {
        let mut buf = Vec::new();
        variant.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let id = VarInt::decode(&mut check).unwrap();
        assert_eq!(id.0, expected_id);

        let mut cursor = buf.as_slice();
        let decoded = GameMode::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, variant);
    }
}

// -- Enum with data variants --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
enum ChatMessage {
    Text { message: String },
    Command { command: String },
}

#[test]
fn enum_data_roundtrip() {
    let original = ChatMessage::Text {
        message: "hello".into(),
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = ChatMessage::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Enum with #[field] attributes in variants --

/// Switch-like enum where variant fields use `#[field(varint)]` and
/// `#[field(optional)]` attributes, matching the pattern generated by
/// the codegen for Minecraft protocol switch types.
#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
enum UseEntityAction {
    #[variant(id = 0)]
    Interact {
        #[field(varint)]
        hand: i32,
    },
    #[variant(id = 1)]
    Attack,
    #[variant(id = 2)]
    InteractAt {
        x: f32,
        y: f32,
        z: f32,
        #[field(varint)]
        hand: i32,
    },
}

#[test]
fn enum_variant_field_attrs_interact() {
    let original = UseEntityAction::Interact { hand: 1 };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    // Should be VarInt(0) + VarInt(1)
    let mut cursor = buf.as_slice();
    let id = VarInt::decode(&mut cursor).unwrap();
    assert_eq!(id.0, 0);
    let hand = VarInt::decode(&mut cursor).unwrap();
    assert_eq!(hand.0, 1);
    assert!(cursor.is_empty());

    let mut cursor = buf.as_slice();
    let decoded = UseEntityAction::decode(&mut cursor).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn enum_variant_field_attrs_interact_at() {
    let original = UseEntityAction::InteractAt {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        hand: 0,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = UseEntityAction::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

#[test]
fn enum_variant_field_attrs_unit() {
    let original = UseEntityAction::Attack;
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    // Should be just VarInt(1)
    assert_eq!(buf.len(), 1);
    let mut cursor = buf.as_slice();
    let decoded = UseEntityAction::decode(&mut cursor).unwrap();
    assert_eq!(decoded, original);
}

// -- Enum with tuple variants --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
enum SimpleEnum {
    A(u8),
    B(u16, u32),
}

#[test]
fn enum_tuple_roundtrip() {
    let original = SimpleEnum::B(100, 200);
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = SimpleEnum::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Unknown enum discriminant --

#[test]
fn enum_unknown_discriminant() {
    let mut buf = Vec::new();
    VarInt(99).encode(&mut buf).unwrap();
    let mut cursor = buf.as_slice();
    assert!(matches!(
        NextState::decode(&mut cursor),
        Err(Error::InvalidData(_))
    ));
}

// -- Nested derived structs --

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct Inner {
    value: i32,
}

#[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
struct Outer {
    name: String,
    inner: Inner,
}

#[test]
fn nested_struct_roundtrip() {
    let original = Outer {
        name: "test".into(),
        inner: Inner { value: 42 },
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();

    let mut cursor = buf.as_slice();
    let decoded = Outer::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}

// -- Mixed attributes --

#[derive(Debug, PartialEq)]
#[packet(id = 0x0E)]
struct ComplexPacket {
    #[field(varint)]
    entity_id: i32,
    #[field(optional)]
    custom_name: Option<String>,
    #[field(length = "varint")]
    scores: Vec<i32>,
    active: bool,
}

#[test]
fn complex_packet_roundtrip() {
    let original = ComplexPacket {
        entity_id: 42,
        custom_name: Some("Steve".into()),
        scores: vec![100, 200, 300],
        active: true,
    };
    let mut buf = Vec::with_capacity(original.encoded_size());
    original.encode(&mut buf).unwrap();
    assert_eq!(buf.len(), original.encoded_size());

    let mut cursor = buf.as_slice();
    let decoded = ComplexPacket::decode(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(decoded, original);
}
