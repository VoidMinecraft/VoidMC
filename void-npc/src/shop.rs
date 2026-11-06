//! An NPC that sells things: a list, a card for what is selected, and how many to buy.

use voidmc::{Inventory, ItemId, ItemStack, Sound, TextColor};
use voidmc_ui::Text;

use crate::art::{ArtBuilder, Icon, Picture, glass};
use crate::canvas::Canvas;
use crate::font::text_width;
use crate::interface::{Control, Cx, Grid, Interface, Reply, Screen};
use crate::npc::Theme;
use crate::rig::Gesture;
use crate::view::{from_top, grid_top};

const PANEL_X: i32 = 4;
/// Drawn this many times its size, which reaches past the bottom of any screen.
const PANEL_SCALE: u32 = 4;
const PANEL_SIZE: (u32, u32) = (120, 180);
const PANEL_ALPHA: u8 = 215;
const TOP: i32 = 52;
const ROW: i32 = 22;
const SMALL: i32 = 20;
const WIDE: i32 = 106;
const GAP: i32 = 2;
const LEFT: i32 = 36;
const TITLE_Y: i32 = 26;
const NOTE_Y: i32 = 38;
const ROWS_SHOWN: usize = 3;
/// Rows left empty for the card of what is selected.
const CARD_ROWS: i32 = 3;
const ICON_SIZE: u32 = 16;
const SMALL_ICON: u32 = 8;
const BUY_SOUND: &str = "entity.villager.yes";
const REFUSED_SOUND: &str = "entity.villager.no";
const BACK_LABEL: &str = "Retour";
const MAX_QUANTITY: u32 = 64;

fn card_y() -> i32 {
    grid_top(TOP) + ROW * ROWS_SHOWN as i32 + 4
}

/// Something for sale. Its limit is per player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    item: String,
    label: String,
    price: u32,
    bundle: u8,
    limit: Option<u32>,
    texture: String,
}

impl Offer {
    /// `item` is a name such as `totem_of_undying`, and `price` what one costs.
    pub fn new(item: &str, label: impl Into<String>, price: u32) -> Self {
        Self {
            item: format!("minecraft:{item}"),
            label: label.into(),
            price,
            bundle: 1,
            limit: None,
            texture: format!("item/{item}"),
        }
    }

    /// Sells this many at a time, for the price.
    pub fn bundle(mut self, count: u8) -> Self {
        self.bundle = count.max(1);
        self
    }

    pub fn limit(mut self, count: u32) -> Self {
        self.limit = Some(count);
        self
    }

    /// The picture to show, for what the game draws from a block model.
    pub fn texture(mut self, texture: impl Into<String>) -> Self {
        self.texture = texture.into();
        self
    }

    fn stack(&self, count: u8) -> Option<ItemStack> {
        ItemStack::of(&self.item, count)
    }
}

#[derive(Debug, Clone, Default)]
struct Icons {
    rows: Vec<Icon>,
    card: Option<Icon>,
}

/// Sells [`Offer`]s for one item.
#[derive(Debug, Clone)]
pub struct Shop {
    title: String,
    currency: String,
    currency_name: String,
    offers: Vec<Offer>,
    panel: Option<Picture>,
    balance_icon: Option<Icon>,
    price_icon: Option<Icon>,
    icons: Vec<Icons>,
}

/// What a player is looking at in a [`Shop`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Basket {
    page: usize,
    selected: usize,
    quantity: u32,
    note: Option<(String, bool)>,
}

impl Default for Basket {
    fn default() -> Self {
        Self {
            page: 0,
            selected: 0,
            quantity: 1,
            note: None,
        }
    }
}

fn count_of(inventory: &Inventory, item: ItemId) -> u32 {
    (0..Inventory::SIZE)
        .map(|slot| inventory.get(slot))
        .filter(|stack| stack.item == item)
        .map(|stack| u32::from(stack.count))
        .sum()
}

/// Takes `count` of `item`, last slots first, which leaves the hotbar for last.
fn take(inventory: &mut Inventory, item: ItemId, mut count: u32) {
    for slot in (Inventory::MAIN_START..Inventory::OFFHAND).rev() {
        if count == 0 {
            return;
        }
        let mut stack = inventory.get(slot).clone();
        if stack.item != item {
            continue;
        }
        let taken = count.min(u32::from(stack.count));
        count -= taken;
        stack.count -= taken as u8;
        inventory.set(
            slot,
            if stack.count == 0 {
                ItemStack::EMPTY
            } else {
                stack
            },
        );
    }
}

impl Shop {
    /// Paid for in `currency`, an item name such as `emerald`.
    pub fn new(title: impl Into<String>, currency: &str, currency_name: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            currency: currency.to_string(),
            currency_name: currency_name.into(),
            offers: Vec::new(),
            panel: None,
            balance_icon: None,
            price_icon: None,
            icons: Vec::new(),
        }
    }

    pub fn offer(mut self, offer: Offer) -> Self {
        self.offers.push(offer);
        self
    }

    fn currency_item(&self) -> ItemId {
        ItemId::from_name(&format!("minecraft:{}", self.currency)).unwrap_or(ItemId::AIR)
    }

    fn left(&self, offer: &Offer, memory: &crate::Memory) -> Option<u32> {
        let bought = memory.count(&offer.item);
        offer.limit.map(|limit| limit.saturating_sub(bought))
    }

    /// The most `quantity` can go up to, on what is for sale.
    fn most(&self, offer: &Offer, memory: &crate::Memory) -> u32 {
        self.left(offer, memory)
            .unwrap_or(MAX_QUANTITY)
            .clamp(1, MAX_QUANTITY)
    }

    fn buy(&self, basket: &mut Basket, cx: &mut Cx) {
        let Some(offer) = self.offers.get(basket.selected) else {
            return;
        };
        let cost = offer.price * basket.quantity;
        let currency = self.currency_item();
        let refused = |basket: &mut Basket, cx: &mut Cx, why: String| {
            basket.note = Some((why, false));
            cx.play(Sound::new(REFUSED_SOUND));
        };
        if self
            .left(offer, cx.memory)
            .is_some_and(|left| left < basket.quantity)
        {
            return refused(basket, cx, "Plus en stock pour toi.".to_string());
        }
        if count_of(cx.inventory, currency) < cost {
            return refused(basket, cx, format!("Pas assez : {}.", self.currency_name));
        }
        let mut after = cx.inventory.clone();
        take(&mut after, currency, cost);
        let total = u32::from(offer.bundle) * basket.quantity;
        for _ in 0..total.div_ceil(64) {
            let count = total.min(64) as u8;
            let Some(stack) = offer.stack(count) else {
                return;
            };
            if !after.give(stack).is_empty() {
                return refused(basket, cx, "Ton sac est plein.".to_string());
            }
        }
        take(cx.inventory, currency, cost);
        for _ in 0..total.div_ceil(64) {
            if let Some(stack) = offer.stack(total.min(64) as u8) {
                cx.inventory.give(stack);
            }
        }
        cx.memory.add(offer.item.clone(), basket.quantity);
        basket.note = Some((
            format!("Achat réussi ! -{cost} {}", self.currency_name),
            true,
        ));
        cx.play(Sound::new(BUY_SOUND));
        cx.gesture(Gesture::Nod);
        cx.emit(format!("bought:{}", offer.item));
    }
}

impl Interface for Shop {
    type State = Basket;

    fn install(&mut self, art: &mut ArtBuilder, _theme: &Theme) {
        let (width, height) = PANEL_SIZE;
        self.panel = Some(art.scaled(glass(width, height, PANEL_ALPHA), from_top(0), PANEL_SCALE));
        let currency_texture = format!("item/{}", self.currency);
        self.balance_icon = Some(art.icon(&currency_texture, SMALL_ICON, from_top(TITLE_Y)));
        self.price_icon = Some(art.icon(&currency_texture, SMALL_ICON, from_top(card_y() + 18)));
        self.icons = self
            .offers
            .iter()
            .map(|offer| Icons {
                rows: (0..ROWS_SHOWN)
                    .map(|row| {
                        let y = grid_top(TOP) + ROW * row as i32 + 2;
                        art.icon(&offer.texture, ICON_SIZE, from_top(y))
                    })
                    .collect(),
                card: Some(art.icon(&offer.texture, ICON_SIZE, from_top(card_y()))),
            })
            .collect();
    }

    fn view(&self, basket: &Basket, cx: &mut Cx) -> Screen {
        let accent = TextColor::Rgb(cx.theme.accent);
        let right = LEFT + SMALL * 2 + WIDE + GAP * 2;
        let mut canvas = Canvas::new();
        if let Some(panel) = &self.panel {
            canvas.picture(PANEL_X, panel, TextColor::Rgb(cx.theme.shade()));
        }
        let title = format!("{} • {}", cx.name, self.title);
        canvas.text(LEFT, from_top(TITLE_Y), &title, accent);
        let balance = count_of(cx.inventory, self.currency_item()).to_string();
        let balance_x = right - text_width(&balance);
        canvas.text(balance_x, from_top(TITLE_Y), &balance, TextColor::White);
        if let Some((note, good)) = &basket.note {
            let color = if *good { accent } else { TextColor::Red };
            canvas.text(LEFT, from_top(NOTE_Y), note, color);
        }
        let first = basket.page * ROWS_SHOWN;
        let shown = self.offers.iter().enumerate().skip(first).take(ROWS_SHOWN);
        let mut rows: Vec<Vec<Option<Control>>> = Vec::new();
        for (index, offer) in shown {
            let mark = if index == basket.selected { "» " } else { "" };
            let label = Text::new(format!("{mark}{}", offer.label));
            let label = if index == basket.selected {
                label.color(accent)
            } else {
                label
            };
            rows.push(vec![
                None,
                None,
                Some(Control::action(label, &format!("select/{index}"))),
            ]);
        }
        rows.resize(ROWS_SHOWN, vec![None, None, None]);
        rows.extend((0..CARD_ROWS).map(|_| vec![None, None, None]));
        let selected = self.offers.get(basket.selected);
        if let Some(offer) = selected {
            let y = from_top(card_y());
            canvas.text(LEFT + 20, y + 4, &offer.label, TextColor::White);
            let cost = offer.price * basket.quantity;
            let can_pay = count_of(cx.inventory, self.currency_item()) >= cost;
            let price_color = if can_pay {
                TextColor::Green
            } else {
                TextColor::Red
            };
            let price = format!("{cost} {}", self.currency_name);
            canvas.text(LEFT, y + 18, "Prix :", TextColor::White);
            canvas.text(LEFT + 44, y + 18, &price, price_color);
            let stock = match self.left(offer, cx.memory) {
                Some(left) => format!("Limite restante : {left}"),
                None => "Limite restante : illimitée".to_string(),
            };
            canvas.text(LEFT, y + 30, &stock, TextColor::Gray);
        }
        rows.push(vec![
            Some(Control::action(Text::new("-"), "less")),
            Some(Control::action(Text::new("+"), "more")),
            Some(Control::action(
                Text::new(format!("Acheter ×{}", basket.quantity)),
                "buy",
            )),
        ]);
        rows.push(vec![
            None,
            None,
            Some(Control::action(Text::new(BACK_LABEL), "back")),
        ]);
        if self.offers.len() > ROWS_SHOWN {
            let pages = self.offers.len().div_ceil(ROWS_SHOWN);
            let last = rows.len() - 1;
            rows[last][0] = Some(Control::action(Text::new("<"), "previous"));
            rows[last][1] = Some(Control::action(Text::new(">"), "following"));
            let page = format!("{}/{pages}", basket.page + 1);
            canvas.text(
                right - text_width(&page),
                from_top(NOTE_Y),
                &page,
                TextColor::Gray,
            );
        }
        if let Some(icon) = &self.balance_icon {
            canvas.icon(balance_x - 10, icon);
        }
        if let (Some(icon), Some(_)) = (&self.price_icon, selected) {
            canvas.icon(LEFT + 34, icon);
        }
        for (place, index) in (first..self.offers.len().min(first + ROWS_SHOWN)).enumerate() {
            if let Some(icons) = self.icons.get(index) {
                canvas.icon(LEFT + 21, &icons.rows[place]);
            }
        }
        if let Some(icon) = self
            .icons
            .get(basket.selected)
            .and_then(|icons| icons.card.as_ref())
        {
            canvas.icon(LEFT, icon);
        }
        Screen {
            canvas,
            grid: Grid {
                x: LEFT,
                top: TOP,
                widths: vec![SMALL, SMALL, WIDE],
                rows,
            },
        }
    }

    fn click(&self, basket: &mut Basket, action: &str, cx: &mut Cx) -> Reply {
        basket.note = None;
        let most = self
            .offers
            .get(basket.selected)
            .map_or(1, |offer| self.most(offer, cx.memory));
        match action.split_once('/') {
            Some(("select", index)) => {
                if let Some(index) = index.parse().ok().filter(|at| *at < self.offers.len()) {
                    basket.selected = index;
                    basket.quantity = 1;
                }
            }
            _ => match action {
                "less" => basket.quantity = basket.quantity.saturating_sub(1).max(1),
                "more" => basket.quantity = (basket.quantity + 1).min(most),
                "buy" => self.buy(basket, cx),
                "back" => return Reply::Back,
                "previous" => basket.page = basket.page.saturating_sub(1),
                "following" => {
                    let last = self.offers.len().div_ceil(ROWS_SHOWN).saturating_sub(1);
                    basket.page = (basket.page + 1).min(last);
                }
                _ => {}
            },
        }
        Reply::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;

    fn shop() -> Shop {
        Shop::new("Boutique", "emerald", "émeraudes")
            .offer(Offer::new("totem_of_undying", "Totem", 50))
            .offer(Offer::new("arrow", "Flèches", 2).bundle(16).limit(2))
    }

    fn purse(emeralds: u8) -> Inventory {
        let mut inventory = Inventory::new();
        inventory.give(ItemStack::of("minecraft:emerald", emeralds).unwrap());
        inventory
    }

    fn with_cx<R>(
        inventory: &mut Inventory,
        memory: &mut Memory,
        run: impl FnOnce(&mut Cx) -> R,
    ) -> R {
        let theme = Theme::EMERALD;
        let mut cx = Cx {
            inventory,
            memory,
            theme: &theme,
            name: "Marchande",
            sounds: Vec::new(),
            events: Vec::new(),
            props: Vec::new(),
        };
        run(&mut cx)
    }

    fn emeralds(inventory: &Inventory) -> u32 {
        count_of(inventory, ItemId::from_name("minecraft:emerald").unwrap())
    }

    fn totems(inventory: &Inventory) -> u32 {
        count_of(
            inventory,
            ItemId::from_name("minecraft:totem_of_undying").unwrap(),
        )
    }

    #[test]
    fn buying_takes_the_price_and_gives_the_item() {
        let (mut inventory, mut memory) = (purse(64), Memory::default());
        let mut basket = Basket::default();
        with_cx(&mut inventory, &mut memory, |cx| {
            assert_eq!(shop().click(&mut basket, "buy", cx), Reply::Stay);
        });
        assert_eq!(emeralds(&inventory), 14);
        assert_eq!(totems(&inventory), 1);
        assert!(matches!(basket.note, Some((_, true))));
    }

    #[test]
    fn without_the_money_nothing_changes_hands() {
        let (mut inventory, mut memory) = (purse(10), Memory::default());
        let mut basket = Basket::default();
        with_cx(&mut inventory, &mut memory, |cx| {
            shop().click(&mut basket, "buy", cx);
        });
        assert_eq!(emeralds(&inventory), 10);
        assert_eq!(totems(&inventory), 0);
        assert!(matches!(basket.note, Some((_, false))));
    }

    #[test]
    fn a_full_bag_loses_nothing() {
        let mut inventory = purse(64);
        for slot in Inventory::MAIN_START..Inventory::OFFHAND {
            if inventory.get(slot).is_empty() {
                inventory.set(slot, ItemStack::of("minecraft:dirt", 64).unwrap());
            }
        }
        let mut memory = Memory::default();
        let mut basket = Basket::default();
        with_cx(&mut inventory, &mut memory, |cx| {
            shop().click(&mut basket, "buy", cx);
        });
        assert_eq!(emeralds(&inventory), 64);
        assert!(matches!(basket.note, Some((_, false))));
    }

    #[test]
    fn a_limit_holds_across_visits() {
        let (mut inventory, mut memory) = (purse(64), Memory::default());
        let shop = shop();
        for _ in 0..3 {
            let mut basket = Basket {
                selected: 1,
                ..Basket::default()
            };
            with_cx(&mut inventory, &mut memory, |cx| {
                shop.click(&mut basket, "buy", cx);
            });
        }
        assert_eq!(
            count_of(&inventory, ItemId::from_name("minecraft:arrow").unwrap()),
            32
        );
        assert_eq!(emeralds(&inventory), 60);
    }

    #[test]
    fn the_quantity_stays_between_one_and_what_is_left() {
        let (mut inventory, mut memory) = (purse(64), Memory::default());
        let shop = shop();
        let mut basket = Basket {
            selected: 1,
            ..Basket::default()
        };
        with_cx(&mut inventory, &mut memory, |cx| {
            shop.click(&mut basket, "less", cx);
            assert_eq!(basket.quantity, 1);
            for _ in 0..5 {
                shop.click(&mut basket, "more", cx);
            }
        });
        assert_eq!(basket.quantity, 2);
    }

    #[test]
    fn a_click_past_the_list_selects_nothing() {
        let (mut inventory, mut memory) = (purse(1), Memory::default());
        let mut basket = Basket::default();
        with_cx(&mut inventory, &mut memory, |cx| {
            shop().click(&mut basket, "select/9", cx);
        });
        assert_eq!(basket.selected, 0);
    }

    #[test]
    fn back_goes_back() {
        let (mut inventory, mut memory) = (purse(1), Memory::default());
        with_cx(&mut inventory, &mut memory, |cx| {
            assert_eq!(
                shop().click(&mut Basket::default(), "back", cx),
                Reply::Back
            );
        });
    }
}
