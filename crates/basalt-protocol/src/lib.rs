#[cfg(test)]
mod derive_tests {
    use basalt_derive::{Decode, Encode, EncodedSize};
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

    #[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
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
    fn packet_with_id_roundtrip() {
        let original = HandshakePacket {
            protocol_version: 763,
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();

        // First byte should be VarInt(0x00) = 0x00
        assert_eq!(buf[0], 0x00);

        let mut cursor = buf.as_slice();
        let decoded = HandshakePacket::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn packet_wrong_id_fails() {
        let packet = HandshakePacket {
            protocol_version: 763,
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        // Corrupt the packet ID
        buf[0] = 0x01;
        let mut cursor = buf.as_slice();
        assert!(matches!(
            HandshakePacket::decode(&mut cursor),
            Err(Error::InvalidData(_))
        ));
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
        // VarInt(1) = 1 byte, VarInt(300000) = 3 bytes
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

            // Verify the discriminant
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

    #[derive(Debug, PartialEq, Encode, Decode, EncodedSize)]
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
}
