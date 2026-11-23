//! Custom shaders written by the app in WGSL and painted like quads.
//!
//! An app registers a [`ShaderSource`] once and gets a [`Shader`] back, a cheap handle it paints
//! with through [`Window::paint_shader`](crate::Window::paint_shader). The source only defines
//! `fn shade(input: ShaderInput) -> vec4<f32>`, which returns a straight alpha sRGB colour for
//! one pixel. GPUI concatenates it with a prelude, [`SHADER_PRELUDE`], that supplies the vertex
//! stage, the [`ShaderInput`](SHADER_PRELUDE) struct and a set of helpers, and multiplies the
//! result by the rounded rectangle's antialiased coverage and the element's opacity.
//!
//! The same WGSL runs on every backend. The wgpu renderer compiles it directly, while Metal and
//! DirectX translate it with naga, which is also what validates it at registration.

use crate::{Hsla, Rgba, SharedString};
use collections::FxHashMap;
use parking_lot::RwLock;
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, LazyLock},
};

/// The WGSL every custom shader is appended to. It declares the bindings, the vertex and
/// fragment entry points, `ShaderInput` and the helpers a shader may call.
pub const SHADER_PRELUDE: &str = include_str!("shader/prelude.wgsl");

/// The WGSL that reads instances from a storage buffer. A backend without storage buffers
/// supplies its own `gpui_load_shader_instance` in its place.
pub const SHADER_STORAGE_TRANSPORT: &str = include_str!("shader/storage.wgsl");

/// The entry point of the vertex stage every custom shader shares.
pub const SHADER_VERTEX_ENTRY: &str = "gpui_shader_vertex";

/// The entry point of the fragment stage that calls the shader's `shade`.
pub const SHADER_FRAGMENT_ENTRY: &str = "gpui_shader_fragment";

/// How many floats a shader instance carries in [`ShaderInstance::params`].
pub const SHADER_PARAM_COUNT: usize = 16;

/// How many colours a shader instance carries in [`ShaderInstance::colors`].
pub const SHADER_COLOR_COUNT: usize = 4;

/// The WGSL source of a custom shader.
///
/// The source must define `fn shade(input: ShaderInput) -> vec4<f32>`. It may define any other
/// functions and constants, but no bindings, and no names the prelude already uses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShaderSource {
    /// A name for the shader in errors and GPU debuggers.
    pub label: SharedString,
    /// The WGSL that defines `shade`.
    pub wgsl: SharedString,
}

impl ShaderSource {
    /// A shader source from its label and WGSL.
    pub fn new(label: impl Into<SharedString>, wgsl: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            wgsl: wgsl.into(),
        }
    }
}

/// The number a registered shader is known by. It is unique within the process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ShaderId(pub u32);

/// A registered custom shader, ready to paint with. Copying it is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shader {
    id: ShaderId,
}

impl Shader {
    /// Validates the source and registers it. Registering the same source again returns the
    /// shader it was registered as the first time without validating it again, so this is
    /// cheap to call while rendering.
    pub fn new(source: ShaderSource) -> Result<Self, ShaderError> {
        let id = REGISTRY.register(source)?;
        Ok(Self { id })
    }

    /// The id the renderers know this shader by.
    pub fn id(&self) -> ShaderId {
        self.id
    }

    /// The label the shader was registered with.
    pub fn label(&self) -> SharedString {
        shader_program(self.id)
            .map(|program| program.label.clone())
            .unwrap_or_default()
    }
}

/// Why a shader failed to register: its WGSL did not parse, or did not validate against the
/// prelude.
#[derive(Clone, Debug, thiserror::Error)]
#[error("shader `{label}` is invalid:\n{message}")]
pub struct ShaderError {
    /// The label of the shader that failed.
    pub label: SharedString,
    /// What naga reported, with line numbers counted in the source the prelude was added to.
    pub message: String,
}

/// What one painted instance of a shader is given beyond its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderInstance {
    /// A time in seconds the shader animates by. The app supplies it, so a frozen value gives a
    /// still frame, and painting a shader never schedules another frame by itself.
    pub time: f32,
    /// Sixteen numbers whose meaning the shader defines. The shader reads them as
    /// `input.params[i / 4][i % 4]`.
    pub params: [f32; SHADER_PARAM_COUNT],
    /// Four colours whose meaning the shader defines. The shader reads them as straight alpha
    /// sRGB in `input.colors`.
    pub colors: [Hsla; SHADER_COLOR_COUNT],
}

impl Default for ShaderInstance {
    fn default() -> Self {
        Self {
            time: 0.,
            params: [0.; SHADER_PARAM_COUNT],
            colors: [Hsla::transparent_black(); SHADER_COLOR_COUNT],
        }
    }
}

impl ShaderInstance {
    /// An instance at the given time with every parameter zero and every colour transparent.
    pub fn at(time: f32) -> Self {
        Self {
            time,
            ..Self::default()
        }
    }

    /// Sets parameter `index`, which must be below [`SHADER_PARAM_COUNT`].
    pub fn param(mut self, index: usize, value: f32) -> Self {
        self.params[index] = value;
        self
    }

    /// Sets the parameters from `start` on to `values`.
    pub fn params(mut self, start: usize, values: &[f32]) -> Self {
        self.params[start..start + values.len()].copy_from_slice(values);
        self
    }

    /// Sets colour `index`, which must be below [`SHADER_COLOR_COUNT`].
    pub fn color(mut self, index: usize, color: impl Into<Hsla>) -> Self {
        self.colors[index] = color.into();
        self
    }

    /// The colours as straight alpha sRGB components, faded by `opacity`, the way the shader
    /// reads them.
    pub(crate) fn rgba_colors(&self) -> [[f32; 4]; SHADER_COLOR_COUNT] {
        self.colors.map(|color| {
            let Rgba { r, g, b, a } = color.into();
            [r, g, b, a]
        })
    }
}

/// A registered shader as the renderers see it.
#[derive(Debug)]
pub struct ShaderProgram {
    /// The shader's id.
    pub id: ShaderId,
    /// The label it was registered with.
    pub label: SharedString,
    /// The WGSL it was registered with, without the prelude.
    pub wgsl: SharedString,
}

impl ShaderProgram {
    /// The full WGSL module: the prelude, the given instance transport and the shader itself.
    pub fn compose(&self, transport: &str) -> String {
        compose(transport, &self.wgsl)
    }

    /// Parses and validates the module with the storage buffer transport.
    pub fn module(&self) -> Result<(naga::Module, naga::valid::ModuleInfo), ShaderError> {
        parse(&self.label, &self.compose(SHADER_STORAGE_TRANSPORT))
    }

    /// Translates the shader to the Metal Shading Language. The globals are bound at buffer
    /// [`MslShader::GLOBALS_BUFFER`] and the instances at [`MslShader::INSTANCES_BUFFER`] in
    /// both stages.
    #[cfg(any(target_os = "macos", target_os = "ios", test))]
    pub fn to_msl(&self) -> Result<MslShader, ShaderError> {
        use naga::back::msl;

        let (module, info) = self.module()?;
        let resources = msl::EntryPointResources {
            resources: [
                (
                    naga::ResourceBinding {
                        group: 0,
                        binding: 0,
                    },
                    msl::BindTarget {
                        buffer: Some(MslShader::GLOBALS_BUFFER as u8),
                        ..Default::default()
                    },
                ),
                (
                    naga::ResourceBinding {
                        group: 1,
                        binding: 0,
                    },
                    msl::BindTarget {
                        buffer: Some(MslShader::INSTANCES_BUFFER as u8),
                        ..Default::default()
                    },
                ),
            ]
            .into_iter()
            .collect(),
            immediates_buffer: None,
            sizes_buffer: Some(MslShader::SIZES_BUFFER as u8),
        };
        let options = msl::Options {
            lang_version: (2, 1),
            per_entry_point_map: [SHADER_VERTEX_ENTRY, SHADER_FRAGMENT_ENTRY]
                .into_iter()
                .map(|entry| (entry.to_string(), resources.clone()))
                .collect(),
            fake_missing_bindings: false,
            ..Default::default()
        };
        let pipeline = msl::PipelineOptions {
            vertex_pulling_transform: false,
            ..Default::default()
        };
        let (source, translation) = msl::write_string(&module, &info, &options, &pipeline)
            .map_err(|error| self.error(format!("translating to MSL failed: {error}")))?;
        let (vertex_entry, fragment_entry) =
            self.entry_names(&module, translation.entry_point_names, "MSL")?;
        Ok(MslShader {
            source,
            vertex_entry,
            fragment_entry,
        })
    }

    /// Translates the shader to HLSL for shader model 5.0. The globals are bound at constant
    /// buffer [`HlslShader::GLOBALS_REGISTER`], the batch's first instance at
    /// [`HlslShader::CONSTANTS_REGISTER`] and the instances, as a raw buffer, at texture register
    /// [`HlslShader::INSTANCES_REGISTER`]. They sit past the registers the built in shaders use,
    /// so drawing a custom shader leaves those bindings alone.
    #[cfg(any(target_os = "windows", test))]
    pub fn to_hlsl(&self) -> Result<HlslShader, ShaderError> {
        use naga::back::hlsl;

        let (module, info) = self.module()?;
        let target = |register| hlsl::BindTarget {
            space: 0,
            register,
            ..Default::default()
        };
        let options = hlsl::Options {
            shader_model: hlsl::ShaderModel::V5_0,
            binding_map: [
                (
                    naga::ResourceBinding {
                        group: 0,
                        binding: 0,
                    },
                    target(HlslShader::GLOBALS_REGISTER),
                ),
                (
                    naga::ResourceBinding {
                        group: 1,
                        binding: 0,
                    },
                    target(HlslShader::INSTANCES_REGISTER),
                ),
            ]
            .into_iter()
            .collect(),
            fake_missing_bindings: false,
            special_constants_binding: Some(target(HlslShader::CONSTANTS_REGISTER)),
            ..Default::default()
        };
        let mut source = String::new();
        let pipeline = hlsl::PipelineOptions::default();
        let mut writer = hlsl::Writer::new(&mut source, &options, &pipeline);
        let reflection = writer
            .write(&module, &info, None)
            .map_err(|error| self.error(format!("translating to HLSL failed: {error}")))?;
        let (vertex_entry, fragment_entry) =
            self.entry_names(&module, reflection.entry_point_names, "HLSL")?;
        Ok(HlslShader {
            source: constant_buffers_for_shader_model_5(&source),
            vertex_entry,
            fragment_entry,
        })
    }

    /// The names the vertex and fragment entry points were written under, which a backend may
    /// change to stay clear of its reserved words.
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows", test))]
    fn entry_names<E: std::fmt::Display>(
        &self,
        module: &naga::Module,
        names: Vec<Result<String, E>>,
        language: &str,
    ) -> Result<(String, String), ShaderError> {
        let mut vertex = None;
        let mut fragment = None;
        for (entry_point, name) in module.entry_points.iter().zip(names) {
            let name = name.map_err(|error| {
                self.error(format!(
                    "translating `{}` to {language} failed: {error}",
                    entry_point.name
                ))
            })?;
            match entry_point.name.as_str() {
                SHADER_VERTEX_ENTRY => vertex = Some(name),
                SHADER_FRAGMENT_ENTRY => fragment = Some(name),
                _ => {}
            }
        }
        match (vertex, fragment) {
            (Some(vertex), Some(fragment)) => Ok((vertex, fragment)),
            _ => Err(self.error(format!("the {language} output lost an entry point"))),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows", test))]
    fn error(&self, message: String) -> ShaderError {
        ShaderError {
            label: self.label.clone(),
            message,
        }
    }
}

/// A custom shader translated to the Metal Shading Language.
#[cfg(any(target_os = "macos", target_os = "ios", test))]
#[derive(Clone, Debug)]
pub struct MslShader {
    /// The MSL source of both stages.
    pub source: String,
    /// The name of the vertex function.
    pub vertex_entry: String,
    /// The name of the fragment function.
    pub fragment_entry: String,
}

#[cfg(any(target_os = "macos", target_os = "ios", test))]
impl MslShader {
    /// The buffer index the globals are bound at in both stages.
    pub const GLOBALS_BUFFER: u64 = 0;
    /// The buffer index the instances are bound at in both stages.
    pub const INSTANCES_BUFFER: u64 = 1;
    /// The buffer index naga reads the byte length of the instance buffer from, one `u32`.
    pub const SIZES_BUFFER: u64 = 2;
}

/// A custom shader translated to HLSL.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug)]
pub struct HlslShader {
    /// The HLSL source of both stages.
    pub source: String,
    /// The name of the vertex function, compiled for `vs_5_0`.
    pub vertex_entry: String,
    /// The name of the fragment function, compiled for `ps_5_0`.
    pub fragment_entry: String,
}

#[cfg(any(target_os = "windows", test))]
impl HlslShader {
    /// The constant buffer register the globals are read from.
    pub const GLOBALS_REGISTER: u32 = 2;
    /// The constant buffer register naga reads the batch's first vertex and instance from, as
    /// `int first_vertex, first_instance` followed by a word of padding.
    pub const CONSTANTS_REGISTER: u32 = 3;
    /// The texture register the raw instance buffer is read from.
    pub const INSTANCES_REGISTER: u32 = 2;
}

/// Rewrites the `ConstantBuffer<T>` declarations naga emits, which need shader model 5.1, as
/// `cbuffer` blocks that shader model 5.0 compiles to the same layout.
#[cfg(any(target_os = "windows", test))]
fn constant_buffers_for_shader_model_5(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        let declaration = line
            .strip_prefix("ConstantBuffer<")
            .and_then(|rest| rest.split_once("> "))
            .and_then(|(ty, rest)| {
                let (name, register) = rest.split_once(":")?;
                Some((ty, name.trim(), register.trim().strip_suffix(";")?))
            });
        match declaration {
            Some((ty, name, register)) => {
                output.push_str(&format!(
                    "cbuffer {name}_block : {register} {{ {ty} {name}; }}"
                ));
            }
            None => output.push_str(line),
        }
        output.push('\n');
    }
    output
}

/// The program registered under `id`, if any.
pub fn shader_program(id: ShaderId) -> Option<Arc<ShaderProgram>> {
    REGISTRY.programs.read().get(id.0 as usize).cloned()
}

fn compose(transport: &str, wgsl: &str) -> String {
    let mut source = String::with_capacity(SHADER_PRELUDE.len() + transport.len() + wgsl.len() + 2);
    source.push_str(SHADER_PRELUDE);
    source.push('\n');
    source.push_str(transport);
    source.push('\n');
    source.push_str(wgsl);
    source
}

fn parse(
    label: &SharedString,
    source: &str,
) -> Result<(naga::Module, naga::valid::ModuleInfo), ShaderError> {
    let module = naga::front::wgsl::parse_str(source).map_err(|error| ShaderError {
        label: label.clone(),
        message: error.emit_to_string_with_path(source, label),
    })?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| ShaderError {
        label: label.clone(),
        message: error.emit_to_string_with_path(source, label),
    })?;
    Ok((module, info))
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::default);

/// Every shader the process has registered, indexed by id. Shaders are never unregistered, so
/// an id stays valid for the life of the process.
#[derive(Default)]
struct Registry {
    programs: RwLock<Vec<Arc<ShaderProgram>>>,
    by_source: RwLock<FxHashMap<u64, Vec<ShaderId>>>,
}

impl Registry {
    fn register(&self, source: ShaderSource) -> Result<ShaderId, ShaderError> {
        let key = {
            let mut hasher = collections::FxHasher::default();
            source.wgsl.hash(&mut hasher);
            hasher.finish()
        };
        if let Some(id) = self.find(key, &source.wgsl) {
            return Ok(id);
        }

        let composed = compose(SHADER_STORAGE_TRANSPORT, &source.wgsl);
        if let Err(error) = parse(&source.label, &composed) {
            log::error!("{error}");
            return Err(error);
        }

        let mut by_source = self.by_source.write();
        // Another thread may have registered the same source while this one validated it.
        if let Some(id) = self.find_locked(&by_source, key, &source.wgsl) {
            return Ok(id);
        }
        let mut programs = self.programs.write();
        let id = ShaderId(programs.len() as u32);
        programs.push(Arc::new(ShaderProgram {
            id,
            label: source.label,
            wgsl: source.wgsl,
        }));
        by_source.entry(key).or_default().push(id);
        Ok(id)
    }

    fn find(&self, key: u64, wgsl: &str) -> Option<ShaderId> {
        self.find_locked(&self.by_source.read(), key, wgsl)
    }

    fn find_locked(
        &self,
        by_source: &FxHashMap<u64, Vec<ShaderId>>,
        key: u64,
        wgsl: &str,
    ) -> Option<ShaderId> {
        let programs = self.programs.read();
        by_source.get(&key)?.iter().copied().find(|id| {
            programs
                .get(id.0 as usize)
                .is_some_and(|program| program.wgsl.as_ref() == wgsl)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRADIENT: &str = r#"
        fn shade(input: ShaderInput) -> vec4<f32> {
            let tone = bayer(vec2<u32>(input.frag_coord), 3u) * 0.1 + value_noise(input.uv * 4.0);
            let color = mix_oklab(input.colors[0].rgb, input.colors[1].rgb, input.uv.x + tone);
            return vec4<f32>(dither(color, input.frag_coord), 1.0);
        }
    "#;

    #[test]
    fn registering_the_same_source_twice_returns_one_shader() {
        // The registry is shared by every test in the process, so a source another test also
        // registers could come back under that test's label.
        let source = format!("{GRADIENT}// only registered here\n");
        let first = Shader::new(ShaderSource::new("gradient", source.clone())).unwrap();
        let second = Shader::new(ShaderSource::new("again", source)).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.label(), "gradient");
    }

    #[test]
    fn invalid_sources_are_reported_not_registered() {
        let missing = Shader::new(ShaderSource::new("missing", "fn other() {}")).unwrap_err();
        assert_eq!(missing.label, "missing");
        assert!(missing.message.contains("shade"), "{}", missing.message);

        let mistyped = Shader::new(ShaderSource::new(
            "mistyped",
            "fn shade(input: ShaderInput) -> vec4<f32> { return input.time; }",
        ))
        .unwrap_err();
        assert!(!mistyped.message.is_empty());
    }

    #[test]
    fn shaders_translate_to_msl_and_hlsl() {
        let shader = Shader::new(ShaderSource::new("translated", GRADIENT)).unwrap();
        let program = shader_program(shader.id()).unwrap();

        let msl = program.to_msl().unwrap();
        assert!(msl.source.contains(&msl.vertex_entry));
        assert!(msl.source.contains(&msl.fragment_entry));
        if let Ok(path) = std::env::var("GPUI_DUMP_SHADER_TRANSLATIONS") {
            std::fs::write(format!("{path}.metal"), &msl.source).unwrap();
        }

        let hlsl = program.to_hlsl().unwrap();
        assert!(hlsl.source.contains(&hlsl.vertex_entry));
        assert!(hlsl.source.contains(&hlsl.fragment_entry));
        assert!(hlsl.source.contains("register(t2)"));
        assert!(hlsl.source.contains("register(b3)"));
        assert!(!hlsl.source.contains("ConstantBuffer<"));
        if let Ok(path) = std::env::var("GPUI_DUMP_SHADER_TRANSLATIONS") {
            std::fs::write(format!("{path}.hlsl"), &hlsl.source).unwrap();
        }
    }

    #[test]
    fn instance_layout_matches_the_prelude() {
        use crate::ShaderQuad;
        assert_eq!(std::mem::size_of::<ShaderQuad>(), 208);
        assert_eq!(std::mem::offset_of!(ShaderQuad, bounds), 16);
        assert_eq!(std::mem::offset_of!(ShaderQuad, content_mask), 32);
        assert_eq!(std::mem::offset_of!(ShaderQuad, corner_radii), 48);
        assert_eq!(std::mem::offset_of!(ShaderQuad, scale_factor), 64);
        assert_eq!(std::mem::offset_of!(ShaderQuad, params), 80);
        assert_eq!(std::mem::offset_of!(ShaderQuad, colors), 144);
    }
}
