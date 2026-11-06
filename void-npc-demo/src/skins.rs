//! Skins drawn by hand, in code: each part of the model has its place in a 64 by 64 picture.

use voidmc_ui::Image;

type Color = [u8; 4];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r, g, b, 255]
}

fn fill(image: &mut Image, (x, y, width, height): (u32, u32, u32, u32), color: Color) {
    for row in y..y + height {
        for column in x..x + width {
            image.set(column, row, color);
        }
    }
}

/// A limb with the same colour all round, and a darker end.
fn limb(image: &mut Image, at: (u32, u32), color: Color, end: Color, end_height: u32) {
    let (x, y) = at;
    fill(image, (x + 4, y, 8, 4), color);
    fill(image, (x, y + 4, 16, 12), color);
    fill(image, (x, y + 16 - end_height, 16, end_height), end);
}

struct Look {
    skin: Color,
    hair: Color,
    eyes: Color,
    robe: Color,
    trim: Color,
    boots: Color,
}

fn person(look: &Look) -> Image {
    let mut image = Image::new(64, 64);
    fill(&mut image, (8, 0, 16, 8), look.hair);
    fill(&mut image, (0, 8, 32, 8), look.hair);
    fill(&mut image, (8, 10, 8, 6), look.skin);
    fill(&mut image, (9, 12, 2, 1), rgb(245, 245, 245));
    fill(&mut image, (13, 12, 2, 1), rgb(245, 245, 245));
    fill(&mut image, (10, 12, 1, 1), look.eyes);
    fill(&mut image, (13, 12, 1, 1), look.eyes);
    fill(&mut image, (11, 14, 2, 1), rgb(150, 90, 70));
    fill(&mut image, (20, 16, 16, 4), look.robe);
    fill(&mut image, (16, 20, 24, 12), look.robe);
    fill(&mut image, (23, 20, 2, 12), look.trim);
    fill(&mut image, (20, 28, 8, 1), look.trim);
    limb(&mut image, (40, 16), look.robe, look.skin, 2);
    limb(&mut image, (32, 48), look.robe, look.skin, 2);
    limb(&mut image, (0, 16), look.robe, look.boots, 3);
    limb(&mut image, (16, 48), look.robe, look.boots, 3);
    image
}

pub fn merchant() -> Image {
    let mut image = person(&Look {
        skin: rgb(226, 178, 138),
        hair: rgb(30, 74, 58),
        eyes: rgb(36, 120, 90),
        robe: rgb(38, 96, 74),
        trim: rgb(222, 184, 84),
        boots: rgb(62, 44, 32),
    });
    fill(&mut image, (40, 0, 16, 8), rgb(24, 60, 48));
    fill(&mut image, (32, 8, 8, 8), rgb(24, 60, 48));
    fill(&mut image, (48, 8, 16, 8), rgb(24, 60, 48));
    fill(&mut image, (40, 8, 8, 2), rgb(24, 60, 48));
    image
}

pub fn fortune_teller() -> Image {
    let mut image = person(&Look {
        skin: rgb(186, 138, 112),
        hair: rgb(214, 214, 228),
        eyes: rgb(150, 90, 220),
        robe: rgb(62, 40, 104),
        trim: rgb(198, 150, 240),
        boots: rgb(36, 24, 58),
    });
    fill(&mut image, (40, 0, 16, 8), rgb(92, 56, 150));
    fill(&mut image, (32, 8, 32, 2), rgb(92, 56, 150));
    fill(&mut image, (43, 8, 2, 2), rgb(240, 210, 110));
    image
}
