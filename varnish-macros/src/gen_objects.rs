//! Code to generate the Varnish VMOD code for the object and its methods.

use std::fmt::Write;

use serde_json::{json, Value};

use crate::gen_func::FuncProcessor;
use crate::model::{ObjInfo, SharedTypes};
use crate::names::Names;

#[derive(Debug, Default)]
pub struct ObjProcessor {
    names: Names,

    /// `struct vmod_foo_bar;` C code
    pub cproto_typedef_decl: String,

    /// One JSON `$OBJ` blob per constructor (VCL type name = constructor function name)
    pub json_objs: Vec<Value>,
    pub funcs: Vec<FuncProcessor>,
}

impl ObjProcessor {
    pub fn from_info(names: Names, info: &ObjInfo, types: &SharedTypes) -> Self {
        let constructor_fps: Vec<FuncProcessor> = info
            .constructors
            .iter()
            .map(|f| {
                FuncProcessor::from_info(names.to_func(f.func_type, f.ident.as_str()), f, types)
            })
            .collect();

        let non_constructor_fps: Vec<FuncProcessor> = std::iter::once(&info.destructor)
            .chain(info.funcs.iter())
            .map(|f| {
                FuncProcessor::from_info(names.to_func(f.func_type, f.ident.as_str()), f, types)
            })
            .collect();

        let json_objs = info
            .constructors
            .iter()
            .zip(constructor_fps.iter())
            .map(|(constructor_info, constructor_fp)| {
                let mut obj_json: Vec<Value> = vec![
                    "$OBJ".into(),
                    constructor_info.vcl_ident().into(),
                    json! {{ "NULL_OK": false }},
                    names.struct_obj_name().into(),
                ];
                obj_json.push(constructor_fp.json.clone());
                if let Some(r) = &constructor_fp.restrict_json {
                    obj_json.push(r.clone());
                }
                for fp in &non_constructor_fps {
                    obj_json.push(fp.json.clone());
                    if let Some(r) = &fp.restrict_json {
                        obj_json.push(r.clone());
                    }
                }
                obj_json.into()
            })
            .collect();

        let funcs: Vec<FuncProcessor> = constructor_fps
            .into_iter()
            .chain(non_constructor_fps)
            .collect();

        let mut obj = Self {
            names,
            json_objs,
            funcs,
            ..Default::default()
        };
        obj.cproto_typedef_decl = obj.gen_cproto();
        obj
    }

    /// per-object part of $CPROTO
    fn gen_cproto(&self) -> String {
        let mut decl = "\n".to_string();
        let _ = writeln!(decl, "{};", self.names.struct_obj_name());
        decl
    }
}
