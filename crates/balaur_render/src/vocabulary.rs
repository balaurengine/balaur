//! Every word and key the render components spell, and the script constants
//! beside them. One list per crate, so a schema line, the reader behind it and
//! the read-back cannot disagree about a spelling.

/// The words a `shape`, `shape2d`, `camera` or `light2d` component spells, and
/// the script constants beside them. Written once so a matcher, a schema's
/// `options` list and the read-back cannot disagree.
#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    use balaur_geometry::primitive::words as p;

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

    pub(crate) const CIRCLE: &str = p::CIRCLE;
    pub(crate) const RECTANGLE: &str = p::RECTANGLE;
    pub(crate) const ELLIPSE: &str = p::ELLIPSE;
    pub(crate) const STAR: &str = p::STAR;
    pub(crate) const NGON: &str = p::NGON;

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
    pub(crate) const CRT: &str = "crt";
    pub(crate) const GRAYSCALE: &str = "grayscale";
    pub(crate) const WAVES: &str = "waves";
    pub(crate) const LOUPE: &str = "loupe";
    pub(crate) const STEREO: &str = "stereo";
    pub(crate) const EDGES: &str = "edges";
    /// kiss3d's 2D global illumination, on `camera2d`.
    pub(crate) const GI: &str = "gi";
    pub(crate) const RAY_MARCH: &str = "ray_march";
    pub(crate) const CASCADES: &str = "cascades";
    pub(crate) const GI_SOLVERS: &[&str] = &[RAY_MARCH, CASCADES];
    /// The finishing passes the engine ships as post-process materials,
    /// rather than as flags on the pipeline. Named in the order they read
    /// best stacked, which is also the order the shader declares them.
    pub(crate) const FINISHES: &[&str] = &[VIGNETTE, ABERRATION, GRAIN, PIXELATE];
    /// The passes kiss3d draws as effects of their own, on the chain.
    pub(crate) const EFFECTS: &[&str] = &[
        FXAA, SHARPEN, CRT, GRAYSCALE, WAVES, LOUPE, STEREO, EDGES, GI,
    ];
    pub(crate) const POST_EFFECTS: &[&str] = &[
        BLOOM, SSAO, SSR, DOF, FXAA, SHARPEN, TONEMAP, VIGNETTE, ABERRATION, GRAIN, PIXELATE, CRT,
        GRAYSCALE, WAVES, LOUPE, STEREO, EDGES, GI,
    ];

    pub(crate) const BOKEH: &str = "bokeh";
    pub(crate) const GAUSSIAN: &str = "gaussian";
    /// How the `dof` pass blurs.
    pub(crate) const DOF_MODES: &[&str] = &[BOKEH, GAUSSIAN];

    pub(crate) const TOP_LEFT: &str = "top_left";
    pub(crate) const TOP_RIGHT: &str = "top_right";
    pub(crate) const BOTTOM_LEFT: &str = "bottom_left";
    pub(crate) const BOTTOM_RIGHT: &str = "bottom_right";
    /// Where the `loupe` pass draws its inset.
    pub(crate) const LOUPE_CORNERS: &[&str] = &[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT];

    pub(crate) const AOV_DEPTH: &str = "depth";
    pub(crate) const AOV_NORMALS: &str = "normals";
    pub(crate) const AOV_CAMERA_NORMALS: &str = "camera_normals";
    pub(crate) const AOV_SEGMENTATION: &str = "segmentation";
    /// What `render.snap_aov` renders the scene as.
    pub(crate) const AOVS: &[&str] =
        &[AOV_DEPTH, AOV_NORMALS, AOV_CAMERA_NORMALS, AOV_SEGMENTATION];

    /// What a multimesh's `populate` lays its instances out as: scattered
    /// over a surface node, or in a row, a ring or a grid.
    pub(crate) const SURFACE: &str = "surface";
    pub(crate) const ROW: &str = "row";
    pub(crate) const RING: &str = "ring";
    pub(crate) const GRID: &str = "grid";

    pub(crate) const POINT: &str = "point";
    pub(crate) const DIRECTIONAL: &str = "directional";
    pub(crate) const SPOT: &str = "spot";

    pub(crate) const LINEAR: &str = "linear";
    pub(crate) const EXPONENTIAL: &str = "exponential";
    pub(crate) const EXPONENTIAL_SQUARED: &str = "exponential_squared";
    pub(crate) const NONE: &str = "none";

    pub(crate) const OPAQUE: &str = "opaque";
    pub(crate) const MASK: &str = "mask";
    pub(crate) const BLEND: &str = "blend";
    pub(crate) const PREMULTIPLIED: &str = "premultiplied";
    pub(crate) const AUTO: &str = "auto";
    pub(crate) const ADD: &str = "add";
    pub(crate) const MULTIPLY: &str = "multiply";
    pub(crate) const SCREEN: &str = "screen";
    /// How a surface's alpha is read: as its dimension draws one by default,
    /// ignored, a cutout, a blend, a blend of a colour that already carries
    /// its alpha, or one of the 2D blends that light, shade or screen.
    pub(crate) const ALPHA_MODES: &[&str] = &[
        AUTO,
        OPAQUE,
        MASK,
        BLEND,
        PREMULTIPLIED,
        ADD,
        MULTIPLY,
        SCREEN,
    ];

    pub(crate) const OCCLUSION: &str = "occlusion";
    pub(crate) const RELIEF: &str = "relief";
    /// How a height map is searched for the point a ray meets.
    pub(crate) const PARALLAX_METHODS: &[&str] = &[OCCLUSION, RELIEF];

    pub(crate) const GLASS: &str = "glass";
    pub(crate) const METAL: &str = "metal";
    pub(crate) const LIGHT: &str = "light";
    /// What the path tracer takes a surface for.
    pub(crate) const TRACE_SURFACES: &[&str] = &[OPAQUE, GLASS, METAL, LIGHT];

    pub(crate) const UVS: &str = "uvs";
    /// The debug looks a shader-less material can draw as, kiss3d's named
    /// built-in materials.
    pub(crate) const MATERIAL_VIEWS: &[&str] = &[AOV_NORMALS, UVS];

    pub(crate) const ACES: &str = "aces";
    pub(crate) const REINHARD: &str = "reinhard";
    pub(crate) const AGX: &str = "agx";
    pub(crate) const NEUTRAL: &str = "neutral";
    pub(crate) const TONY_MCMAPFACE: &str = "tony_mcmapface";

    pub(crate) const START: &str = "start";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const END: &str = "end";
    pub(crate) const JUSTIFY: &str = "justify";

    pub(crate) const NORMAL: &str = "normal";
    pub(crate) const ITALIC: &str = "italic";

    pub(crate) const WORD_OR_GLYPH: &str = "word_or_glyph";

    pub(crate) const LEFT: &str = "left";
    pub(crate) const RIGHT: &str = "right";
    #[allow(dead_code, reason = "read only behind the aseprite feature")]
    pub(crate) const PINGPONG: &str = "pingpong";

    pub(crate) mod options {
        pub(crate) const HOLES: &str = "holes";
        pub(crate) const PIXELS_PER_UNIT: &str = "pixels_per_unit";
        pub(crate) const THRESHOLD: &str = "threshold";
        pub(crate) const TOLERANCE: &str = "tolerance";
    }

    #[allow(dead_code, reason = "read only behind the compile feature")]
    pub(crate) mod shader_types {
        pub(crate) const BALAUR_PROBE: &str = "balaur_probe";
        pub(crate) const F32: &str = "f32";
        pub(crate) const VEC2: &str = "vec2";
        pub(crate) const VEC2F: &str = "vec2f";
        pub(crate) const VEC3: &str = "vec3";
        pub(crate) const VEC3F: &str = "vec3f";
        pub(crate) const VEC4: &str = "vec4";
        pub(crate) const VEC4F: &str = "vec4f";
    }

    pub(crate) mod functions {
        pub(crate) const SHADER_PROBE: &str = "shader_probe";
    }

    pub(crate) mod general {
        pub(crate) const RENDER: &str = "render";
    }

    pub(crate) mod material_check {
        pub(crate) const TYPE: &str = "type";
    }

    pub(crate) mod pipeline {
        pub(crate) const FS_MAIN: &str = "fs_main";
        pub(crate) const VS_MAIN: &str = "vs_main";
    }

    pub(crate) mod shader_material {
        pub(crate) const MATERIAL: &str = "material";
    }
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
    ("LIGHT_POINT", words::POINT),
    ("LIGHT_DIRECTIONAL", words::DIRECTIONAL),
    ("LIGHT_SPOT", words::SPOT),
    ("ALPHA_OPAQUE", words::OPAQUE),
    ("ALPHA_MASK", words::MASK),
    ("ALPHA_BLEND", words::BLEND),
    ("ALPHA_PREMULTIPLIED", words::PREMULTIPLIED),
    ("FOG_NONE", words::NONE),
    ("FOG_LINEAR", words::LINEAR),
    ("FOG_EXPONENTIAL", words::EXPONENTIAL),
    ("FOG_EXPONENTIAL_SQUARED", words::EXPONENTIAL_SQUARED),
    ("TONEMAP_NONE", words::NONE),
    ("TONEMAP_ACES", words::ACES),
    ("TONEMAP_REINHARD", words::REINHARD),
    ("TONEMAP_AGX", words::AGX),
    ("TONEMAP_NEUTRAL", words::NEUTRAL),
    ("TONEMAP_TONY_MCMAPFACE", words::TONY_MCMAPFACE),
    ("ALIGN_START", words::START),
    ("ALIGN_CENTER", words::CENTER),
    ("ALIGN_END", words::END),
    ("FONT_NORMAL", words::NORMAL),
    ("FONT_ITALIC", words::ITALIC),
    ("AOV_DEPTH", words::AOV_DEPTH),
    ("AOV_NORMALS", words::AOV_NORMALS),
    ("AOV_CAMERA_NORMALS", words::AOV_CAMERA_NORMALS),
    ("AOV_SEGMENTATION", words::AOV_SEGMENTATION),
];

/// Every property key the render components spell, so a schema line and the
/// reader behind it name the same key.
#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    use balaur_geometry::primitive::keys as p;

    pub(crate) const CAST_SHADOW: &str = "cast_shadow";
    pub(crate) const LIGHT_LAYERS: &str = "light_layers";
    pub(crate) const RENDER_LAYERS: &str = "render_layers";
    /// A screenshot's file and what stopped it, in a `screenshot_failed` payload.
    pub(crate) const PATH: &str = "path";
    pub(crate) const ERROR: &str = "error";
    pub(crate) const TEXT_ALIGN: &str = "text_align";

    pub(crate) const A: &str = "a";
    /// What a mask drops a pixel below: a text node's key, and the `[surface]` one.
    pub(crate) const ALPHA_CUTOFF: &str = "alpha_cutoff";
    pub(crate) const B: &str = "b";
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
    pub(crate) const BLOOM_KNEE: &str = "bloom_knee";
    pub(crate) const SSR_MAX_STEPS: &str = "ssr_max_steps";
    pub(crate) const SSR_THICKNESS: &str = "ssr_thickness";
    pub(crate) const SSR_MAX_DISTANCE: &str = "ssr_max_distance";
    pub(crate) const SSR_ROUGHNESS_CUTOFF: &str = "ssr_roughness_cutoff";
    pub(crate) const SSR_EDGE_FADE: &str = "ssr_edge_fade";
    pub(crate) const SSR_INTENSITY: &str = "ssr_intensity";
    pub(crate) const DOF_MODE: &str = "dof_mode";
    pub(crate) const DOF_FOCUS_DISTANCE: &str = "dof_focus_distance";
    pub(crate) const DOF_APERTURE_F_STOPS: &str = "dof_aperture_f_stops";
    pub(crate) const DOF_SENSOR_HEIGHT: &str = "dof_sensor_height";
    pub(crate) const DOF_MAX_BLUR_PIXELS: &str = "dof_max_blur_pixels";
    pub(crate) const DOF_MAX_DEPTH: &str = "dof_max_depth";
    pub(crate) const DOF_TAPS: &str = "dof_taps";
    pub(crate) const FXAA_EDGE_THRESHOLD: &str = "fxaa_edge_threshold";
    pub(crate) const FXAA_EDGE_THRESHOLD_MIN: &str = "fxaa_edge_threshold_min";
    pub(crate) const SHARPEN_AMOUNT: &str = "sharpen_amount";
    pub(crate) const CRT_CURVATURE: &str = "crt_curvature";
    pub(crate) const CRT_ABERRATION: &str = "crt_aberration";
    pub(crate) const CRT_SCANLINE_INTENSITY: &str = "crt_scanline_intensity";
    pub(crate) const CRT_SCANLINE_COUNT: &str = "crt_scanline_count";
    pub(crate) const CRT_VIGNETTE: &str = "crt_vignette";
    pub(crate) const EDGES_THRESHOLD: &str = "edges_threshold";
    pub(crate) const GI_RAYS: &str = "gi_rays";
    pub(crate) const GI_MAX_DISTANCE: &str = "gi_max_distance";
    pub(crate) const GI_MAX_STEPS: &str = "gi_max_steps";
    pub(crate) const GI_DOWNSCALE: &str = "gi_downscale";
    pub(crate) const GI_TEMPORAL_BLEND: &str = "gi_temporal_blend";
    pub(crate) const GI_SOLVER: &str = "gi_solver";
    pub(crate) const GI_CASCADE_COUNT: &str = "gi_cascade_count";
    pub(crate) const GI_CASCADE_DIRECTIONS: &str = "gi_cascade_directions";
    pub(crate) const GI_SCREEN_OCCLUDERS: &str = "gi_screen_occluders";
    pub(crate) const GI_PROBE_SPACING: &str = "gi_probe_spacing";
    pub(crate) const LOUPE_ZOOM: &str = "loupe_zoom";
    pub(crate) const LOUPE_FOCUS: &str = "loupe_focus";
    pub(crate) const LOUPE_CORNER: &str = "loupe_corner";
    pub(crate) const LOUPE_SIZE: &str = "loupe_size";
    pub(crate) const LOUPE_BORDER_COLOR: &str = "loupe_border_color";
    pub(crate) const TRANSMISSION_STEPS: &str = "transmission_steps";
    pub(crate) const C: &str = "c";
    pub(crate) const SEED: &str = "seed";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const DEPTH_TEST: &str = "depth_test";
    pub(crate) const DOUBLE_SIDED: &str = "double_sided";
    pub(crate) const FONT: &str = "font";
    pub(crate) const TEXT_STYLE: &str = "text_style";
    pub(crate) const FONT_SIZE: &str = "font_size";
    pub(crate) const FONT_STYLE: &str = "font_style";
    pub(crate) const FONT_WEIGHT: &str = "font_weight";
    pub(crate) const HEIGHT: &str = p::HEIGHT;
    pub(crate) const SPECULAR_STRENGTH: &str = "specular_strength";
    pub(crate) const SHININESS: &str = "shininess";
    pub(crate) const NORMAL_STRENGTH: &str = "normal_strength";
    /// A `draw_text` option; `text2d` spells it `font_style`.
    pub(crate) const MIRROR: &str = "mirror";
    pub(crate) const KIND: &str = p::KIND;
    pub(crate) const LINE_BREAK: &str = "line_break";
    pub(crate) const TRUNCATE: &str = "truncate";
    pub(crate) const TRUNCATE_AT: &str = "truncate_at";
    pub(crate) const MAX_LINES: &str = "max_lines";
    pub(crate) const MAX_HEIGHT: &str = "max_height";
    pub(crate) const TAB_WIDTH: &str = "tab_width";
    pub(crate) const MATERIAL: &str = "material";
    pub(crate) const MARKUP: &str = "markup";
    pub(crate) const MAX_WIDTH: &str = "max_width";
    pub(crate) const MESH: &str = "mesh";
    pub(crate) const PIXELS_PER_UNIT: &str = "pixels_per_unit";
    pub(crate) const PLATE: &str = "plate";
    pub(crate) const POST: &str = "post";
    /// The asset type a camera's `effects` row names.
    pub(crate) const CAMERA_EFFECTS: &str = "camera_effects";
    pub(crate) const RADIUS: &str = p::RADIUS;
    pub(crate) const ORIGIN: &str = "origin";
    pub(crate) const REGION_ORIGIN: &str = "region_origin";
    pub(crate) const REGION_SIZE: &str = "region_size";
    pub(crate) const Z_INDEX: &str = "z_index";
    pub(crate) const SHADOW_RESOLUTION: &str = "shadow_resolution";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const SKELETON: &str = "skeleton";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const TEXTURE: &str = "texture";
    /// A `multimesh` asset's keys, and one instance's: the transform
    /// component's three, a colour and four floats of custom data.
    pub(crate) const INSTANCES: &str = "instances";
    pub(crate) const VISIBLE_INSTANCE_COUNT: &str = "visible_instance_count";
    pub(crate) const POSITION: &str = "position";
    pub(crate) const ROTATION_EULER: &str = "rotation_euler";
    pub(crate) const SCALE: &str = "scale";
    pub(crate) const CUSTOM: &str = "custom";
    /// `populate`'s options.
    pub(crate) const COUNT: &str = "count";
    pub(crate) const COUNTS: &str = "counts";
    pub(crate) const STEP: &str = "step";
    pub(crate) const ROTATION: &str = "rotation";
    pub(crate) const TILT: &str = "tilt";
    pub(crate) const RANDOM_SCALE: &str = "random_scale";
    pub(crate) const SURFACE: &str = "surface";
    /// What every drawn node shows beside its surface, and the node's own
    /// flags: the renderable keys of both dimensions.
    pub(crate) const WIREFRAME_WIDTH: &str = "wireframe_width";
    pub(crate) const WIREFRAME_COLOR: &str = "wireframe_color";
    pub(crate) const DOT_SIZE: &str = "dot_size";
    pub(crate) const DOT_COLOR: &str = "dot_color";
    pub(crate) const SEGMENTATION_ID: &str = "segmentation_id";
    pub(crate) const RECEIVE_SHADOWS: &str = "receive_shadows";
    /// An instance's 3x3, columns first, shear and all.
    pub(crate) const BASIS: &str = "basis";
    pub(crate) const SHADOW_VIEWS: &str = "shadow_views";
    pub(crate) const PROBE_CAPTURE_SIZE_PIXELS: &str = "probe_capture_size_pixels";
    pub(crate) const CLUSTER_GRID: &str = "cluster_grid";
    pub(crate) const CLUSTER_MAX_LIGHTS: &str = "cluster_max_lights";
    /// The cameras' steps, height, bloom chain and zoom limits.
    pub(crate) const BLOOM_MIPS: &str = "bloom_mips";
    /// A material's `[surface]` table.
    pub(crate) const ALPHA: &str = "alpha";
    pub(crate) const TRANSMISSION: &str = "transmission";
    pub(crate) const IOR: &str = "ior";
    pub(crate) const THICKNESS: &str = "thickness";
    pub(crate) const ATTENUATION_COLOR: &str = "attenuation_color";
    pub(crate) const ATTENUATION_DISTANCE: &str = "attenuation_distance";
    pub(crate) const MIRROR_INTENSITY: &str = "mirror_intensity";
    pub(crate) const MIRROR_FALLOFF: &str = "mirror_falloff";
    pub(crate) const MIRROR_NORMAL: &str = "mirror_normal";
    pub(crate) const MIRROR_RESOLUTION_SCALE: &str = "mirror_resolution_scale";
    pub(crate) const MIRROR_RENDER_LAYERS: &str = "mirror_render_layers";
    pub(crate) const TRACE_SURFACE: &str = "trace_surface";
    pub(crate) const SSR: &str = "ssr";
    pub(crate) const SSR_INFINITE_THICKNESS: &str = "ssr_infinite_thickness";
    pub(crate) const SSR_DISTANCE_FADE: &str = "ssr_distance_fade";
    pub(crate) const SSR_FRESNEL: &str = "ssr_fresnel";
    /// What a material with no `shader` sets on kiss3d's own material.
    pub(crate) const VIEW: &str = "view";
    pub(crate) const METALLIC: &str = "metallic";
    pub(crate) const ROUGHNESS: &str = "roughness";
    pub(crate) const EMISSION_COLOR: &str = "emission_color";
    pub(crate) const SPECULAR_TINT: &str = "specular_tint";
    pub(crate) const REFLECTANCE: &str = "reflectance";
    pub(crate) const CLEARCOAT: &str = "clearcoat";
    pub(crate) const CLEARCOAT_ROUGHNESS: &str = "clearcoat_roughness";
    pub(crate) const ANISOTROPY: &str = "anisotropy";
    pub(crate) const ANISOTROPY_ROTATION_DEGREES: &str = "anisotropy_rotation_degrees";
    pub(crate) const SUBSURFACE: &str = "subsurface";
    pub(crate) const SUBSURFACE_RADIUS: &str = "subsurface_radius";
    pub(crate) const PARALLAX_SCALE: &str = "parallax_scale";
    pub(crate) const PARALLAX_LAYERS: &str = "parallax_layers";
    pub(crate) const PARALLAX_METHOD: &str = "parallax_method";
    pub(crate) const PARALLAX_RELIEF_STEPS: &str = "parallax_relief_steps";
    pub(crate) const SHADER: &str = "shader";
    pub(crate) const TILEMAP: &str = "tilemap";
}

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
