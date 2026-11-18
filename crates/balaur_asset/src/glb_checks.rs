//! What a glTF file must hold before the gltf crate reads it. The crate
//! trusts what its own validation never checks: a GLB's declared length, the
//! accessor a primitive's positions name, each accessor's type, stride and
//! extent, and a channel's target. Reading any of those unchecked panics.

use anyhow::{Context, Result, bail};
use gltf::Semantic;
use gltf::accessor::{DataType, Dimensions};
use gltf::animation::Property;
use gltf::json::validation::Checked;

/// A GLB's 12-byte header: magic, version, and the length of the whole file.
const GLB_HEADER: u32 = 12;

// The component types glTF 2.0 allows each use of an accessor.
const FLOAT: &[DataType] = &[DataType::F32];
const UNSIGNED: &[DataType] = &[DataType::F32, DataType::U8, DataType::U16];
const SIGNED: &[DataType] = &[
    DataType::F32,
    DataType::I8,
    DataType::U8,
    DataType::I16,
    DataType::U16,
];
const INDEX: &[DataType] = &[DataType::U8, DataType::U16, DataType::U32];
const JOINT: &[DataType] = &[DataType::U8, DataType::U16];

/// The document and the GLB's binary chunk, or an error naming what the file
/// gets wrong that gltf would have panicked on.
pub(crate) fn read_gltf(bytes: &[u8], name: &str) -> Result<(gltf::Document, Option<Vec<u8>>)> {
    let declared = bytes
        .get(8..12)
        .and_then(|l| <[u8; 4]>::try_from(l).ok())
        .map(u32::from_le_bytes);
    if bytes.starts_with(b"glTF") && declared.is_some_and(|len| len < GLB_HEADER) {
        bail!("{name} declares a length shorter than its own header");
    }
    let file = gltf::Gltf::from_slice_without_validation(bytes)
        .with_context(|| format!("reading {name}"))?;
    let json = file.document.into_json();
    // Read by gltf's validation before it checks the index.
    let positions = Checked::Valid(Semantic::Positions);
    let missing = json
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.primitives)
        .filter_map(|primitive| primitive.attributes.get(&positions))
        .any(|accessor| accessor.value() >= json.accessors.len());
    if missing {
        bail!("{name} names a position accessor it has not got");
    }
    let document = gltf::Document::from_json(json).with_context(|| format!("reading {name}"))?;
    accessors_fit(&document).with_context(|| format!("reading {name}"))?;
    Ok((document, file.blob))
}

/// Whether a channel's target names a node the file has and a property glTF
/// defines. One that does not, an extension's own path say, is read past.
pub(crate) fn target_reads(
    document: &gltf::Document,
    channel: &gltf::animation::Channel<'_>,
) -> bool {
    let root = document.as_json();
    root.animations
        .get(channel.animation().index())
        .and_then(|a| a.channels.get(channel.index()))
        .is_some_and(|c| {
            matches!(c.target.path, Checked::Valid(_)) && c.target.node.value() < root.nodes.len()
        })
}

/// Every accessor the reader reads, against what its use allows.
fn accessors_fit(document: &gltf::Document) -> Result<()> {
    use Dimensions::{Mat4, Scalar, Vec2, Vec3, Vec4};
    for primitive in document.meshes().flat_map(|mesh| mesh.primitives()) {
        for (semantic, accessor) in primitive.attributes() {
            let (dimensions, types): (&[Dimensions], _) = match semantic {
                Semantic::Positions | Semantic::Normals => (&[Vec3], FLOAT),
                Semantic::Tangents => (&[Vec4], FLOAT),
                Semantic::TexCoords(_) => (&[Vec2], UNSIGNED),
                Semantic::Colors(_) => (&[Vec3, Vec4], UNSIGNED),
                Semantic::Joints(_) => (&[Vec4], JOINT),
                Semantic::Weights(_) => (&[Vec4], UNSIGNED),
                Semantic::Extras(_) => continue,
            };
            fits(&accessor, dimensions, types)?;
        }
        if let Some(indices) = primitive.indices() {
            fits(&indices, &[Scalar], INDEX)?;
        }
        for target in primitive.morph_targets() {
            for accessor in [target.positions(), target.normals(), target.tangents()]
                .into_iter()
                .flatten()
            {
                fits(&accessor, &[Vec3], FLOAT)?;
            }
        }
    }
    for matrices in document
        .skins()
        .filter_map(|skin| skin.inverse_bind_matrices())
    {
        fits(&matrices, &[Mat4], FLOAT)?;
    }
    for animation in document.animations() {
        for sampler in animation.samplers() {
            fits(&sampler.input(), &[Scalar], FLOAT)?;
        }
        for channel in animation.channels() {
            if !target_reads(document, &channel) {
                continue;
            }
            let (dimensions, types): (&[Dimensions], _) = match channel.target().property() {
                Property::Translation | Property::Scale => (&[Vec3], FLOAT),
                Property::Rotation => (&[Vec4], SIGNED),
                Property::MorphTargetWeights => (&[Scalar], SIGNED),
            };
            fits(&channel.sampler().output(), dimensions, types)?;
        }
    }
    Ok(())
}

fn fits(
    accessor: &gltf::Accessor<'_>,
    dimensions: &[Dimensions],
    types: &[DataType],
) -> Result<()> {
    let index = accessor.index();
    let (kind, dims) = (accessor.data_type(), accessor.dimensions());
    if !dimensions.contains(&dims) || !types.contains(&kind) {
        bail!("accessor {index} holds {kind:?} {dims:?}, which its use does not take");
    }
    let size = accessor.size();
    let spans = |view: &gltf::buffer::View<'_>, offset, count, size| {
        spans(view, offset, count, size).with_context(|| format!("accessor {index}"))
    };
    if let Some(view) = accessor.view() {
        spans(&view, accessor.offset(), accessor.count(), size)?;
    }
    if let Some(sparse) = accessor.sparse() {
        let (indices, values) = (sparse.indices(), sparse.values());
        spans(
            &indices.view(),
            indices.offset(),
            sparse.count(),
            indices.index_type().size(),
        )?;
        spans(&values.view(), values.offset(), sparse.count(), size)?;
    }
    Ok(())
}

/// `count` elements of `size` bytes from `offset` inside `view`, a stride
/// apart that holds a whole one, and the view inside its buffer: the sums the
/// reader does unchecked.
fn spans(view: &gltf::buffer::View<'_>, offset: usize, count: usize, size: usize) -> Result<()> {
    let stride = view.stride().unwrap_or(size);
    let end = count
        .checked_sub(1)
        .and_then(|n| n.checked_mul(stride))
        .and_then(|n| n.checked_add(offset))
        .and_then(|n| n.checked_add(size));
    let in_buffer = view
        .offset()
        .checked_add(view.length())
        .is_some_and(|end| end <= view.buffer().length());
    if stride < size || end.is_none_or(|end| end > view.length()) || !in_buffer {
        bail!(
            "buffer view {} cannot hold {count} of {size} bytes from byte {offset}",
            view.index()
        );
    }
    Ok(())
}
