use allocative::Allocative;
use starlark::{
    any::ProvidesStaticType,
    environment::GlobalsBuilder,
    eval::{Arguments, Evaluator, ParametersSpec, ParametersSpecParam},
    starlark_module, starlark_simple_value,
    values::{starlark_value, Freeze, Heap, NoSerialize, StarlarkValue, Trace, Value},
};

use crate::{lang::evaluator_ext::EvaluatorExt, EvalContext};

use anyhow::anyhow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphicError {
    #[error("'{name}' must be a string")]
    ParameterNotString { name: &'static str },
    #[error("graphic must be a DXF")]
    SourceNotDxf,
}

impl From<GraphicError> for starlark::Error {
    fn from(err: GraphicError) -> Self {
        starlark::Error::new_other(err)
    }
}

/// Graphic represents a graphical element on one or more board layers.
#[derive(Clone, Trace, ProvidesStaticType, NoSerialize, Allocative, Freeze)]
#[repr(C)]
pub struct GraphicValue {
    name: String,
    source_path: String,
    graphic_path: String,
    layer: String,
}

impl GraphicValue {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source_path(&self) -> &str {
        &self.source_path
    }

    pub fn graphic_path(&self) -> &str {
        &self.graphic_path
    }

    pub fn layer(&self) -> &str {
        &self.layer
    }
}

impl std::fmt::Debug for GraphicValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("Graphic");
        debug.field("name", &self.name);
        debug.field("path", &self.graphic_path);
        debug.field("layer", &self.layer);
        debug.finish()
    }
}

starlark_simple_value!(GraphicValue);

#[starlark_value(type = "Graphic")]
impl<'v> StarlarkValue<'v> for GraphicValue
where
    Self: ProvidesStaticType<'v>,
{
    fn get_attr(&self, attr: &str, heap: &'v Heap) -> Option<Value<'v>> {
        match attr {
            "name" => Some(heap.alloc_str(&self.name).to_value()),
            "path" => Some(heap.alloc_str(&self.graphic_path).to_value()),
            "layer" => Some(heap.alloc_str(&self.layer).to_value()),
            _ => None,
        }
    }

    fn has_attr(&self, attr: &str, _heap: &'v Heap) -> bool {
        matches!(attr, "name" | "path" | "layer")
    }

    fn dir_attr(&self) -> Vec<String> {
        vec![
            String::from("name"),
            String::from("path"),
            String::from("layer"),
        ]
    }
}

impl std::fmt::Display for GraphicValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Graphic {{ name: \"{}\", path: \"{}\", layer: \"{}\" }}",
            &self.name, &self.graphic_path, &self.layer
        )
    }
}

pub type FrozenGraphicValue = <GraphicValue as starlark::values::Freeze>::Frozen;

/// GraphicType is a factory for creating Graphic values.
#[derive(Debug, Trace, ProvidesStaticType, NoSerialize, Allocative, Freeze)]
#[repr(C)]
pub struct GraphicType;

starlark_simple_value!(GraphicType);

impl std::fmt::Display for GraphicType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<Graphic>")
    }
}

#[starlark_value(type = "Graphic")]
impl<'v> StarlarkValue<'v> for GraphicType
where
    Self: ProvidesStaticType<'v>,
{
    fn invoke(
        &self,
        _me: Value<'v>,
        args: &Arguments<'v, '_>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<Value<'v>> {
        let param_spec: ParametersSpec<Value<'_>> = ParametersSpec::new_named_only(
            "Graphic",
            [
                ("name", ParametersSpecParam::<Value<'_>>::Required),
                ("path", ParametersSpecParam::<Value<'_>>::Required),
                ("layer", ParametersSpecParam::<Value<'_>>::Required),
            ],
        );

        let (name, source_path, graphic_path, layer) =
            param_spec.parser(args, eval, |param_parser, eval_ctx| {
                let name = param_parser
                    .next::<Value>()?
                    .unpack_str()
                    .ok_or(GraphicError::ParameterNotString { name: "name" })?
                    .to_owned();

                let file_spec = param_parser
                    .next::<Value>()?
                    .unpack_str()
                    .ok_or(GraphicError::ParameterNotString { name: "path" })?
                    .to_owned();

                if !file_spec.ends_with(".dxf") {
                    return Err(starlark::Error::new_other(GraphicError::SourceNotDxf));
                }

                let (source_path, graphic_path) =
                    resolve_graphic_paths(file_spec, eval_ctx.eval_context().unwrap())?;

                let layer = param_parser
                    .next::<Value>()?
                    .unpack_str()
                    .ok_or(GraphicError::ParameterNotString { name: "layer" })?
                    .to_owned();

                Ok((name, source_path, graphic_path, layer))
            })?;

        // The parameters parsed succesful and resolved to a source path.
        // Allocate an object on the heap and add it to the current module.
        let graphic = eval.heap().alloc_complex(GraphicValue {
            name,
            source_path,
            graphic_path,
            layer,
        });

        if let Some(mut module) = eval.module_value_mut() {
            module.add_child(graphic);
        }

        Ok(graphic)
    }

    fn eval_type(&self) -> Option<starlark::typing::Ty> {
        Some(<GraphicType as StarlarkValue>::get_type_starlark_repr())
    }
}

#[starlark_module]
pub fn graphics_globals(builder: &mut GlobalsBuilder) {
    const Graphic: GraphicType = GraphicType;
}

/// Resolve the source and graphic paths for a given graphic specification.
fn resolve_graphic_paths(
    graphic_spec: String,
    eval_ctx: &EvalContext,
) -> Result<(String, String), starlark::Error> {
    let source_path = std::path::Path::new(
        eval_ctx
            .source_path
            .as_ref()
            .ok_or_else(|| starlark::Error::new_other(anyhow!("No source path available")))?,
    );

    let graphic_path = eval_ctx
        .get_load_resolver()
        .resolve_path(&graphic_spec, &source_path)
        .map_err(|e| {
            starlark::Error::new_other(anyhow!("Failed to resolve graphic path: {}", e))
        })?;

    let source_path = String::from(source_path.to_str().ok_or_else(|| {
        starlark::Error::new_other(anyhow!("Source path contains invalid UTF-8 characters"))
    })?);

    // Get the absolute path using the file provider.
    let absolute_graphic_path = eval_ctx
        .file_provider()
        .canonicalize(&graphic_path)
        .unwrap_or(graphic_path.clone())
        .to_string_lossy()
        .into_owned();

    Ok((source_path, absolute_graphic_path))
}
