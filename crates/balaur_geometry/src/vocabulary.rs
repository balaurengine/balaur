//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) mod mesh_formats {
        pub(crate) const GLB: &str = "glb";
        pub(crate) const GLTF: &str = "gltf";
        pub(crate) const OBJ: &str = "obj";
        pub(crate) const VN: &str = "vn";
        pub(crate) const VT: &str = "vt";
    }

    pub(crate) mod tile_rules {
        pub(crate) const CORNERS: &str = "corners";
        pub(crate) const CORNERS_AND_SIDES: &str = "corners_and_sides";
        pub(crate) const EMPTY: &str = "empty";
        pub(crate) const MIRROR: &str = "mirror";
        pub(crate) const QUARTERS: &str = "quarters";
        pub(crate) const ROTATE: &str = "rotate";
        pub(crate) const RULES: &str = "rules";
        pub(crate) const SAME: &str = "same";
        pub(crate) const SIDES: &str = "sides";
    }

    pub(crate) mod tile_shapes {
        pub(crate) const FULL: &str = "full";
        pub(crate) const HEX: &str = "hex";
        pub(crate) const ISOMETRIC: &str = "isometric";
        pub(crate) const NONE: &str = "none";
        pub(crate) const ORTHOGONAL: &str = "orthogonal";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const CHANCE: &str = "chance";
    pub(crate) const FPS: &str = "fps";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const OUTSIDE: &str = "outside";
    pub(crate) const QUARTERS: &str = "quarters";
    pub(crate) const TERRAIN: &str = "terrain";
    pub(crate) const TILE: &str = "tile";
    pub(crate) const TILES: &str = "tiles";
}
