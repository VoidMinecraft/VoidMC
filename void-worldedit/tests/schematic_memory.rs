use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::GzEncoder;
use voidmc_worldedit::{BlockPos, Schematic};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grew(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grew(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(malloc_size(layout.size()), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            grew(new_size);
            LIVE.fetch_sub(malloc_size(layout.size()), Ordering::Relaxed);
        }
        new
    }
}

fn grew(size: usize) {
    let size = malloc_size(size);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn malloc_size(size: usize) -> usize {
    size.max(1).next_multiple_of(16)
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const LIMIT: u64 = 8 * 1024 * 1024;
const MAX_VOLUME: u64 = 64 * 1024 * 1024;

struct Measured {
    loaded: Option<Schematic>,
    peak: usize,
    elapsed: Duration,
}

fn measure(input: &[u8], limit: u64) -> Measured {
    let start = Instant::now();
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let loaded = Schematic::read_with_limits(input, MAX_VOLUME, limit);
    let peak = PEAK.load(Ordering::Relaxed) - base;
    if let Err(error) = &loaded {
        assert!(!error.to_string().is_empty());
    }
    Measured {
        loaded: loaded.ok(),
        peak,
        elapsed: start.elapsed(),
    }
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

fn tag(out: &mut Vec<u8>, id: u8, name: &str) {
    out.push(id);
    out.extend((name.len() as u16).to_be_bytes());
    out.extend(name.as_bytes());
}

fn list(out: &mut Vec<u8>, name: &str, element: u8, len: usize) {
    tag(out, 9, name);
    out.push(element);
    out.extend((len as i32).to_be_bytes());
}

fn root(body: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::new();
    tag(&mut out, 10, "");
    body(&mut out);
    out.push(0);
    out
}

fn schematic(size: (i16, i16, i16), extra: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    root(|out| {
        tag(out, 10, "Schematic");
        tag(out, 3, "Version");
        out.extend(3i32.to_be_bytes());
        for (name, side) in [("Width", size.0), ("Height", size.1), ("Length", size.2)] {
            tag(out, 2, name);
            out.extend(side.to_be_bytes());
        }
        extra(out);
        out.push(0);
    })
}

fn blocks(out: &mut Vec<u8>, palette: &[&str], data: &[u8]) {
    tag(out, 10, "Blocks");
    tag(out, 10, "Palette");
    for (entry, name) in palette.iter().enumerate() {
        tag(out, 3, name);
        out.extend((entry as i32).to_be_bytes());
    }
    out.push(0);
    tag(out, 7, "Data");
    out.extend((data.len() as i32).to_be_bytes());
    out.extend(data);
    out.push(0);
}

fn within_limit(name: &str, measured: &Measured, limit: u64) {
    assert!(
        measured.peak as u64 <= limit,
        "{name}: allocated {} bytes for a {limit}-byte limit",
        measured.peak
    );
    assert!(
        measured.elapsed < Duration::from_secs(10),
        "{name}: took {:?}",
        measured.elapsed
    );
}

fn crafted(limit: u64) -> Vec<(&'static str, Vec<u8>)> {
    let fill = |per_element: usize| limit as usize / 100 * 95 / per_element;
    let compounds = root(|out| {
        let count = fill(200);
        list(out, "", 10, count);
        for _ in 0..count {
            out.extend([1, 0, 0, 0, 0]);
        }
    });
    let strings = root(|out| {
        let count = fill(25);
        list(out, "", 8, count);
        for _ in 0..count {
            out.extend([0, 1, b'a']);
        }
    });
    let mut trailing = root(|out| {
        tag(out, 7, "");
        let len = fill(2);
        out.extend((len as i32).to_be_bytes());
        out.resize(out.len() + len, 0);
    });
    trailing.resize(fill(1), 0);
    let offset = schematic((1, 1, 1), |out| {
        blocks(out, &["minecraft:stone"], &[0]);
        tag(out, 11, "Offset");
        let count = fill(4);
        out.extend((count as i32).to_be_bytes());
        out.resize(out.len() + count * 4, 7);
    });
    let nested_strings = root(|out| {
        let outer = fill(550);
        list(out, "", 9, outer);
        for _ in 0..outer {
            out.push(8);
            out.extend(20i32.to_be_bytes());
            for _ in 0..20 {
                out.extend([0, 1, b'b']);
            }
        }
    });
    let int_arrays = root(|out| {
        let count = fill(28);
        list(out, "", 11, count);
        for _ in 0..count {
            out.extend(1i32.to_be_bytes());
            out.extend(9i32.to_be_bytes());
        }
    });
    let deep = root(|out| {
        for _ in 0..500 {
            tag(out, 10, "");
        }
        out.resize(out.len() + 500, 0);
    });
    let named = root(|out| {
        for index in 0..fill(150) {
            tag(out, 1, &format!("{index:036}"));
            out.push(0);
        }
    });
    vec![
        ("list of one-entry compounds", compounds),
        ("list of one-byte strings", strings),
        ("tree followed by trailing data", trailing),
        ("huge Offset int array", offset),
        ("nested lists of small strings", nested_strings),
        ("many small int arrays", int_arrays),
        ("deep compounds", deep),
        ("many long-named entries", named),
    ]
}

#[test]
fn crafted_files_stay_within_the_memory_limit() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (name, raw) in crafted(LIMIT) {
        within_limit(name, &measure(&raw, LIMIT), LIMIT);
        within_limit(name, &measure(&gzip(&raw), LIMIT), LIMIT);
    }
}

#[test]
fn trailing_data_after_the_root_is_rejected() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut valid = schematic((1, 1, 1), |out| blocks(out, &["minecraft:stone"], &[0]));
    assert!(measure(&valid, LIMIT).loaded.is_some());
    valid.push(0);
    assert!(measure(&valid, LIMIT).loaded.is_none());
    assert!(measure(&gzip(&valid), LIMIT).loaded.is_none());
}

#[test]
fn an_oversized_offset_is_ignored_without_copying_it() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let raw = schematic((1, 1, 1), |out| {
        blocks(out, &["minecraft:stone"], &[0]);
        tag(out, 11, "Offset");
        let count = (LIMIT / 8) as usize;
        out.extend((count as i32).to_be_bytes());
        out.resize(out.len() + count * 4, 1);
    });
    let measured = measure(&raw, LIMIT);
    let loaded = measured.loaded.expect("a long Offset is ignored");
    assert_eq!(loaded.clipboard.offset(), BlockPos::ZERO);
    assert!(measured.peak < 256 * 1024, "peak {}", measured.peak);
}

#[test]
fn unknown_palette_names_count_against_the_limit() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let names: Vec<String> = (0..60_000)
        .map(|index| format!("custom:{index:0100}"))
        .collect();
    let palette: Vec<&str> = names.iter().map(String::as_str).collect();
    let raw = schematic((1, 1, 1), |out| blocks(out, &palette, &[0]));
    let small = 2 * 1024 * 1024;
    let measured = measure(&raw, small);
    assert!(measured.loaded.is_none());
    within_limit("unknown palette names", &measured, small);
    let measured = measure(&raw, LIMIT * 2);
    assert_eq!(measured.loaded.unwrap().unknown_blocks.len(), 60_000);
}

#[test]
fn a_full_schematic_with_biomes_and_block_entities_loads_within_the_limit() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height, length) = (128i16, 64i16, 128i16);
    let volume = width as usize * height as usize * length as usize;
    let names: Vec<String> = (0..200)
        .map(|index| {
            if index % 2 == 0 {
                "minecraft:stone".to_string()
            } else {
                format!("minecraft:oak_log[axis={}]", ["x", "y", "z"][index % 3])
            }
        })
        .collect();
    let palette: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut data = Vec::with_capacity(volume * 2);
    for index in 0..volume {
        let entry = (index % 200) as u32;
        if entry < 128 {
            data.push(entry as u8);
        } else {
            data.push((entry & 0x7f) as u8 | 0x80);
            data.push((entry >> 7) as u8);
        }
    }
    let raw = root(|out| {
        tag(out, 10, "Schematic");
        tag(out, 10, "Biomes");
        tag(out, 10, "Palette");
        tag(out, 3, "minecraft:plains");
        out.extend(0i32.to_be_bytes());
        out.push(0);
        tag(out, 7, "Data");
        out.extend((volume as i32).to_be_bytes());
        out.resize(out.len() + volume, 0);
        out.push(0);
        blocks(out, &palette, &data);
        list(out, "BlockEntities", 10, 30_000);
        for index in 0..30_000i32 {
            tag(out, 8, "Id");
            out.extend(14u16.to_be_bytes());
            out.extend(b"minecraft:sign");
            tag(out, 11, "Pos");
            out.extend(3i32.to_be_bytes());
            for value in [index % 128, index % 64, index / 128] {
                out.extend(value.to_be_bytes());
            }
            out.push(0);
        }
        tag(out, 3, "Version");
        out.extend(3i32.to_be_bytes());
        for (name, side) in [("Width", width), ("Height", height), ("Length", length)] {
            tag(out, 2, name);
            out.extend(side.to_be_bytes());
        }
        out.push(0);
    });
    for input in [gzip(&raw), raw] {
        let measured = measure(&input, LIMIT);
        within_limit("full schematic", &measured, LIMIT);
        let loaded = measured.loaded.expect("a legitimate schematic loads");
        assert_eq!(loaded.clipboard.volume(), volume);
        assert_eq!(loaded.clipboard.size(), BlockPos::new(128, 64, 128));
    }
}
