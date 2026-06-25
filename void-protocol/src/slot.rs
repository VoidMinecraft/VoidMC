//! Item `Slot` wire type (Minecraft 1.20.5+ data-component form).
//!
//! On the wire an item slot is:
//!
//! ```text
//! count: VarInt
//! if count > 0 {
//!     item_id:               VarInt
//!     components_to_add:      VarInt (length)
//!     components_to_remove:   VarInt (length)
//!     add:    [ (type_id: VarInt, body…) ]
//!     remove: [ type_id: VarInt ]
//! }
//! ```
//!
//! Data-component bodies are **not** self-delimiting and there are ~100 of them,
//! so a fully generic decoder is impossible. We therefore:
//!
//! * **encode** with full fidelity — we only ever emit components we constructed,
//!   and [`DataComponent::Raw`] re-emits unparsed bodies verbatim; and
//! * **decode** a curated subset of components. An unknown component id can't be
//!   length-delimited, so [`Slot::decode`] returns [`DecodeError::InvalidLength`]
//!   for it (the dispatcher logs and drops the packet rather than crashing).
//!
//! In practice the only inbound slots we must decode in early phases come from
//! creative mode, where plain items carry **no** added components, so the curated
//! path is sufficient.

use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

/// Numeric ids from the `minecraft:data_component_type` registry, taken from the
/// registration order in Minecraft 26.1.2's `DataComponents` (verified against the
/// PaperMC `mcVersion=26.1.2` sources). Re-verify on a protocol bump — they are
/// only exercised when a slot actually carries components.
pub mod component_ids {
    pub const CUSTOM_DATA: i32 = 0;
    pub const MAX_STACK_SIZE: i32 = 1;
    pub const MAX_DAMAGE: i32 = 2;
    pub const DAMAGE: i32 = 3;
    pub const REPAIR_COST: i32 = 19;
}

/// A single structured data component carried by an [`ItemStack`](Slot).
///
/// The curated variants are (de)serialized losslessly. Anything outside the
/// curated set is represented as [`DataComponent::Raw`], which re-emits its body
/// verbatim on encode. `Raw` is never produced by [`Slot::decode`] (unparsed
/// bodies can't be length-delimited); it exists so server code can forward or
/// synthesize components it does not otherwise model.
#[derive(Debug, Clone, PartialEq)]
pub enum DataComponent {
    /// `minecraft:max_stack_size` — overrides the default stack size.
    MaxStackSize(i32),
    /// `minecraft:max_damage` — durability cap.
    MaxDamage(i32),
    /// `minecraft:damage` — current durability damage.
    Damage(i32),
    /// `minecraft:repair_cost` — anvil repair cost.
    RepairCost(i32),
    /// `minecraft:custom_data` — arbitrary NBT compound.
    CustomData(Nbt),
    /// Any component the server does not model: re-emitted verbatim.
    Raw { type_id: i32, body: Vec<u8> },
}

impl DataComponent {
    /// The numeric `minecraft:data_component_type` id of this component.
    pub fn type_id(&self) -> i32 {
        match self {
            DataComponent::MaxStackSize(_) => component_ids::MAX_STACK_SIZE,
            DataComponent::MaxDamage(_) => component_ids::MAX_DAMAGE,
            DataComponent::Damage(_) => component_ids::DAMAGE,
            DataComponent::RepairCost(_) => component_ids::REPAIR_COST,
            DataComponent::CustomData(_) => component_ids::CUSTOM_DATA,
            DataComponent::Raw { type_id, .. } => *type_id,
        }
    }

    /// Encodes the component's body only (the caller writes the type id).
    fn encode_body(&self, buf: &mut Vec<u8>) {
        match self {
            DataComponent::MaxStackSize(v)
            | DataComponent::MaxDamage(v)
            | DataComponent::Damage(v)
            | DataComponent::RepairCost(v) => VarI32(*v).encode(buf),
            DataComponent::CustomData(nbt) => nbt.encode(buf),
            DataComponent::Raw { body, .. } => buf.extend_from_slice(body),
        }
    }

    /// Decodes the body of a known component. Returns `None` for component ids
    /// outside the curated *decode* set (their bodies can't be length-delimited,
    /// so the slot decode bails rather than mis-framing later fields).
    ///
    /// NBT-bodied components (e.g. `custom_data`) are intentionally **not** in the
    /// decode set yet: the shared NBT codec under-advances the read cursor by one
    /// byte (`void-codec/src/primitives/nbt.rs`), which would corrupt any field
    /// following the NBT. They remain fully supported on *encode*. Re-enable here
    /// once that codec's cursor accounting is fixed (tracked for M6).
    fn decode_body(type_id: i32, buf: &mut &[u8]) -> Option<Result<DataComponent, DecodeError>> {
        let component = match type_id {
            component_ids::MAX_STACK_SIZE => VarI32::decode(buf).map(|v| Self::MaxStackSize(v.0)),
            component_ids::MAX_DAMAGE => VarI32::decode(buf).map(|v| Self::MaxDamage(v.0)),
            component_ids::DAMAGE => VarI32::decode(buf).map(|v| Self::Damage(v.0)),
            component_ids::REPAIR_COST => VarI32::decode(buf).map(|v| Self::RepairCost(v.0)),
            _ => return None,
        };
        Some(component)
    }
}

impl Encode for DataComponent {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.type_id()).encode(buf);
        self.encode_body(buf);
    }
}

/// A network item slot. `count <= 0` means the slot is empty and carries no
/// further fields on the wire.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slot {
    pub count: i32,
    pub item_id: i32,
    pub components_to_add: Vec<DataComponent>,
    pub components_to_remove: Vec<i32>,
}

impl Slot {
    /// The empty slot (single `0x00` byte on the wire).
    pub const EMPTY: Slot = Slot {
        count: 0,
        item_id: 0,
        components_to_add: Vec::new(),
        components_to_remove: Vec::new(),
    };

    /// A slot holding `count` of `item_id` with no components.
    pub fn simple(item_id: i32, count: i32) -> Self {
        Slot {
            count,
            item_id,
            components_to_add: Vec::new(),
            components_to_remove: Vec::new(),
        }
    }

    /// Whether the slot is empty.
    pub fn is_empty(&self) -> bool {
        self.count <= 0
    }
}

impl Encode for Slot {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.count).encode(buf);
        if self.count <= 0 {
            return;
        }
        VarI32(self.item_id).encode(buf);
        VarI32(self.components_to_add.len() as i32).encode(buf);
        VarI32(self.components_to_remove.len() as i32).encode(buf);
        for component in &self.components_to_add {
            component.encode(buf);
        }
        for &type_id in &self.components_to_remove {
            VarI32(type_id).encode(buf);
        }
    }
}

impl Decode for Slot {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let count = VarI32::decode(buf)?.0;
        if count <= 0 {
            return Ok(Slot::EMPTY);
        }
        let item_id = VarI32::decode(buf)?.0;
        let n_add = VarI32::decode(buf)?.0;
        let n_remove = VarI32::decode(buf)?.0;
        if n_add < 0 || n_remove < 0 {
            return Err(DecodeError::InvalidLength);
        }

        let mut components_to_add = Vec::with_capacity(n_add as usize);
        for _ in 0..n_add {
            let type_id = VarI32::decode(buf)?.0;
            match DataComponent::decode_body(type_id, buf) {
                Some(component) => components_to_add.push(component?),
                // Unknown component body — not self-delimiting, can't continue.
                None => return Err(DecodeError::InvalidLength),
            }
        }

        let mut components_to_remove = Vec::with_capacity(n_remove as usize);
        for _ in 0..n_remove {
            components_to_remove.push(VarI32::decode(buf)?.0);
        }

        Ok(Slot {
            count,
            item_id,
            components_to_add,
            components_to_remove,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ussr_nbt::owned::Tag;

    fn roundtrip(slot: &Slot) -> Slot {
        let mut buf = Vec::new();
        slot.encode(&mut buf);
        let mut slice = buf.as_slice();
        let decoded = Slot::decode(&mut slice).expect("decode");
        assert_eq!(slice.len(), 0, "all bytes consumed");
        decoded
    }

    #[test]
    fn empty_slot_is_single_zero_byte() {
        let mut buf = Vec::new();
        Slot::EMPTY.encode(&mut buf);
        assert_eq!(buf, vec![0x00]);
        assert_eq!(roundtrip(&Slot::EMPTY), Slot::EMPTY);
    }

    #[test]
    fn simple_slot_roundtrips() {
        let slot = Slot::simple(1, 64);
        assert_eq!(roundtrip(&slot), slot);
    }

    #[test]
    fn slot_with_varint_components_roundtrips() {
        let slot = Slot {
            count: 1,
            item_id: 895,
            components_to_add: vec![DataComponent::Damage(123), DataComponent::MaxStackSize(1)],
            components_to_remove: vec![component_ids::REPAIR_COST],
        };
        assert_eq!(roundtrip(&slot), slot);
    }

    #[test]
    fn custom_data_encodes_but_decode_is_deferred() {
        // CustomData is fully supported on encode (produces bytes), but NBT-bodied
        // component decode is deferred (see `decode_body`), so a slot carrying it
        // currently fails to decode rather than mis-framing — never panics.
        let nbt = Nbt {
            name: "".into(),
            compound: vec![("key".into(), Tag::Int(7))].into(),
        };
        let slot = Slot {
            count: 5,
            item_id: 1,
            components_to_add: vec![DataComponent::CustomData(nbt)],
            components_to_remove: Vec::new(),
        };
        let mut buf = Vec::new();
        slot.encode(&mut buf);
        assert!(buf.len() > 1, "custom_data produces a non-empty body");
        let mut slice = buf.as_slice();
        assert_eq!(Slot::decode(&mut slice), Err(DecodeError::InvalidLength));
    }

    #[test]
    fn raw_component_is_reemitted_verbatim() {
        // A raw component whose body is a single VarInt; encode then decode it as
        // the matching curated variant to prove the bytes are identical.
        let raw = Slot {
            count: 1,
            item_id: 1,
            components_to_add: vec![DataComponent::Raw {
                type_id: component_ids::DAMAGE,
                body: {
                    let mut b = Vec::new();
                    VarI32(42).encode(&mut b);
                    b
                },
            }],
            components_to_remove: Vec::new(),
        };
        let mut buf = Vec::new();
        raw.encode(&mut buf);
        let mut slice = buf.as_slice();
        let decoded = Slot::decode(&mut slice).expect("decode");
        assert_eq!(decoded.components_to_add, vec![DataComponent::Damage(42)]);
    }

    #[test]
    fn unknown_component_errors_instead_of_panicking() {
        // Hand-build a slot with one component of an unmodeled type id.
        let mut buf = Vec::new();
        VarI32(1).encode(&mut buf); // count
        VarI32(1).encode(&mut buf); // item_id
        VarI32(1).encode(&mut buf); // n_add
        VarI32(0).encode(&mut buf); // n_remove
        VarI32(9999).encode(&mut buf); // unknown component type id
        buf.push(0x00);
        let mut slice = buf.as_slice();
        assert_eq!(Slot::decode(&mut slice), Err(DecodeError::InvalidLength));
    }
}
