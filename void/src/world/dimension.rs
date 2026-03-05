/// Identifies a dimension in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DimensionId {
    Overworld,
    Nether,
    End,
}

impl DimensionId {
    pub fn protocol_id(&self) -> i32 {
        match self {
            DimensionId::Overworld => 0,
            DimensionId::Nether => 1,
            DimensionId::End => 2,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            DimensionId::Overworld => "minecraft:overworld",
            DimensionId::Nether => "minecraft:the_nether",
            DimensionId::End => "minecraft:the_end",
        }
    }
}
