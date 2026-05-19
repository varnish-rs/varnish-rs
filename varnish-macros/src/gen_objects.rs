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

    /// JSON blob for the function
    pub json: Value,
    pub funcs: Vec<FuncProcessor>,
}

impl ObjProcessor {
    pub fn from_info(names: Names, info: &ObjInfo, types: &SharedTypes) -> Self {
        let funcs = info
            .iter()
            .map(|f| {
                FuncProcessor::from_info(names.to_func(f.func_type, f.ident.as_str()), f, types)
            })
            .collect();

        let mut obj = Self {
            names,
            funcs,
            ..Default::default()
        };
        obj.init();
        obj
    }

    fn init(&mut self) {
        self.cproto_typedef_decl = self.gen_cproto();
        self.json = self.get_json();
    }

    /// per-object part of $CPROTO
    fn gen_cproto(&self) -> String {
        let mut decl = "\n".to_string();
        let _ = writeln!(decl, "{};", self.names.struct_obj_name());
        decl
    }

    fn get_json(&self) -> Value {
        let mut json: Vec<Value> = vec![
            "$OBJ".into(),
            self.names.obj_name().into(),
            json! {{ "NULL_OK": false }},
            self.names.struct_obj_name().into(),
        ];
        for func in &self.funcs {
            json.push(func.json.clone());
            if let Some(restrict) = &func.restrict_json {
                json.push(restrict.clone());
            }
        }

        json.into()
    }
}
