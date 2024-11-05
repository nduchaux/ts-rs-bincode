use std::collections::HashMap;

use quote::ToTokens;
use syn::{Fields, GenericArgument, Ident, PathArguments, Type};

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
            self.process_type(stype);
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

    fn process_type(&mut self, stype: &Type) {
        let type_string = stype.to_token_stream().to_string();

        // Si le type est un type primitif ou générique, on ne fait rien
        if is_primitive_type(&type_string) || self.generics.iter().any(|g| g == &type_string) {
            return;
        }

        // Si le type est un type générique (ex: Option<T>, Vec<T>, etc.)
        if let Type::Path(type_path) = stype {
            if let Some(last_segment) = type_path.path.segments.last() {
                let ident = last_segment.ident.to_string();
                if ident == "Option" || ident == "Vec" || ident == "Result"
                /* ajoutez d'autres types génériques si nécessaire */
                {
                    if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                        for arg in &args.args {
                            if let GenericArgument::Type(inner_type) = arg {
                                // Appel récursif pour le type interne
                                self.process_type(inner_type);
                            }
                        }
                    }
                } else {
                    // Type non primitif et non générique connu, on l'ajoute aux définitions
                    self.def.insert(type_string.clone(), type_string);

                    // Vous pouvez également traiter les sous-types si ce type contient des types internes
                    if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                        for arg in &args.args {
                            if let GenericArgument::Type(inner_type) = arg {
                                self.process_type(inner_type);
                            }
                        }
                    }
                }
            }
        } else if let Type::Tuple(type_tuple) = stype {
            // Gérer les types tuple
            for elem in &type_tuple.elems {
                self.process_type(elem);
            }
        }
        // Ajoutez d'autres cas si nécessaire (par exemple, Type::Array)
    }

    pub fn add_field(&mut self, name: String, stype: &Type, include_in_def: bool) {
        if include_in_def {
            self.process_type(stype);
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
        // panic!("def: {:?}", self.def);
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
                    let def = format!("#/definitions/{}", last_type)
                        .replace("\n", "")
                        .replace(" ", "");
                    let final_type = sref.replace(&last_type, &def).replace(" ", "");
                    s.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                        field.name, final_type
                    ));
                // Example: Option < User > => Option < #/definitions/User >
                // Example: Vec < User > => Vec < #/definitions/User >
                // Example: Option < Vec < User >> => Option < Vec < #/definitions/User > >
                // Example: Option < Property < User > > => Option < #/definitions/ Property < #/definitions/User > >
                } else if self
                    .def
                    .iter()
                    .any(|(k, _)| field.sref.to_string().contains(k))
                {
                    let sref = field.sref.to_string();
                    let last_type =
                        _get_last_type_from_angle_brackets(sref.clone(), self.generics.clone());
                    let def = format!("#/definitions/{}", last_type)
                        .replace("\n", "")
                        .replace(" ", "");
                    let final_type = sref.replace(&last_type, &def).replace(" ", "");
                    s.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                        field.name, final_type
                    ));
                } else {
                    s.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                        field.name,
                        field.sref.to_string().replace(" ", "")
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
                        let sref: String = field.sref.to_string();
                        // let last_type =
                        //     _get_last_type_from_angle_brackets(sref.clone(), self.generics.clone());
                        let def = format!("#/definitions/{}", sref)
                            .replace("\n", "")
                            .replace(" ", "");
                        // panic!("last_type: {:?}", last_type);
                        let final_type = sref.replace(&sref, &def).replace(" ", "");
                        // panic!("final_type: {:?}", final_type);
                        s.push_str(&format!(
                            "        {{\n          \"name\": \"{}\",\n          \"type\": \"{}\"\n        }},\n",
                            name,
                            final_type,
                        ));
                    } else {
                        s.push_str(&format!(
                            "        {{\n          \"name\": \"{}\",\n          \"type\": \"{}\"\n        }},\n",
                            name,
                            field.sref.to_string().replace(" ", "")
                        ));
                    }
                    index += 1;
                }
                s.push_str("      ],\n");

                // Definitions part
                s.push_str("  \"definitions\": {\n");
                for field in &variant.fields {
                    if self.def.contains_key(&field.sref.to_string()) {
                        let sref = field.sref.to_string();
                        let _def = sref
                            // Replace any special characters with an underscore
                            .replace(|c: char| !c.is_alphanumeric(), "_")
                            .replace(" ", "")
                            // Remove duplicate underscores
                            .replace("__", "_")
                            .replace("__", "_")
                            // Remove trailing underscores
                            .trim_end_matches('_')
                            .trim_start_matches('_')
                            // Convert to lowercase
                            .to_uppercase();
                        let def = sref.replace("\n", "").replace(" ", "");
                        s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def, _def));
                    }
                }
                s.push_str("        },\n");
                s.push_str("        },\n");
            }
            s.push_str("    ],\n");
        }

        // Definitions part
        if self.stype == SchemaType::Struct {
            s.push_str("  \"definitions\": {\n");
            for (_, def) in &self.def {
                let _def = def
                    // Replace any special characters with an underscore
                    .replace(|c: char| !c.is_alphanumeric(), "_")
                    .replace(" ", "")
                    // Remove duplicate underscores
                    .replace("__", "_")
                    .replace("__", "_")
                    // Remove trailing underscores
                    .trim_end_matches('_')
                    .trim_start_matches('_')
                    // Convert to lowercase
                    .to_uppercase();
                let def = def.replace("\n", "").replace(" ", "");
                s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def, _def));
            }
            s.push_str("  },\n");
        }

        // Generics part
        s.push_str("  \"generics\": {\n");
        for generic in &self.generics {
            s.push_str(&format!("    \"{}\": &&&&{}&&&&,\n", generic, generic));
        }
        s.push_str("  }\n}");

        s
    }
}

fn _get_last_type_from_angle_brackets(type_string: String, generics: Vec<String>) -> String {
    if !type_string.contains('<') && !type_string.contains('>') && !type_string.contains(',') {
        return type_string.trim().to_string();
    }
    let start = type_string.find('<').unwrap_or(0);
    let end = type_string.rfind('>').unwrap_or(type_string.len());
    // if start == 0 || end == type_string.len() {
    //     return type_string;
    // }
    let _type_string = type_string[start + 1..end].to_string();
    if _type_string.contains('<') {
        return _get_last_type_from_angle_brackets(_type_string, generics);
    }
    if generics.iter().any(|g| _type_string.contains(g)) {
        return type_string.trim().to_string();
    }
    return _type_string.trim().to_string();
}

// Fonction pour vérifier si un type est primitif
fn is_primitive_type(type_string: &str) -> bool {
    matches!(
        type_string.as_ref(),
        "usize"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "String"
            | "Uuid"
            | "NaiveDateTime"
    )
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
