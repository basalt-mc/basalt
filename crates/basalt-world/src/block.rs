//! Block state IDs for common Minecraft 1.21.4 blocks.
//!
//! These are the default state IDs from the vanilla data generator.
//! Each block can have multiple states (e.g., grass_block has snowy
//! and non-snowy variants), but we use the default (non-snowy) state.

/// Air — no block, fully transparent.
pub const AIR: u16 = 0;

/// Stone — basic underground block.
pub const STONE: u16 = 1;

/// Dirt — soil block without grass.
pub const DIRT: u16 = 10;

/// Grass block — dirt with grass on top (default: snowy=false).
pub const GRASS_BLOCK: u16 = 9;

/// Bedrock — indestructible bottom layer.
pub const BEDROCK: u16 = 85;

/// Water — still water (default state, level 0).
pub const WATER: u16 = 86;

/// Sand — beach and desert block.
pub const SAND: u16 = 118;

/// Gravel — underwater floor block.
pub const GRAVEL: u16 = 124;

/// Snow block — covers high-altitude terrain tops.
pub const SNOW_BLOCK: u16 = 5950;

/// Chest — default state (facing north, type single, no waterlog).
pub const CHEST: u16 = 3010;

/// Range of block states that are chest variants (facing × type × waterlogged).
const CHEST_MIN: u16 = 3009;
const CHEST_MAX: u16 = 3032;

/// Returns true if the block state is any chest variant.
pub fn is_chest(state: u16) -> bool {
    (CHEST_MIN..=CHEST_MAX).contains(&state)
}

/// Returns true if the chest state is a single chest (not part of a double).
pub fn is_single_chest(state: u16) -> bool {
    is_chest(state) && chest_type(state) == 0
}

/// Extracts the facing index from a chest state (0=north, 1=south, 2=west, 3=east).
pub fn chest_facing(state: u16) -> u16 {
    if !is_chest(state) {
        return 0;
    }
    (state - CHEST_MIN) / 6
}

/// Extracts the type from a chest state (0=single, 1=left, 2=right).
pub fn chest_type(state: u16) -> u16 {
    if !is_chest(state) {
        return 0;
    }
    ((state - CHEST_MIN) % 6) / 2
}

/// Builds a chest state from facing, type, and waterlogged=false.
pub fn chest_state(facing: u16, chest_type: u16) -> u16 {
    CHEST_MIN + facing * 6 + chest_type * 2 + 1 // +1 for waterlogged=false
}

/// Returns the adjacent position for double chest pairing based on facing.
///
/// For north/south facing: checks east/west (±X).
/// For east/west facing: checks north/south (±Z).
/// Returns `(dx1, dz1, dx2, dz2)` — two candidate offsets to check.
pub fn chest_adjacent_offsets(facing: u16) -> [(i32, i32); 2] {
    match facing {
        0 | 1 => [(-1, 0), (1, 0)], // north/south: check west/east
        2 | 3 => [(0, -1), (0, 1)], // west/east: check north/south
        _ => [(0, 0), (0, 0)],
    }
}

/// Given a facing and the offset direction of the adjacent chest,
/// returns (type_for_new, type_for_existing).
///
/// The "left" chest is the one on the left when looking at the front.
pub fn chest_double_types(facing: u16, dx: i32, dz: i32) -> (u16, u16) {
    // (new_type, existing_type)
    match facing {
        0 => {
            // North-facing: front faces north, viewed from south
            if dx < 0 { (2, 1) } else { (1, 2) } // west neighbor: new=right, existing=left
        }
        1 => {
            // South-facing: front faces south, viewed from north
            if dx < 0 { (1, 2) } else { (2, 1) }
        }
        2 => {
            // West-facing: front faces west, viewed from east
            if dz < 0 { (1, 2) } else { (2, 1) }
        }
        3 => {
            // East-facing: front faces east, viewed from west
            if dz < 0 { (2, 1) } else { (1, 2) }
        }
        _ => (0, 0),
    }
}

/// Returns the chest block state for a given player yaw (facing the player).
///
/// The chest faces the player: if the player is looking north (yaw ~180),
/// the chest faces south so the player sees the front.
pub fn chest_state_for_yaw(yaw: f32) -> u16 {
    // Normalize yaw to 0-360
    let yaw = ((yaw % 360.0) + 360.0) % 360.0;
    // Chest states: base 3009, facing*6 + type*2 + waterlogged
    // type=single(0), waterlogged=false(1)
    // facing: 0=north, 1=south, 2=west, 3=east
    let facing = match yaw as u16 {
        0..=45 | 316..=360 => 0, // player faces south → chest faces north
        46..=135 => 3,           // player faces west → chest faces east
        136..=225 => 1,          // player faces north → chest faces south
        226..=315 => 2,          // player faces east → chest faces west
        _ => 0,
    };
    3009 + facing * 6 + 1 // +1 for waterlogged=false
}

/// Maximum item ID in the lookup table.
const MAX_ITEM_ID: usize = 1384;

/// Lookup table mapping item registry IDs to default block state IDs.
/// Generated from minecraft-data 1.21.4. Entries with value `u16::MAX`
/// indicate items that do not have a corresponding block.
#[rustfmt::skip]
const ITEM_TO_BLOCK_STATE: [u16; 1385] = [
        0,    1,    2,    3,    4,    5,    6,    7,25918,25920,26331,23329,22094,22098,22112,22184,
    22916,22505,22509,22523,22595,22917,22921,22935,23007,23328,25781,    9,   10,   11,   13,25915,
    25916,19621,19604,   14,   15,   16,   17,   18,   19,   20,   21,   25,   26,   27,19679,19680,
       28,   29,   31,   33,   35,   37,   39,   41,   43,   50,   85,  118,  119,  125,  123,  124,
      133,  134,  131,  132,23955,23956,  129,  130, 5904, 5906, 8285, 8286,  563,  564, 4329, 4330,
      135,10023,20461,11624,27571,27572,27573,27696,22044,22045, 2135,23951, 2134, 4331,20460,23952,
    23953,23954,23964,23963,23962,23961,23960,23959,23958,23957,24220,24140,24060,23980,24310,24304,
    24298,24292,24313,24315,24314,24316,23968,23967,23966,23965,24320,24319,24318,24317,24572,24492,
    24412,24332,24662,24656,24650,24644,  137,  140,  143,  146,  149,  152,  158,  155,  161,  164,
      166,19610,19593,  169,  193,  172,  175,  178,  181,  184,  187,  190,  196,19613,19596,  226,
      229,  232,  235,  238,  241,  244,  247,  250,19619,19602,  199,  202,  205,  208,  211,  214,
      217,   23,  220,  223,19616,19599,  279,  307,  335,  363,  391,  419,  447,  475,  503,  531,
      559,  560,  561,  562,23330,  565,  578,  579,  580, 2047, 2048, 2049,25837,25838, 2050, 2051,
    13946, 2090, 2091, 2092, 2093, 2094, 2095, 2096, 2097, 2098, 2099, 2100, 2101, 2102, 2103, 2104,
     2105, 2118,27862,27863, 2120, 2121, 2122, 2123, 2124, 2125, 2126, 2127, 2128, 2129, 2131, 2130,
     2119,13521,25836, 2132, 2133,19622,19605,19678,19607,19608,19624,19651, 5968,13773,25840,25839,
    25856,27698,27860,27697,25914,25858,25900,13958,12044,12050,12056,12062,12068,12074,12080,12086,
    12092,12098,12104,19684,19690,12110,12116,12122,12128,12134,12140,12146,12152,12158,12164,12170,
    12176,12182,12188,11588,11594,11600,12193,12194,12192,12191, 2136, 2139, 2203,27596, 2396, 2397,
     2398,13351,13416,13417,13423,13425,13438, 2916, 2926, 3010, 4332, 4341, 4350, 4742, 4780, 5941,
     5949, 5950, 5951, 5967, 5985, 6017,12514,12546,12578,12610,12642,12674,12706,12738,12770,19728,
    19760, 7044, 6035, 6039, 6018, 6019, 6020, 6022, 6025,27570, 6027, 6032, 6776, 6777, 6778, 6779,
     6780, 6781,27568, 6770, 6771, 6772, 6773, 6774, 6775,27153,27565,26742,27566,27564,27586, 6782,
     6846, 6910, 7005, 7009, 7043, 7045, 7101, 7229, 7357, 7633, 7634, 7646, 7718, 7724, 8045, 7401,
     7481, 7561, 7631, 7632, 8046,21736,21735, 8078, 8090,23812,23940,23942,23950, 8163, 8185, 8189,
    13507, 8190, 8216, 8288, 8439, 2940, 8451, 8531, 8611,10694,10774,10854,10934,11014,11094,11174,
    19964,20044, 8686, 8692, 8696, 9020,15176,15500,15824,16148,16472,16796,17120,17444,17768,18092,
    18416,18740,19064,20557,21414,20977,26010,26421,27243,26832, 9906, 9910, 9914,10035,10034,21737,
    10037,10050,10155,10156,10157,10158,10159,10160,10161,10162,10163,10164,10165,10166,10167,10168,
    10169,10170,11245,11277,11605,11607,11608,11609,11610,11611,11612,11613,11614,11615,11616,11617,
    11618,11619,11620,11621,11622,11623,11625,13526,11627,11629,11631,11633,11635,11637, 6114, 6115,
     6116, 6117, 6118, 6119, 6120, 6121, 6122, 6123, 6124, 6125, 6126, 6127, 6128, 6129,10202,10234,
    10266,10298,10330,10362,10394,10426,10458,10490,10522,10554,10586,10618,10650,10682,11342,11343,
    11344,11356,11436,11516,11603,11958,11959,11960,11972,13534,13546,13556,13557,19606,13558,13560,
    13562,13579,13585,13591,13597,13603,13609,13615,13621,13627,13633,13639,13645,13651,13657,13663,
    13669,13675,13677,13681,13685,13689,13693,13697,13701,13705,13709,13713,13717,13721,13725,13729,
    13733,13737,13741,13742,13743,13744,13745,13746,13747,13748,13749,13750,13751,13752,13753,13754,
    13755,13756,13757,13758,13759,13760,13761,13762,13763,13764,13765,13766,13767,13768,13769,13770,
    13771,13772,13801,13813,13816,13817,13818,13819,13820,13821,13822,13823,13824,13825,13836,13838,
    13840,13842,13844,13828,13830,13832,13834,13826,13856,13858,13860,13862,13864,13846,13848,13850,
    13852,13854,13954,13955,13986,14066,14146,14226,14306,14386,14466,14546,14626,14706,14786,14866,
    14946,15026,25932,26343,27165,26754,15098,15104,15110,15116,15122,15128,15134,15140,15146,15152,
    15158,15164,15170,26004,26415,27237,26826,19416,65535, 5907,10022, 6053, 9975, 2060, 2041,11243,
    20458,13568,10024,  567,10144,19466,20394, 5802,25756,10006,23333,23429, 8304, 9919, 2138, 8192,
      582, 5926,21396, 9395, 9419, 9443, 9467, 9491, 9515, 9539, 9563, 9587, 9611,20122,20146, 5818,
    21386, 9942, 9958, 5884, 5886, 5888, 5890, 5892, 5894, 5896, 5898, 5900, 5902,19694,19696, 5830,
     4688,12782,12846,12910,12974,13038,13102,13166,13230,13294,20172,20236,24676,24740,24868,24804,
    24932,24996,25124,25060,11293, 6145, 6209, 6273, 6337, 6401, 6465, 6529, 6593, 6657, 6721,19776,
    19840,25192,25256,25384,25320,25448,25512,25640,25576, 7365,12202,12234,12266,12298,12330,12362,
    12394,12426,12458,19896,19928, 2000, 2024, 4750,10132,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,20370,20383,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535, 4333,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535, 4358, 4390, 4422, 4518, 4454, 4486, 4550, 4582, 4614, 4646,20290,
    20322, 4962, 5026, 5090, 5282, 5154, 5218, 5346, 5410, 5602, 5666, 5474, 5538,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,13800,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535, 6043, 1734, 1750, 1766, 1782, 1798, 1814, 1830, 1846, 1862, 1878, 1894,
     1910, 1926, 1942, 1958, 1974,65535,27648,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535, 8159,65535,65535,65535,65535,65535,65535, 8171, 8172,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535, 9341,65535,65535,65535,65535,
    65535,65535, 9642, 9682, 9762, 9722, 9802, 9842, 9882,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,11638,11654,11670,11686,11702,11718,11734,11750,11766,11782,11798,11814,11830,11846,11862,
    11878,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,19417,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,20385,19422,19434,19442,19449,19450,19455,
    19479,19480,19485,19519,19523,65535,65535,19527,19559,19623,65535,20410,20434,65535,20459,20472,
    20462,20473,20881,20485,21298,20884,21382,21310,20887,20885,20891,20905,20886,20463,21741,21757,
    21773,21789,21805,21821,21837,21853,21869,21885,21901,21917,21933,21949,21965,21981,21997,22091,
    22079,22067,22055,25766,27577,27580,27583,27585,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,65535,
    65535,65535,65535,65535,25690,25692,25694,25696,25698,25700,25702,25704,25708,25712,25716,25720,
    25724,25728,25732,25736,27657,65535,65535,27667,65535,
];

/// Returns whether a block state ID represents a solid (collidable) block.
///
/// Solid blocks prevent entity movement through them. Air, water, and
/// a few other blocks are non-solid. This is a simplified check — a
/// full implementation would use the block registry's collision shape.
pub fn is_solid(state: u16) -> bool {
    // Non-solid blocks: air, water, lava, and their variants
    // This is a conservative approximation — in vanilla, collision
    // shapes are per-block-state, but for basic physics this suffices.
    !matches!(state, AIR | WATER)
}

/// Maps a Minecraft item registry ID to the default block state ID.
///
/// Returns `None` if the item does not have a corresponding block
/// (e.g., swords, food, spawn eggs). The mapping covers all 943
/// vanilla 1.21.4 blocks with matching items.
pub fn item_to_default_block_state(item_id: i32) -> Option<u16> {
    if item_id < 0 || item_id as usize > MAX_ITEM_ID {
        return None;
    }
    let state = ITEM_TO_BLOCK_STATE[item_id as usize];
    if state == u16::MAX { None } else { Some(state) }
}

/// Returns the item ID that a block state drops when broken.
///
/// Performs a reverse lookup of the [`item_to_default_block_state`]
/// table. Returns `None` for block states with no matching item
/// (e.g., air, technical blocks) or non-default block state variants.
pub fn block_state_to_item_id(state: u16) -> Option<i32> {
    if state == 0 {
        return None; // air drops nothing
    }
    // Blocks with multiple state variants: map any variant to the item
    if is_chest(state) {
        return Some(313); // chest item ID
    }
    for (item_id, &block_state) in ITEM_TO_BLOCK_STATE.iter().enumerate() {
        if block_state == state {
            return Some(item_id as i32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_item_maps_to_stone_block() {
        assert_eq!(item_to_default_block_state(1), Some(STONE));
    }

    #[test]
    fn dirt_item_maps_to_dirt_block() {
        assert_eq!(item_to_default_block_state(28), Some(DIRT));
    }

    #[test]
    fn air_item_maps_to_air_block() {
        assert_eq!(item_to_default_block_state(0), Some(AIR));
    }

    #[test]
    fn negative_item_id_returns_none() {
        assert_eq!(item_to_default_block_state(-1), None);
    }

    #[test]
    fn out_of_range_item_id_returns_none() {
        assert_eq!(item_to_default_block_state(99999), None);
    }

    #[test]
    fn non_block_item_returns_none() {
        // Item IDs in the 65535 sentinel range (e.g., swords, food)
        // Check a known non-block item
        assert_eq!(item_to_default_block_state(MAX_ITEM_ID as i32), None);
    }

    #[test]
    fn bedrock_mapping() {
        assert_eq!(item_to_default_block_state(58), Some(BEDROCK));
    }

    #[test]
    fn stone_block_drops_stone_item() {
        assert_eq!(block_state_to_item_id(STONE), Some(1));
    }

    #[test]
    fn air_drops_nothing() {
        assert_eq!(block_state_to_item_id(AIR), None);
    }

    #[test]
    fn all_terrain_blocks_have_reverse_mapping() {
        // Every block the terrain generators use must produce a drop
        assert!(block_state_to_item_id(STONE).is_some(), "stone");
        assert!(block_state_to_item_id(DIRT).is_some(), "dirt");
        assert!(block_state_to_item_id(GRASS_BLOCK).is_some(), "grass_block");
        assert!(block_state_to_item_id(SAND).is_some(), "sand");
        assert!(block_state_to_item_id(GRAVEL).is_some(), "gravel");
        assert!(block_state_to_item_id(BEDROCK).is_some(), "bedrock");
        assert!(block_state_to_item_id(SNOW_BLOCK).is_some(), "snow_block");
    }

    #[test]
    fn roundtrip_item_to_block_to_item() {
        // Stone: item 1 -> block state 1 -> item 1
        let state = item_to_default_block_state(1).unwrap();
        assert_eq!(block_state_to_item_id(state), Some(1));
    }
}
