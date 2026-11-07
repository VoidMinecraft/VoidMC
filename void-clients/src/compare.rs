use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use serde::Serialize;

const DEFAULT_DELTA: u8 = 16;
const TILE: u32 = 32;
const DIMMED: u8 = 3;
const CHANGED: [u8; 3] = [255, 40, 40];
const IGNORED: [u8; 3] = [60, 90, 200];
const REQUIRED: [u8; 3] = [40, 220, 90];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn parse(text: &str) -> Result<Self, String> {
        let numbers: Vec<u32> = text
            .split(',')
            .map(|part| part.trim().parse())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("bad zone {text}: x,y,w,h"))?;
        match numbers[..] {
            [x, y, w, h] if w > 0 && h > 0 => Ok(Self { x, y, w, h }),
            _ => Err(format!(
                "bad zone {text}: x,y,w,h, with a width and a height"
            )),
        }
    }

    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    fn on_edge(&self, x: u32, y: u32) -> bool {
        self.contains(x, y)
            && (x == self.x || y == self.y || x + 1 == self.x + self.w || y + 1 == self.y + self.h)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Required {
    pub rect: Rect,
    pub max: u64,
    pub delta: u8,
}

impl Required {
    /// `x,y,w,h`, then at most a number of pixels and a level: `x,y,w,h:max:delta`.
    fn parse(text: &str, delta: u8) -> Result<Self, String> {
        let mut parts = text.split(':');
        let rect = Rect::parse(parts.next().unwrap_or_default())?;
        let max = parts
            .next()
            .map_or(Ok(0), str::parse)
            .map_err(|_| format!("bad pixel limit in {text}"))?;
        let delta = parts
            .next()
            .map_or(Ok(delta), str::parse)
            .map_err(|_| format!("bad level in {text}"))?;
        if parts.next().is_some() {
            return Err(format!("bad zone {text}: x,y,w,h:max:delta"));
        }
        Ok(Self { rect, max, delta })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    pub delta: u8,
    pub max_pixels: Option<u64>,
    pub max_tile: Option<u64>,
    pub ignore: Vec<Rect>,
    pub require: Vec<Required>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            delta: DEFAULT_DELTA,
            max_pixels: None,
            max_tile: None,
            ignore: Vec::new(),
            require: Vec::new(),
        }
    }
}

impl Rules {
    /// Takes `key=value` words, the same in a scenario, on the command line and in a zones file.
    /// A file counts as if written before the other words, so they override it, and every
    /// `require=` takes the `delta` that comes of it all.
    pub fn parse(words: &[String], base: &Path) -> Result<Self, String> {
        let mut all = Vec::new();
        for word in words {
            if let Some(file) = word.strip_prefix("zones=") {
                all.extend(zones_file(&base.join(file))?);
            }
        }
        all.extend(
            words
                .iter()
                .filter(|word| !word.starts_with("zones="))
                .cloned(),
        );
        let mut rules = Self::default();
        let mut delayed = Vec::new();
        for word in &all {
            let (key, value) = word
                .split_once('=')
                .ok_or_else(|| format!("bad option {word}: key=value"))?;
            match key {
                "delta" => rules.delta = value.parse().map_err(|_| format!("bad {word}"))?,
                "max" => rules.max_pixels = Some(value.parse().map_err(|_| format!("bad {word}"))?),
                "tile" => rules.max_tile = Some(value.parse().map_err(|_| format!("bad {word}"))?),
                "ignore" => rules.ignore.push(Rect::parse(value)?),
                "require" => delayed.push(value.to_string()),
                _ => {
                    return Err(format!(
                        "unknown option {key}: delta, max, tile, ignore, require, zones"
                    ));
                }
            }
        }
        for zone in delayed {
            rules.require.push(Required::parse(&zone, rules.delta)?);
        }
        Ok(rules)
    }

    pub fn has_a_limit(&self) -> bool {
        self.max_pixels.is_some() || self.max_tile.is_some() || !self.require.is_empty()
    }
}

fn zones_file(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let words: Vec<String> = text
        .lines()
        .flat_map(|line| {
            line.split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace()
        })
        .map(str::to_string)
        .collect();
    match words.iter().find(|word| word.starts_with("zones=")) {
        Some(word) => Err(format!(
            "{}: a zones file cannot hold {word}",
            path.display()
        )),
        None => Ok(words),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Picture {
    pub fn load(path: &Path) -> Result<Self, String> {
        let read = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let file = File::open(path).map_err(|e| read(&e))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| read(&e))?;
        let size = reader.output_buffer_size().ok_or("picture too large")?;
        let mut bytes = vec![0; size];
        let info = reader.next_frame(&mut bytes).map_err(|e| read(&e))?;
        let channels = info.color_type.samples();
        let mut rgb = Vec::with_capacity((info.width * info.height * 3) as usize);
        for pixel in bytes[..info.buffer_size()].chunks_exact(channels) {
            match channels {
                1 | 2 => rgb.extend([pixel[0]; 3]),
                _ => rgb.extend(&pixel[..3]),
            }
        }
        Ok(Self {
            width: info.width,
            height: info.height,
            rgb,
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&self.rgb)
            .map_err(|e| e.to_string())
    }

    fn at(&self, x: u32, y: u32) -> &[u8] {
        let start = ((y * self.width + x) * 3) as usize;
        &self.rgb[start..start + 3]
    }

    fn put(&mut self, x: u32, y: u32, colour: [u8; 3]) {
        let start = ((y * self.width + x) * 3) as usize;
        self.rgb[start..start + 3].copy_from_slice(&colour);
    }

    /// The mean of the three channels over the whole picture, from 0 to 255.
    pub fn brightness(&self) -> f64 {
        let sum: u64 = self.rgb.iter().map(|byte| u64::from(*byte)).sum();
        sum as f64 / self.rgb.len().max(1) as f64
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneResult {
    pub zone: Required,
    pub pixels: u64,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorstTile {
    pub x: u32,
    pub y: u32,
    pub size: u32,
    pub pixels: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub delta: u8,
    pub pixels: u64,
    pub total: u64,
    pub max_pixels: Option<u64>,
    pub worst_tile: WorstTile,
    pub max_tile: Option<u64>,
    pub zones: Vec<ZoneResult>,
    pub passed: bool,
    pub diff: Option<PathBuf>,
}

impl Outcome {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "{} of {} px differ by {} or more, worst {size}x{size} tile {} px at {},{}",
            self.pixels,
            self.total,
            self.delta,
            self.worst_tile.pixels,
            self.worst_tile.x,
            self.worst_tile.y,
            size = self.worst_tile.size,
        );
        for zone in &self.zones {
            let Rect { x, y, w, h } = zone.zone.rect;
            text.push_str(&format!(
                "\n  {} zone {x},{y},{w},{h}: {} px of {} allowed, level {}",
                if zone.passed { "ok  " } else { "FAIL" },
                zone.pixels,
                zone.zone.max,
                zone.zone.delta
            ));
        }
        text
    }
}

fn gap(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// Compares two pictures of one size. Ignored pixels count nowhere, not even in a required zone.
pub fn compare(a: &Picture, b: &Picture, rules: &Rules) -> Result<(Outcome, Picture), String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!(
            "the pictures differ in size: {}x{} and {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }
    for zone in rules
        .ignore
        .iter()
        .chain(rules.require.iter().map(|z| &z.rect))
    {
        if zone.x + zone.w > a.width || zone.y + zone.h > a.height {
            return Err(format!(
                "zone {},{},{},{} leaves the {}x{} picture",
                zone.x, zone.y, zone.w, zone.h, a.width, a.height
            ));
        }
    }
    let columns = a.width.div_ceil(TILE);
    let rows = a.height.div_ceil(TILE);
    let mut tiles = vec![0u64; (columns * rows) as usize];
    let mut zones = vec![0u64; rules.require.len()];
    let mut pixels = 0;
    let mut image = Picture {
        width: a.width,
        height: a.height,
        rgb: a.rgb.iter().map(|byte| byte / DIMMED).collect(),
    };
    for y in 0..a.height {
        for x in 0..a.width {
            if rules.ignore.iter().any(|zone| zone.contains(x, y)) {
                continue;
            }
            let difference = gap(a.at(x, y), b.at(x, y));
            for (count, zone) in zones.iter_mut().zip(&rules.require) {
                if zone.rect.contains(x, y) && difference >= zone.delta {
                    *count += 1;
                    image.put(x, y, CHANGED);
                }
            }
            if difference >= rules.delta {
                pixels += 1;
                tiles[((y / TILE) * columns + x / TILE) as usize] += 1;
                image.put(x, y, CHANGED);
            }
        }
    }
    for y in 0..a.height {
        for x in 0..a.width {
            if rules.ignore.iter().any(|zone| zone.on_edge(x, y)) {
                image.put(x, y, IGNORED);
            }
            if rules.require.iter().any(|zone| zone.rect.on_edge(x, y)) {
                image.put(x, y, REQUIRED);
            }
        }
    }
    let (worst, most) = tiles
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .map_or((0, 0), |(index, count)| (index as u32, *count));
    let zones: Vec<ZoneResult> = rules
        .require
        .iter()
        .zip(zones)
        .map(|(zone, count)| ZoneResult {
            zone: zone.clone(),
            pixels: count,
            passed: count <= zone.max,
        })
        .collect();
    let passed = rules.max_pixels.is_none_or(|max| pixels <= max)
        && rules.max_tile.is_none_or(|max| most <= max)
        && zones.iter().all(|zone| zone.passed);
    let outcome = Outcome {
        delta: rules.delta,
        pixels,
        total: u64::from(a.width) * u64::from(a.height),
        max_pixels: rules.max_pixels,
        worst_tile: WorstTile {
            x: worst % columns * TILE,
            y: worst / columns * TILE,
            size: TILE,
            pixels: most,
        },
        max_tile: rules.max_tile,
        zones,
        passed,
        diff: None,
    };
    Ok((outcome, image))
}

/// Compares two files and writes the picture of the difference.
pub fn files(a: &Path, b: &Path, rules: &Rules, diff: &Path) -> Result<Outcome, String> {
    if !rules.has_a_limit() {
        return Err("nothing to hold the pictures to: give max=, tile= or require=".into());
    }
    let (mut outcome, image) = compare(&Picture::load(a)?, &Picture::load(b)?, rules)?;
    if let Some(parent) = diff.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    image.save(diff)?;
    outcome.diff = Some(diff.to_path_buf());
    Ok(outcome)
}

/// `voidc compare a.png b.png [key=value ...] [--diff out.png]`.
pub fn command(args: &[String]) -> Result<(), String> {
    let [a, b, rest @ ..] = args else {
        return Err("voidc compare <a.png> <b.png> [max=N] [tile=N] [delta=N] [ignore=x,y,w,h] [require=x,y,w,h:max:delta] [zones=file] [--diff out.png]".into());
    };
    let (mut words, mut diff) = (Vec::new(), None);
    let mut rest = rest.iter();
    while let Some(word) = rest.next() {
        if word == "--diff" {
            diff = rest.next().map(PathBuf::from);
        } else {
            words.push(word.clone());
        }
    }
    let rules = Rules::parse(&words, Path::new("."))?;
    let diff = diff.unwrap_or_else(|| PathBuf::from(a).with_extension("diff.png"));
    let outcome = files(Path::new(a), Path::new(b), &rules, &diff)?;
    println!("{}", outcome.summary());
    println!("diff: {}", diff.display());
    if outcome.passed {
        Ok(())
    } else {
        Err("the pictures differ".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, level: u8) -> Picture {
        Picture {
            width,
            height,
            rgb: vec![level; (width * height * 3) as usize],
        }
    }

    fn rules(words: &[&str]) -> Rules {
        let words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        Rules::parse(&words, Path::new(".")).unwrap()
    }

    fn paint(picture: &mut Picture, rect: Rect, level: u8) {
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                picture.put(x, y, [level; 3]);
            }
        }
    }

    fn scatter(picture: &mut Picture, count: u32) {
        for n in 0..count {
            let (x, y) = (n * 37 % picture.width, n * 91 % picture.height);
            picture.put(x, y, [255; 3]);
        }
    }

    #[test]
    fn a_small_defect_in_much_scattered_noise_shows_in_a_tile_not_in_the_total() {
        let a = flat(640, 360, 40);
        let mut noisy = a.clone();
        scatter(&mut noisy, 780);
        let mut broken = noisy.clone();
        paint(
            &mut broken,
            Rect {
                x: 200,
                y: 100,
                w: 12,
                h: 12,
            },
            200,
        );
        let limit = rules(&["max=1000", "tile=60"]);
        assert!(compare(&a, &noisy, &limit).unwrap().0.passed);
        let total_only = rules(&["max=1000"]);
        assert!(compare(&a, &broken, &total_only).unwrap().0.passed);
        let (outcome, _) = compare(&a, &broken, &limit).unwrap();
        assert!(!outcome.passed);
        assert_eq!((outcome.worst_tile.x, outcome.worst_tile.y), (192, 96));
    }

    #[test]
    fn an_ignored_zone_counts_nowhere() {
        let a = flat(64, 64, 10);
        let mut b = a.clone();
        let zone = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        };
        paint(&mut b, zone, 200);
        let (outcome, _) = compare(
            &a,
            &b,
            &rules(&["max=0", "ignore=0,0,8,8", "require=0,0,16,16"]),
        )
        .unwrap();
        assert_eq!(outcome.pixels, 0);
        assert!(outcome.passed);
    }

    #[test]
    fn a_required_zone_has_its_own_limit_and_level() {
        let a = flat(64, 64, 100);
        let mut b = a.clone();
        paint(
            &mut b,
            Rect {
                x: 10,
                y: 10,
                w: 4,
                h: 4,
            },
            104,
        );
        let (outcome, _) = compare(&a, &b, &rules(&["max=0", "require=8,8,16,16:15:3"])).unwrap();
        assert_eq!(outcome.pixels, 0);
        assert_eq!(outcome.zones[0].pixels, 16);
        assert!(!outcome.zones[0].passed);
        assert!(!outcome.passed);
        let (outcome, _) = compare(&a, &b, &rules(&["require=8,8,16,16:16:3"])).unwrap();
        assert!(outcome.passed);
    }

    #[test]
    fn a_zone_outside_the_picture_is_an_error_not_a_pass() {
        let a = flat(32, 32, 0);
        assert!(compare(&a, &a, &rules(&["require=30,30,8,8"])).is_err());
    }

    #[test]
    fn pictures_of_two_sizes_do_not_compare() {
        assert!(compare(&flat(8, 8, 0), &flat(9, 8, 0), &rules(&["max=0"])).is_err());
    }

    #[test]
    fn a_comparison_without_a_limit_is_refused() {
        let dir = std::env::temp_dir().join(format!("voidc-compare-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.png");
        flat(4, 4, 9).save(&path).unwrap();
        assert!(files(&path, &path, &Rules::default(), &dir.join("d.png")).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_picture_survives_a_file_and_the_difference_is_written() {
        let dir = std::env::temp_dir().join(format!("voidc-diff-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (one, two) = (dir.join("one.png"), dir.join("two.png"));
        let mut picture = flat(40, 40, 30);
        picture.save(&one).unwrap();
        assert_eq!(Picture::load(&one).unwrap(), picture);
        paint(
            &mut picture,
            Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            },
            200,
        );
        picture.save(&two).unwrap();
        let diff = dir.join("nested/diff.png");
        let outcome = files(&one, &two, &rules(&["max=3"]), &diff).unwrap();
        assert!(!outcome.passed);
        assert_eq!(Picture::load(&diff).unwrap().at(0, 0), CHANGED);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_zones_file_gives_ignored_and_required_zones() {
        let dir = std::env::temp_dir().join(format!("voidc-zones-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("map.zones"),
            "# the birds\nignore=0,0,10,10\nrequire=20,20,4,4:2\n",
        )
        .unwrap();
        let parsed = Rules::parse(&["zones=map.zones".to_string()], &dir).unwrap();
        assert_eq!(parsed.ignore.len(), 1);
        assert_eq!(parsed.require[0].max, 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_zones_file_keeps_its_delta_and_its_zones_inherit_it() {
        let dir = std::env::temp_dir().join(format!("voidc-delta-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("map.zones"), "delta=4\nrequire=0,0,4,4:2\n").unwrap();
        let parsed = Rules::parse(&["zones=map.zones".to_string()], &dir).unwrap();
        assert_eq!(parsed.delta, 4);
        assert_eq!(parsed.require[0].delta, 4);
        let words = ["zones=map.zones".to_string(), "delta=9".to_string()];
        let parsed = Rules::parse(&words, &dir).unwrap();
        assert_eq!((parsed.delta, parsed.require[0].delta), (9, 9));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_zones_file_cannot_open_another() {
        let dir = std::env::temp_dir().join(format!("voidc-nested-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.zones"), "zones=a.zones\n").unwrap();
        assert!(Rules::parse(&["zones=a.zones".to_string()], &dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_zones_are_refused() {
        for word in [
            "ignore=1,2,3",
            "ignore=1,2,0,4",
            "require=1,2,3,4:x",
            "colour=red",
            "zones=/no/such/file",
        ] {
            assert!(
                Rules::parse(&[word.to_string()], Path::new(".")).is_err(),
                "{word}"
            );
        }
    }
}
