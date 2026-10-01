#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mobility {
    Walk,
    Fly,
    Swim,
}

/// How an entity moves through the voxel world, in blocks. A profile is plain
/// data: the pathfinder compiles it once per search.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationProfile {
    pub mobility: Mobility,
    pub width: f64,
    pub height: f64,
    pub step_height: f64,
    pub jump_height: f64,
    pub max_fall: u8,
    pub water_cost: Option<f32>,
    pub hazard_cost: Option<f32>,
    pub lava_cost: Option<f32>,
    pub jump_cost: f32,
    pub fall_cost: f32,
    pub max_nodes: u32,
    pub heuristic_weight: f32,
    pub allow_partial: bool,
}

impl Default for NavigationProfile {
    fn default() -> Self {
        Self::walker()
    }
}

impl NavigationProfile {
    /// A vanilla-like humanoid walker: 0.6 × 1.8, steps 0.6, jumps 1.25 and
    /// drops at most 3 blocks.
    pub fn walker() -> Self {
        Self {
            mobility: Mobility::Walk,
            width: 0.6,
            height: 1.8,
            step_height: 0.6,
            jump_height: 1.25,
            max_fall: 3,
            water_cost: Some(4.0),
            hazard_cost: None,
            lava_cost: None,
            jump_cost: 0.5,
            fall_cost: 0.1,
            max_nodes: 4096,
            heuristic_weight: 1.0,
            allow_partial: true,
        }
    }

    pub fn flyer() -> Self {
        Self {
            mobility: Mobility::Fly,
            width: 0.6,
            height: 0.9,
            water_cost: Some(2.0),
            ..Self::walker()
        }
    }

    pub fn swimmer() -> Self {
        Self {
            mobility: Mobility::Swim,
            width: 0.9,
            height: 0.6,
            water_cost: Some(1.0),
            ..Self::walker()
        }
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn step_height(mut self, height: f64) -> Self {
        self.step_height = height;
        self
    }

    pub fn jump_height(mut self, height: f64) -> Self {
        self.jump_height = height;
        self
    }

    pub fn max_fall(mut self, blocks: u8) -> Self {
        self.max_fall = blocks;
        self
    }

    pub fn avoid_water(mut self) -> Self {
        self.water_cost = None;
        self
    }

    pub fn water_cost(mut self, cost: f32) -> Self {
        self.water_cost = Some(cost);
        self
    }

    pub fn hazard_cost(mut self, cost: Option<f32>) -> Self {
        self.hazard_cost = cost;
        self
    }

    pub fn lava_cost(mut self, cost: Option<f32>) -> Self {
        self.lava_cost = cost;
        self
    }

    pub fn search_limit(mut self, max_nodes: u32) -> Self {
        self.max_nodes = max_nodes;
        self
    }

    /// Weighted A*: above 1.0 trades path optimality for fewer expansions.
    pub fn heuristic_weight(mut self, weight: f32) -> Self {
        self.heuristic_weight = weight.max(1.0);
        self
    }

    pub fn allow_partial(mut self, allow: bool) -> Self {
        self.allow_partial = allow;
        self
    }

    pub fn compile(&self) -> MoveModel {
        let sixteenths = |blocks: f64| (blocks * 16.0).round().max(0.0) as i32;
        let half_width = self.width.max(0.0) / 2.0;
        MoveModel {
            mobility: self.mobility,
            head: (self.height * 16.0).ceil().max(1.0) as i32,
            step: sixteenths(self.step_height),
            rise: sixteenths(self.step_height.max(self.jump_height)),
            max_fall: self.max_fall as i32 * 16,
            radius: if half_width <= 0.5 { 0 } else { 1 },
            half_width,
            water: self.water_cost,
            hazard: self.hazard_cost,
            lava: self.lava_cost,
            jump_cost: self.jump_cost,
            fall_cost: self.fall_cost,
            max_nodes: self.max_nodes.max(1),
            weight: self.heuristic_weight.max(1.0),
            allow_partial: self.allow_partial,
        }
    }
}

/// A [`NavigationProfile`] in the integer units the search uses: heights in
/// sixteenths of a block, footprint as a column radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveModel {
    pub mobility: Mobility,
    pub head: i32,
    pub step: i32,
    pub rise: i32,
    pub max_fall: i32,
    pub radius: i32,
    pub half_width: f64,
    pub water: Option<f32>,
    pub hazard: Option<f32>,
    pub lava: Option<f32>,
    pub jump_cost: f32,
    pub fall_cost: f32,
    pub max_nodes: u32,
    pub weight: f32,
    pub allow_partial: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_heights_to_sixteenths() {
        let model = NavigationProfile::walker().compile();
        assert_eq!(model.head, 29);
        assert_eq!(model.step, 10);
        assert_eq!(model.rise, 20);
        assert_eq!(model.max_fall, 48);
        assert_eq!(model.radius, 0);
    }

    #[test]
    fn wide_bodies_cover_neighbouring_columns() {
        let model = NavigationProfile::walker().size(1.4, 2.7).compile();
        assert_eq!(model.radius, 1);
        assert_eq!(model.head, 44);
    }
}
