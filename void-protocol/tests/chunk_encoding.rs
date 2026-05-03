use void_protocol::clientbound::{
    Chunk, ChunkHeightmaps, ChunkSection, LightData, PaletteData, biomes, blocks,
};

/// Encode a VarInt the same way the protocol does, for manual verification.
fn encode_varint(value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut v = value as u32;
    loop {
        if (v & !0x7F) == 0 {
            out.push(v as u8);
            return out;
        }
        out.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

#[test]
fn test_single_value_section_encoding() {
    // ----------------------------------------------------------------
    // Create a ChunkSection with:
    //   block_state = SingleValue(8)  -- grass_block (snowy=false)
    //   biome       = SingleValue(0)  -- plains
    //   block_count = 4096            -- fully filled section
    // ----------------------------------------------------------------
    let section = ChunkSection {
        block_count: 4096,
        block_state: PaletteData::SingleValue(blocks::GRASS_BLOCK), // 8
        biome: PaletteData::SingleValue(biomes::PLAINS),            // 0
    };

    let bytes = section.encode_to_bytes();

    // Print full hex dump
    println!("=== Single-value ChunkSection encoded bytes ===");
    println!("Total length: {} bytes", bytes.len());
    print!("Hex: ");
    for (i, b) in bytes.iter().enumerate() {
        print!("{:02x}", b);
        if i < bytes.len() - 1 {
            print!(" ");
        }
    }
    println!();
    println!();

    // ----------------------------------------------------------------
    // Manual verification byte-by-byte
    //
    // Expected layout (Minecraft protocol):
    //
    // [Block Count]       i16 big-endian: 4096 = 0x1000
    // [Block States]
    //   bits_per_entry:   u8  = 0   (single value palette)
    //   palette_value:    VarInt(9)  -> 0x08
    //   data_array_len:   VarInt(0)  -> 0x00
    // [Biomes]
    //   bits_per_entry:   u8  = 0   (single value palette)
    //   palette_value:    VarInt(0)  -> 0x00
    //   data_array_len:   VarInt(0)  -> 0x00
    // ----------------------------------------------------------------

    let mut expected = Vec::new();

    // 1) block_count: i16 big-endian = 4096 = 0x10, 0x00
    expected.extend_from_slice(&4096_i16.to_be_bytes());
    // 1b) fluid_count: i16 = 0 (1.21.5+).
    expected.extend_from_slice(&0_i16.to_be_bytes());

    // 2) Block state palette (single value) — 1.21.5+: no data array.
    expected.push(0x00); // bits_per_entry = 0
    expected.extend(encode_varint(9)); // palette value = grass_block = 8

    // 3) Biome palette (single value) — 1.21.5+: no data array.
    expected.push(0x00); // bits_per_entry = 0
    expected.extend(encode_varint(0)); // palette value = plains = 0

    println!("=== Expected bytes ===");
    print!("Hex: ");
    for (i, b) in expected.iter().enumerate() {
        print!("{:02x}", b);
        if i < expected.len() - 1 {
            print!(" ");
        }
    }
    println!();
    println!();

    // Walk through every byte and explain it
    println!("=== Byte-by-byte breakdown ===");
    let mut offset = 0;

    // Block count (2 bytes, big-endian i16)
    let bc = i16::from_be_bytes([bytes[0], bytes[1]]);
    println!(
        "  [{:02}..{:02}] block_count = {} (0x{:02x} 0x{:02x})",
        offset,
        offset + 1,
        bc,
        bytes[0],
        bytes[1]
    );
    assert_eq!(bc, 4096, "block_count should be 4096");
    offset += 2;

    // Fluid count (1.21.5+): 2 bytes, always 0 for us.
    let fc = i16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    assert_eq!(fc, 0, "fluid_count should be 0");
    offset += 2;

    // Block state palette: bits_per_entry
    println!(
        "  [{:02}]      block_state bits_per_entry = {} (0x{:02x})",
        offset, bytes[offset], bytes[offset]
    );
    assert_eq!(bytes[offset], 0, "block_state bits_per_entry should be 0");
    offset += 1;

    // Block state palette: VarInt palette value
    // VarInt(9) = single byte 0x08
    let varint_8 = encode_varint(9);
    let actual_palette_bytes = &bytes[offset..offset + varint_8.len()];
    println!(
        "  [{:02}]      block_state palette_value = VarInt(9) -> {:02x?}",
        offset, actual_palette_bytes
    );
    assert_eq!(
        actual_palette_bytes,
        &varint_8[..],
        "block_state palette value should encode VarInt(9)"
    );
    offset += varint_8.len();

    // No block_state data array on SingleValue (1.21.5+: ZeroBitStorage → 0 longs).
    let varint_0 = encode_varint(0);

    // Biome palette: bits_per_entry
    println!(
        "  [{:02}]      biome bits_per_entry = {} (0x{:02x})",
        offset, bytes[offset], bytes[offset]
    );
    assert_eq!(bytes[offset], 0, "biome bits_per_entry should be 0");
    offset += 1;

    // Biome palette: VarInt palette value = 0
    let actual_biome_bytes = &bytes[offset..offset + varint_0.len()];
    println!(
        "  [{:02}]      biome palette_value = VarInt(0) -> {:02x?}",
        offset, actual_biome_bytes
    );
    assert_eq!(
        actual_biome_bytes,
        &varint_0[..],
        "biome palette value should encode VarInt(0)"
    );
    offset += varint_0.len();

    // No biome data array on SingleValue (1.21.5+).
    let _ = varint_0;

    println!();
    println!(
        "Total consumed: {} bytes (section length: {})",
        offset,
        bytes.len()
    );
    assert_eq!(offset, bytes.len(), "all bytes should be accounted for");
    assert_eq!(
        bytes, expected,
        "full byte sequence must match expected encoding"
    );

    println!();
    println!("[PASS] Single-value ChunkSection encoding is correct.");
}

#[test]
fn test_empty_section_encoding() {
    // An empty section: block_state=AIR(0), biome=PLAINS(0), block_count=0
    let section = ChunkSection::empty();
    let bytes = section.encode_to_bytes();

    println!("=== Empty ChunkSection encoded bytes ===");
    print!("Hex: ");
    for (i, b) in bytes.iter().enumerate() {
        print!("{:02x}", b);
        if i < bytes.len() - 1 {
            print!(" ");
        }
    }
    println!();

    let mut expected = Vec::new();
    // block_count = 0
    expected.extend_from_slice(&0_i16.to_be_bytes());
    // fluid_count = 0 (1.21.5+).
    expected.extend_from_slice(&0_i16.to_be_bytes());
    // block_state: bits_per_entry=0, VarInt(0)  (no data array on SingleValue)
    expected.push(0x00);
    expected.extend(encode_varint(0));
    // biome: bits_per_entry=0, VarInt(0)  (no data array on SingleValue)
    expected.push(0x00);
    expected.extend(encode_varint(0));

    assert_eq!(bytes, expected, "empty section encoding must match");
    println!("[PASS] Empty section encoding is correct.");
}

#[test]
fn test_full_chunk_24_sections_encoding() {
    // Build a Chunk with 24 sections, varying block states:
    //   - Sections 0..3  -> filled with STONE (id=1)
    //   - Section  4     -> filled with DIRT (id=10)
    //   - Section  5     -> filled with GRASS_BLOCK (id=8)
    //   - Sections 6..23 -> empty (AIR)
    // All biomes = PLAINS (0)

    let mut sections = Vec::with_capacity(24);

    for i in 0..24 {
        let section = match i {
            0..=3 => ChunkSection::filled(blocks::STONE, biomes::PLAINS),
            4 => ChunkSection::filled(blocks::DIRT, biomes::PLAINS),
            5 => ChunkSection::filled(blocks::GRASS_BLOCK, biomes::PLAINS),
            _ => ChunkSection::empty(),
        };
        sections.push(section);
    }

    let chunk = Chunk {
        x: 0,
        z: 0,
        heightmaps: ChunkHeightmaps::empty(),
        sections,
        light: LightData::empty(),
    };

    // Encode the chunk data (section bytes only, same as to_packet().data)
    let mut chunk_data = Vec::new();
    for section in &chunk.sections {
        chunk_data.extend(section.encode_to_bytes());
    }

    println!("=== Full Chunk (24 sections) encoding ===");
    println!("Total chunk data byte count: {}", chunk_data.len());

    // Each single-value section should be exactly 8 bytes:
    //   2 (block_count) + 1 (bpe=0) + 1 (VarInt palette) + 1 (VarInt datalen=0)
    //                   + 1 (bpe=0) + 1 (VarInt palette) + 1 (VarInt datalen=0)
    // = 8 bytes ... except when the palette value needs more than 1 VarInt byte.
    //
    // VarInt sizes for our palette values:
    //   0  -> 1 byte (0x00)
    //   1  -> 1 byte (0x01)
    //   8  -> 1 byte (0x08)
    //   10 -> 1 byte (0x0a)
    //
    // So every section here is exactly 8 bytes.

    // 1.21.5+ SingleValue: 2 (block_count) + 2 (fluid_count) + 1+1 (block) + 1+1 (biome) = 8.
    let single_section_size = 8;
    let expected_total = single_section_size * 24;
    println!(
        "Expected total: {} bytes ({} sections * {} bytes/section)",
        expected_total, 24, single_section_size
    );

    assert_eq!(
        chunk_data.len(),
        expected_total,
        "24 single-value sections should produce {} bytes total",
        expected_total
    );

    // Verify section boundaries for the first 6 sections
    println!();
    println!("=== Per-section verification ===");

    let expected_block_states = [
        (4096_i16, blocks::STONE),       // section 0
        (4096_i16, blocks::STONE),       // section 1
        (4096_i16, blocks::STONE),       // section 2
        (4096_i16, blocks::STONE),       // section 3
        (4096_i16, blocks::DIRT),        // section 4
        (4096_i16, blocks::GRASS_BLOCK), // section 5
    ];

    for (idx, &(exp_count, exp_block)) in expected_block_states.iter().enumerate() {
        let base = idx * single_section_size;
        let section_bytes = &chunk_data[base..base + single_section_size];

        let block_count = i16::from_be_bytes([section_bytes[0], section_bytes[1]]);
        let fluid_count = i16::from_be_bytes([section_bytes[2], section_bytes[3]]);
        assert_eq!(fluid_count, 0, "section {} fluid_count", idx);
        let bpe_block = section_bytes[4];
        let palette_block = section_bytes[5]; // VarInt, single byte for values < 128
        let bpe_biome = section_bytes[6];
        let palette_biome = section_bytes[7];

        println!(
            "  Section {:2}: block_count={:5}, block_state(bpe={}, palette={}), biome(bpe={}, palette={})",
            idx, block_count, bpe_block, palette_block, bpe_biome, palette_biome
        );

        assert_eq!(block_count, exp_count, "section {} block_count", idx);
        assert_eq!(bpe_block, 0, "section {} block bpe", idx);
        assert_eq!(
            palette_block as i32, exp_block,
            "section {} block palette value",
            idx
        );
        assert_eq!(bpe_biome, 0, "section {} biome bpe", idx);
        assert_eq!(
            palette_biome, 0,
            "section {} biome palette value (plains=0)",
            idx
        );
    }

    // Verify empty sections (6..23)
    for idx in 6..24 {
        let base = idx * single_section_size;
        let section_bytes = &chunk_data[base..base + single_section_size];
        let block_count = i16::from_be_bytes([section_bytes[0], section_bytes[1]]);
        assert_eq!(
            block_count, 0,
            "section {} should be empty (block_count=0)",
            idx
        );
        assert_eq!(section_bytes[2], 0, "section {} block bpe should be 0", idx);
        assert_eq!(
            section_bytes[3], 0,
            "section {} block palette should be AIR(0)",
            idx
        );
    }

    // Also verify that to_packet() produces the same data bytes
    let packet = chunk.to_packet();
    assert_eq!(
        packet.data.len(),
        chunk_data.len(),
        "to_packet().data length must match manual section encoding"
    );
    assert_eq!(
        packet.data, chunk_data,
        "to_packet().data must match manual section encoding"
    );

    println!();
    println!(
        "[PASS] Full chunk (24 sections) encoding verified: {} bytes total.",
        chunk_data.len()
    );
}
