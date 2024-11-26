use std::collections::HashMap;

use quote::ToTokens;
use syn::{Expr, Fields, GenericArgument, Ident, PathArguments, Token, Type};

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
    discriminant: Option<i32>,
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

    pub fn add_variant(
        &mut self,
        name: String,
        fields: &Fields,
        discriminant: &Option<(Token![=], Expr)>,
        include_in_def: bool,
    ) {
        let discriminant = match discriminant {
            Some((_, expr)) => {
                if let syn::Expr::Lit(lit) = expr {
                    if let syn::Lit::Int(int) = &lit.lit {
                        Some(int.base10_parse::<i32>().unwrap())
                    } else {
                        None
                    }
                    // lit.to_token_stream().to_string()
                } else {
                    None
                }
            }
            None => None,
        };

        self.variants.push(SchemaVariant {
            name,
            fields: Vec::new(),
            discriminant,
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
                    if let Some(last_segment) = t.path.segments.last() {
                        SchemaFieldRef::Refs(last_segment.ident.to_string())
                    } else {
                        SchemaFieldRef::Refs("Unknown".to_string())
                    }
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
                if ident == "Option" || ident == "Vec" || ident == "Result" || ident == "HashMap"
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
                    // self.def.insert(type_string.clone(), type_string);

                    // Vous pouvez également traiter les sous-types si ce type contient des types internes
                    if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                        for arg in &args.args {
                            if let GenericArgument::Type(inner_type) = arg {
                                self.process_type(inner_type);
                            }
                        }
                    } else {
                        let type_string = last_segment.ident.to_string();
                        self.def.insert(type_string.clone(), type_string);
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
                    if let Some(last_segment) = t.path.segments.last() {
                        SchemaFieldRef::Refs(last_segment.ident.to_string())
                    } else {
                        SchemaFieldRef::Refs("Unknown".to_string())
                    }
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
        // Partie du code modifiée
        if self.stype == SchemaType::Struct {
            for field in &self.fields {
                let sref = field.sref.to_string();
                let final_type = replace_types(&sref, &self.def, &self.generics).replace(" ", "");
                s.push_str(&format!(
                    "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\"\n    }},\n",
                    field.name, final_type
                ));
            }
            s.push_str("  ],\n");
        }

        // Variants part
        if self.stype == SchemaType::Enum {
            let mut variant_index: i32 = 0;
            for variant in &self.variants {
                if let Some(discriminant) = variant.discriminant {
                    variant_index = discriminant;
                }
                s.push_str(&format!(
            "    {{\n      \"name\": \"{}\",\n      \"discriminant\": {},\n      \"type\": \"struct\",\n      \"fields\": [\n",
            variant.name,
            variant_index
        ));
                let mut index: i32 = 0;
                for field in &variant.fields {
                    let name = if field.name.is_empty() {
                        index.to_string()
                    } else {
                        field.name.clone()
                    };
                    let sref = field.sref.to_string();
                    let final_type =
                        replace_types(&sref, &self.def, &self.generics).replace(" ", "");
                    s.push_str(&format!(
                "        {{\n          \"name\": \"{}\",\n          \"type\": \"{}\"\n        }},\n",
                name,
                final_type,
            ));
                    index += 1;
                }
                s.push_str("      ],\n");

                // Definitions part
                s.push_str("  \"definitions\": {\n");
                for field in &variant.fields {
                    let sref = field.sref.to_string();
                    let type_names = extract_type_names(&sref);
                    for type_name in type_names {
                        if self.def.contains_key(&type_name) && !self.generics.contains(&type_name)
                        {
                            let def_name = type_name
                                .replace(|c: char| !c.is_alphanumeric(), "_")
                                .replace(" ", "")
                                .replace("__", "_")
                                .replace("__", "_")
                                .trim_end_matches('_')
                                .trim_start_matches('_')
                                .to_uppercase();
                            let def_key = type_name.replace("\n", "").replace(" ", "");
                            s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def_key, def_name));
                        }
                    }
                }
                s.push_str("        },\n");
                s.push_str("        },\n");

                variant_index += 1;
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

fn extract_type_names(sref: &str) -> Vec<String> {
    let mut type_names = Vec::new();
    let mut chars = sref.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphanumeric() || c == '_' {
            let mut type_name = c.to_string();
            while let Some(&next_c) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    type_name.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            type_names.push(type_name);
        } else if c == '<' || c == ',' {
            continue;
        } else {
            // Ignorer les autres caractères
        }
    }
    type_names
}

fn replace_types(sref: &str, defs: &HashMap<String, String>, generics: &[String]) -> String {
    let mut result = String::new();
    let mut chars = sref.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            result.push(c);
            let mut inner_type = String::new();
            let mut bracket_count = 1;
            while let Some(&next_c) = chars.peek() {
                chars.next();
                inner_type.push(next_c);
                if next_c == '<' {
                    bracket_count += 1;
                } else if next_c == '>' {
                    bracket_count -= 1;
                    if bracket_count == 0 {
                        break;
                    }
                }
            }
            // Appel récursif pour les types à l'intérieur des crochets
            let replaced_inner = replace_types(&inner_type[..inner_type.len() - 1], defs, generics);
            result.push_str(&replaced_inner);
            result.push('>');
        } else if c.is_alphanumeric() || c == '_' {
            let mut type_name = c.to_string();
            while let Some(&next_c) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    type_name.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if defs.contains_key(&type_name) && !generics.contains(&type_name) {
                result.push_str(&format!("#/definitions/{}", type_name));
            } else {
                result.push_str(&type_name);
            }
        } else {
            result.push(c);
        }
    }
    result
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
