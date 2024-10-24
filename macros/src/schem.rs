use std::collections::HashMap;

use quote::ToTokens;
use syn::{Fields, Ident, Type};

#[derive(PartialEq)]
pub enum SchemaType {
    Enum,
    Struct,
}

pub enum SchemaFieldRef {
    Type(String),
    Refs(String),
    ItemsRefs(String),
}

pub struct SchemaField {
    name: String,
    sref: SchemaFieldRef,
}

pub struct SchemaVariant {
    name: String,
    fields: Vec<SchemaField>,
}

impl SchemaFieldRef {
    pub fn to_string(&self) -> String {
        match self {
            SchemaFieldRef::Type(s) => s.clone(),
            SchemaFieldRef::Refs(s) => s.clone(),
            SchemaFieldRef::ItemsRefs(s) => s.clone(),
        }
    }
}

pub struct Schema {
    name: String,
    pub generics: Vec<String>,
    stype: SchemaType,
    pub fields: Vec<SchemaField>,
    pub variants: Vec<SchemaVariant>,
    pub def: HashMap<String, String>,
}

impl Schema {
    pub fn new(name: String, stype: SchemaType) -> Self {
        Self {
            name,
            generics: Vec::new(),
            stype,
            fields: Vec::new(),
            variants: Vec::new(),
            def: HashMap::new(),
        }
    }

    pub fn add_generic(&mut self, ident: Ident) {
        self.generics.push(ident.to_string());
    }

    pub fn add_variant(&mut self, name: String, fields: &Fields, include_in_def: bool) {
        self.variants.push(SchemaVariant {
            name,
            fields: Vec::new(),
        });

        for field in fields {
            let name = match &field.ident {
                Some(ident) => ident.to_string(),
                None => "".to_string(),
            };
            self.add_variant_field(name, &field.ty, include_in_def);
        }
    }

    pub fn add_variant_field(&mut self, name: String, stype: &Type, include_in_def: bool) {
        // panic!("generics: {:?}", self.generics);
        // panic!("stype: {:?}", stype.to_token_stream().to_string());
        if include_in_def {
            let type_string = stype.to_token_stream().to_string();
            if !type_string.contains("i8")
                && !type_string.contains("i32")
                && !type_string.contains("i64")
                && !type_string.contains("f32")
                && !type_string.contains("f64")
                && !type_string.contains("u8")
                && !type_string.contains("u32")
                && !type_string.contains("u64")
                && !type_string.contains("bool")
                && !type_string.contains("char")
                && !type_string.contains("String")
                && !type_string.contains("Uuid")
                && !self.generics.iter().any(|g| g == &type_string)
            {
                // let text = format!("{}: {}", name, stype.to_token_stream().to_string());
                // panic!("{} is not implemented", text);
                self.def
                    // .insert(name.clone(), stype.to_token_stream().to_string());
                    .insert(
                        type_string.clone(),
                        get_last_type_from_angle_brackets(stype, self.generics.clone()),
                    );
            }
        }

        self.variants.last_mut().unwrap().fields.push(SchemaField {
            name,
            sref: match stype {
                Type::Array(t) => {
                    SchemaFieldRef::ItemsRefs(format!("{}", t.elem.to_token_stream().to_string()))
                }
                Type::Path(t) => {
                    SchemaFieldRef::Refs(format!("{}", t.path.to_token_stream().to_string()))
                }
                _ => SchemaFieldRef::Type(stype.to_token_stream().to_string()),
            },
        });
    }

    pub fn add_field(&mut self, name: String, stype: &Type, include_in_def: bool) {
        if include_in_def {
            let type_string = stype.to_token_stream().to_string();
            if !type_string.contains("i8")
                && !type_string.contains("i32")
                && !type_string.contains("i64")
                && !type_string.contains("f32")
                && !type_string.contains("f64")
                && !type_string.contains("u8")
                && !type_string.contains("u32")
                && !type_string.contains("u64")
                && !type_string.contains("bool")
                && !type_string.contains("char")
                && !type_string.contains("String")
                && !type_string.contains("Uuid")
                && !self.generics.iter().any(|g| g == &type_string)
            {
                // let text = format!("{}: {}", name, stype.to_token_stream().to_string());
                // panic!("{} is not implemented", text);
                self.def
                    // .insert(name.clone(), stype.to_token_stream().to_string());
                    .insert(
                        type_string,
                        get_last_type_from_angle_brackets(stype, self.generics.clone()),
                    );
            }
        }
        self.fields.push(SchemaField {
            name,
            sref: match stype {
                Type::Array(t) => {
                    SchemaFieldRef::ItemsRefs(format!("{}", t.elem.to_token_stream().to_string()))
                }
                Type::Path(t) => {
                    SchemaFieldRef::Refs(format!("{}", t.path.to_token_stream().to_string()))
                }
                _ => SchemaFieldRef::Type(stype.to_token_stream().to_string()),
            },
        });
    }

    pub fn to_string(&self) -> String {
        // Header part
        let mut s = format!(
            "{{\n  \"type\": \"{}\",\n  \"name\": \"{}\",\n  \"{}\": [\n",
            match self.stype {
                SchemaType::Enum => "enum",
                SchemaType::Struct => "struct",
            },
            self.name,
            match self.stype {
                SchemaType::Enum => "variants",
                SchemaType::Struct => "fields",
            },
        );

        // Fields part
        if self.stype == SchemaType::Struct {
            for field in &self.fields {
                if self.def.contains_key(&field.sref.to_string()) {
                    let sref = field.sref.to_string();
                    let last_type =
                        _get_last_type_from_angle_brackets(sref.clone(), self.generics.clone());
                    let def = format!("#/definitions/{}", last_type);
                    let final_type = sref.replace(&last_type, &def);
                    s.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                        field.name, final_type
                    ));
                } else {
                    s.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                        field.name,
                        field.sref.to_string()
                    ));
                }
            }
            s.push_str("  ],\n");
        }

        // Variants part
        if self.stype == SchemaType::Enum {
            for variant in &self.variants {
                s.push_str(&format!(
                    "    {{\n      \"name\": \"{}\",\n      \"type\": \"struct\",\n      \"fields\": [\n",
                    variant.name
                ));
                let mut index = 0;
                for field in &variant.fields {
                    let name = if field.name.is_empty() {
                        index.to_string()
                    } else {
                        field.name.clone()
                    };
                    if self.def.contains_key(&field.sref.to_string()) {
                        let sref = field.sref.to_string();
                        let last_type =
                            _get_last_type_from_angle_brackets(sref.clone(), self.generics.clone());
                        let def = format!("#/definitions/{}", last_type);
                        let final_type = sref.replace(&last_type, &def);
                        s.push_str(&format!(
                            "        {{\n          \"name\": \"{}\",\n          \"type\": \"{}\"\n        }},\n",
                            name,
                            final_type,
                        ));
                    } else {
                        s.push_str(&format!(
                            "        {{\n          \"name\": \"{}\",\n          \"type\": \"{}\"\n        }},\n",
                            name,
                            field.sref.to_string()
                        ));
                    }
                    index += 1;
                }
                s.push_str("      ]\n    },\n");
            }
            s.push_str("  ],\n");
        }

        // Definitions part
        s.push_str("  \"definitions\": {\n");
        for (_, def) in &self.def {
            let _def = def
                // Replace any special characters with an underscore
                .replace(|c: char| !c.is_alphanumeric(), "_")
                // Remove duplicate underscores
                .replace("__", "_")
                .replace("__", "_")
                // Remove trailing underscores
                .trim_end_matches('_')
                .trim_start_matches('_')
                // Convert to lowercase
                .to_uppercase();
            s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def, _def));
        }
        s.push_str("  }\n}");

        s
    }
}

fn _get_last_type_from_angle_brackets(type_string: String, generics: Vec<String>) -> String {
    let start = type_string.find('<').unwrap_or(0);
    let end = type_string.rfind('>').unwrap_or(type_string.len());
    if start == 0 || end == type_string.len() {
        return type_string;
    }
    let _type_string = type_string[start + 1..end].to_string();
    if _type_string.contains('<') {
        return _get_last_type_from_angle_brackets(_type_string, generics);
    }
    if generics.iter().any(|g| _type_string.contains(g)) {
        return type_string.trim().to_string();
    }
    return _type_string.trim().to_string();
}

fn get_last_type_from_angle_brackets(type_: &Type, generics: Vec<String>) -> String {
    let type_string = type_.to_token_stream().to_string();
    return _get_last_type_from_angle_brackets(type_string, generics);
}

// fn _remove_generics_from_angle_brackets(type_string: String, generics: Vec<String>) -> String {
//     let mut type_string = type_string;
//     for generic in generics {
//         type_string = type_string.replace(&format!("< {} >", generic), "");
//     }
//     return type_string;
// }

// fn remove_generics_from_angle_brackets(type_: &Type, generics: Vec<String>) -> String {
//     let type_string = type_.to_token_stream().to_string();
//     return _remove_generics_from_angle_brackets(type_string, generics);
// }
