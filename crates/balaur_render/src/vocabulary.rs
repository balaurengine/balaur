//! Every word and key the render components spell, and the script constants
//! beside them. One list per crate, so a schema line, the reader behind it and
//! the read-back cannot disagree about a spelling.

/// The words a `shape`, `shape2d`, `camera` or `light2d` component spells, and
/// the script constants beside them. Written once so a matcher, a schema's
/// `options` list and the read-back cannot disagree.
pub(crate) mod words {
    use balaur_core::primitive::words as p;

    pub(crate) const SPHERE: &str = p::SPHERE;
    pub(crate) const BOX: &str = p::BOX;
    pub(crate) const CAPSULE: &str = p::CAPSULE;
    pub(crate) const CYLINDER: &str = p::CYLINDER;
    pub(crate) const CONE: &str = p::CONE;
    pub(crate) const PLANE: &str = p::PLANE;
    pub(crate) const TORUS: &str = p::TORUS;
    pub(crate) const PYRAMID: &str = p::PYRAMID;
    pub(crate) const PRISM: &str = p::PRISM;
    pub(crate) const TUBE: &str = p::TUBE;
    /// The 3D primitives, in the order the inspector offers them. The mesher
    /// owns the list, so a kind it can build is a kind a scene can name.
    pub(crate) const SHAPES: &[&str] = p::SOLIDS;

    pub(crate) const CIRCLE: &str = p::CIRCLE;
    pub(crate) const RECTANGLE: &str = p::RECTANGLE;
    pub(crate) const ELLIPSE: &str = p::ELLIPSE;
    pub(crate) const STAR: &str = p::STAR;
    pub(crate) const NGON: &str = p::NGON;
    pub(crate) const POLYLINE: &str = "polyline";
    /// The 2D primitives, and the chain of points that is not one of them.
    ///
    /// Taken from the mesher's own list rather than respelled, so a kind core
    /// learns to build is a kind a scene may name; `polyline` is appended
    /// because it follows a `mesh` or `path2d` asset instead of params.
    pub(crate) fn shapes_2d() -> Vec<&'static str> {
        p::FLATS.iter().copied().chain([POLYLINE]).collect()
    }

    /// Two more an occluder may read off a collider's params. `balaur_render`
    /// does not depend on `balaur_physics`, so the words are spelled here too.
    pub(crate) const TRIANGLE: &str = "triangle";
    pub(crate) const SEGMENT: &str = "segment";

    /// Which camera a `camera` node drives.
    pub(crate) const PERSPECTIVE: &str = "3d";
    pub(crate) const ORTHOGRAPHIC: &str = "2d";

    /// The passes a `camera`'s `post` list may name; any other name in it is a
    /// `material` asset.
    pub(crate) const BLOOM: &str = "bloom";
    pub(crate) const SSAO: &str = "ssao";
    pub(crate) const SSR: &str = "ssr";
    pub(crate) const DOF: &str = "dof";
    pub(crate) const TONEMAP: &str = "tonemap";
    pub(crate) const FXAA: &str = "fxaa";
    pub(crate) const SHARPEN: &str = "sharpen";
    pub(crate) const VIGNETTE: &str = "vignette";
    pub(crate) const ABERRATION: &str = "aberration";
    pub(crate) const GRAIN: &str = "grain";
    pub(crate) const PIXELATE: &str = "pixelate";
    /// The finishing passes the engine ships as post-process materials,
    /// rather than as flags on the pipeline. Named in the order they read
    /// best stacked, which is also the order the shader declares them.
    pub(crate) const FINISHES: &[&str] = &[VIGNETTE, ABERRATION, GRAIN, PIXELATE];
    pub(crate) const POST_EFFECTS: &[&str] = &[
        BLOOM, SSAO, SSR, DOF, FXAA, SHARPEN, TONEMAP, VIGNETTE, ABERRATION, GRAIN, PIXELATE,
    ];

    pub(crate) const POINT: &str = "point";
    pub(crate) const DIRECTIONAL: &str = "directional";
    pub(crate) const SPOT: &str = "spot";
    /// The 2D lights.
    pub(crate) const LIGHT_KINDS: &[&str] = &[POINT, DIRECTIONAL];
    /// The 3D lights, which add the cone the 2D ones have no room for.
    pub(crate) const LIGHT_KINDS_3D: &[&str] = &[DIRECTIONAL, POINT, SPOT];

    pub(crate) const LINEAR: &str = "linear";
    pub(crate) const EXPONENTIAL: &str = "exponential";
    pub(crate) const EXPONENTIAL_SQUARED: &str = "exponential_squared";
    pub(crate) const NONE: &str = "none";
    /// How fog thickens with distance, plus the word for no fog at all.
    pub(crate) const FOG_KINDS: &[&str] = &[NONE, LINEAR, EXPONENTIAL, EXPONENTIAL_SQUARED];

    pub(crate) const OPAQUE: &str = "opaque";
    pub(crate) const MASK: &str = "mask";
    pub(crate) const BLEND: &str = "blend";
    /// How a surface's alpha is read: ignored, a cutout, or a blend.
    pub(crate) const ALPHA_MODES: &[&str] = &[OPAQUE, MASK, BLEND];

    pub(crate) const ACES: &str = "aces";
    pub(crate) const REINHARD: &str = "reinhard";
    pub(crate) const AGX: &str = "agx";
    pub(crate) const NEUTRAL: &str = "neutral";
    /// The curves an `environment` maps its HDR film through.
    pub(crate) const TONEMAPS: &[&str] = &[NONE, ACES, REINHARD, AGX, NEUTRAL];

    pub(crate) const START: &str = "start";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const END: &str = "end";
    /// Where a block of text sits across its origin.
    pub(crate) const TEXT_ALIGNS: &[&str] = &[START, CENTER, END];

    pub(crate) const NORMAL: &str = "normal";
    pub(crate) const ITALIC: &str = "italic";
    /// Upright or slanted text.
    pub(crate) const FONT_STYLES: &[&str] = &[NORMAL, ITALIC];
}

/// The words as script constants, so a script writes `render.SHAPE_SPHERE`
/// rather than spelling "sphere" and finding out at runtime that "Sphere" fell
/// through to the default. One list: a capsule is a capsule in 2D and 3D.
pub(crate) const CONSTANTS: &[(&str, &str)] = &[
    ("SHAPE_SPHERE", words::SPHERE),
    ("SHAPE_BOX", words::BOX),
    ("SHAPE_CAPSULE", words::CAPSULE),
    ("SHAPE_CYLINDER", words::CYLINDER),
    ("SHAPE_CONE", words::CONE),
    ("SHAPE_PLANE", words::PLANE),
    ("SHAPE_TORUS", words::TORUS),
    ("SHAPE_PYRAMID", words::PYRAMID),
    ("SHAPE_PRISM", words::PRISM),
    ("SHAPE_TUBE", words::TUBE),
    ("SHAPE_CIRCLE", words::CIRCLE),
    ("SHAPE_RECTANGLE", words::RECTANGLE),
    ("SHAPE_ELLIPSE", words::ELLIPSE),
    ("SHAPE_STAR", words::STAR),
    ("SHAPE_NGON", words::NGON),
    ("SHAPE_POLYLINE", words::POLYLINE),
    ("LIGHT_POINT", words::POINT),
    ("LIGHT_DIRECTIONAL", words::DIRECTIONAL),
    ("LIGHT_SPOT", words::SPOT),
    ("ALPHA_OPAQUE", words::OPAQUE),
    ("ALPHA_MASK", words::MASK),
    ("ALPHA_BLEND", words::BLEND),
    ("FOG_NONE", words::NONE),
    ("FOG_LINEAR", words::LINEAR),
    ("FOG_EXPONENTIAL", words::EXPONENTIAL),
    ("FOG_EXPONENTIAL_SQUARED", words::EXPONENTIAL_SQUARED),
    ("TONEMAP_NONE", words::NONE),
    ("TONEMAP_ACES", words::ACES),
    ("TONEMAP_REINHARD", words::REINHARD),
    ("TONEMAP_AGX", words::AGX),
    ("TONEMAP_NEUTRAL", words::NEUTRAL),
    ("ALIGN_START", words::START),
    ("ALIGN_CENTER", words::CENTER),
    ("ALIGN_END", words::END),
    ("FONT_NORMAL", words::NORMAL),
    ("FONT_ITALIC", words::ITALIC),
];

/// Every property key the render components spell, so a schema line and the
/// reader behind it name the same key.
pub(crate) mod keys {
    use balaur_core::primitive::keys as p;

    /// A primitive's keys are the mesher's, so a schema line here and the
    /// reader there cannot drift apart.
    pub(crate) const AMBIENT_COLOR: &str = "ambient_color";
    pub(crate) const BITMAP_FONT: &str = "bitmap_font";
    pub(crate) const CAST_SHADOW: &str = "cast_shadow";
    pub(crate) const CORNER_RADIUS: &str = p::CORNER_RADIUS;
    pub(crate) const FOG_MODE: &str = "fog_mode";
    pub(crate) const FONT_FAMILY: &str = "font_family";
    pub(crate) const IMAGE_ROTATION_DEGREES: &str = "image_rotation_degrees";
    pub(crate) const INNER_ANGLE_DEGREES: &str = "inner_angle_degrees";
    pub(crate) const INNER_RADIUS: &str = p::INNER_RADIUS;
    pub(crate) const LIGHT_LAYERS: &str = "light_layers";
    pub(crate) const OPERATION: &str = "operation";
    pub(crate) const OUTER_ANGLE_DEGREES: &str = "outer_angle_degrees";
    pub(crate) const POINTS: &str = p::POINTS;
    pub(crate) const RANGE: &str = "range";
    pub(crate) const RINGS: &str = p::RINGS;
    pub(crate) const SEGMENTS: &str = p::SEGMENTS;
    pub(crate) const SHADOW_ENABLED: &str = "shadow_enabled";
    pub(crate) const SIDES: &str = p::SIDES;
    pub(crate) const SKY_ENABLED: &str = "sky_enabled";
    pub(crate) const SKY_ROTATION_DEGREES: &str = "sky_rotation_degrees";
    pub(crate) const TEXT_ALIGN: &str = "text_align";
    pub(crate) const TUBE_RADIUS: &str = p::TUBE_RADIUS;

    pub(crate) const A: &str = "a";
    pub(crate) const ALPHA_CUT: &str = "alpha_cut";
    pub(crate) const ANGLE: &str = "angle";
    pub(crate) const B: &str = "b";
    pub(crate) const BILLBOARD: &str = "billboard";
    pub(crate) const ABERRATION_AMOUNT: &str = "aberration_amount";
    pub(crate) const BLOOM_INTENSITY: &str = "bloom_intensity";
    pub(crate) const BLOOM_THRESHOLD: &str = "bloom_threshold";
    pub(crate) const GRAIN_AMOUNT: &str = "grain_amount";
    pub(crate) const PIXELATE_SIZE: &str = "pixelate_size";
    pub(crate) const SSAO_BIAS: &str = "ssao_bias";
    pub(crate) const SSAO_INTENSITY: &str = "ssao_intensity";
    pub(crate) const SSAO_POWER: &str = "ssao_power";
    pub(crate) const SSAO_RADIUS: &str = "ssao_radius";
    pub(crate) const VIGNETTE_AMOUNT: &str = "vignette_amount";
    pub(crate) const VIGNETTE_ROUNDNESS: &str = "vignette_roundness";
    pub(crate) const C: &str = "c";
    pub(crate) const CELLS: &str = "cells";
    pub(crate) const ORIGIN: &str = "origin";
    pub(crate) const FLAGS: &str = "flags";
    pub(crate) const TERRAIN: &str = "terrain";
    pub(crate) const SEED: &str = "seed";
    pub(crate) const CAP: &str = "cap";
    pub(crate) const CLOSED: &str = "closed";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const COLOR_END: &str = "color_end";
    pub(crate) const CENTERED: &str = "centered";
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const DEPTH_TEST: &str = "depth_test";
    pub(crate) const DOUBLE_SIDED: &str = "double_sided";
    pub(crate) const EMITTING: &str = "emitting";
    pub(crate) const EXPLOSIVENESS: &str = "explosiveness";
    pub(crate) const FALLOFF: &str = "falloff";
    pub(crate) const FLIP_X: &str = "flip_x";
    pub(crate) const FLIP_Y: &str = "flip_y";
    pub(crate) const FONT_SIZE: &str = "font_size";
    pub(crate) const FONT_STYLE: &str = "font_style";
    pub(crate) const FONT_WEIGHT: &str = "font_weight";
    pub(crate) const FRAME: &str = "frame";
    pub(crate) const GRADIENT: &str = "gradient";
    pub(crate) const GRADIENT_STEPS: &str = "gradient_steps";
    pub(crate) const GRAVITY: &str = "gravity";
    pub(crate) const HEIGHT: &str = p::HEIGHT;
    pub(crate) const IMAGE: &str = "image";
    pub(crate) const INTENSITY: &str = "intensity";
    /// A `draw_text` option; `text2d` spells it `font_style`.
    pub(crate) const MIRROR: &str = "mirror";
    pub(crate) const JOIN: &str = "join";
    pub(crate) const KIND: &str = p::KIND;
    pub(crate) const LETTER_SPACING: &str = "letter_spacing";
    pub(crate) const LIFETIME: &str = "lifetime";
    pub(crate) const LINE_HEIGHT: &str = "line_height";
    pub(crate) const LOOK_AT: &str = "look_at";
    pub(crate) const MATERIAL: &str = "material";
    pub(crate) const MARKUP: &str = "markup";
    pub(crate) const MAX_WIDTH: &str = "max_width";
    pub(crate) const MESH: &str = "mesh";
    pub(crate) const MITER_LIMIT: &str = "miter_limit";
    pub(crate) const OFFSET: &str = "offset";
    pub(crate) const ONE_SHOT: &str = "one_shot";
    pub(crate) const OUTLINE_COLOR: &str = "outline_color";
    pub(crate) const OUTLINE_SIZE: &str = "outline_size";
    pub(crate) const PIXELS_PER_UNIT: &str = "pixels_per_unit";
    pub(crate) const POST: &str = "post";
    pub(crate) const RADIUS: &str = p::RADIUS;
    pub(crate) const RATE: &str = "rate";
    pub(crate) const REGION_ORIGIN: &str = "region_origin";
    pub(crate) const REGION_SIZE: &str = "region_size";
    pub(crate) const Z_INDEX: &str = "z_index";
    pub(crate) const SHADOW_RESOLUTION: &str = "shadow_resolution";
    pub(crate) const SHADOW_SOFTNESS: &str = "shadow_softness";
    pub(crate) const SKY: &str = "sky";
    pub(crate) const SKY_INTENSITY: &str = "sky_intensity";
    pub(crate) const FOG_COLOR: &str = "fog_color";
    pub(crate) const FOG_DENSITY: &str = "fog_density";
    pub(crate) const FOG_START: &str = "fog_start";
    pub(crate) const FOG_END: &str = "fog_end";
    pub(crate) const FOG_HEIGHT_FALLOFF: &str = "fog_height_falloff";
    pub(crate) const EXPOSURE: &str = "exposure";
    pub(crate) const TONEMAP: &str = "tonemap";
    pub(crate) const SATURATION: &str = "saturation";
    pub(crate) const CONTRAST: &str = "contrast";
    pub(crate) const GAMMA: &str = "gamma";
    pub(crate) const SHADOW_DISTANCE: &str = "shadow_distance";
    pub(crate) const SHADOW_COLOR: &str = "shadow_color";
    /// A `draw_text` option, `[x, y]`; `text2d` splits it in two.
    pub(crate) const SHADOW_OFFSET: &str = "shadow_offset";
    pub(crate) const SHADOW_OFFSET_X: &str = "shadow_offset_x";
    pub(crate) const SHADOW_OFFSET_Y: &str = "shadow_offset_y";
    pub(crate) const SHEET: &str = "sheet";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const SIZE_END: &str = "size_end";
    pub(crate) const SKELETON: &str = "skeleton";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const SPEED: &str = "speed";
    pub(crate) const SPREAD: &str = "spread";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const TEXT_KEY: &str = "text_key";
    pub(crate) const TAPER: &str = "taper";
    pub(crate) const TEXTURE: &str = "texture";
    pub(crate) const TILESET: &str = "tileset";
    /// A `draw_text` option; `text2d` spells it `font_weight`.
    pub(crate) const WIDTH: &str = "width";
}

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
