use std::collections::{HashMap, HashSet};

use quote::ToTokens;
use syn::{Expr, Fields, GenericArgument, Ident, PathArguments, Token, Type};

#[derive(PartialEq, Debug)]
pub enum SchemaType {
    Enum,
    Struct,
}

#[derive(Debug)]
pub enum SchemaFieldRef {
    Type(String),
    Refs(String),
    ItemsRefs(String),
}

#[derive(Debug)]
pub struct SchemaField {
    name: String,
    sref: SchemaFieldRef,
}

#[derive(Debug)]
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

/// Schema represents a TypeScript schema for a Rust type.
/// 
/// It includes support for generic type parameters, both from the type itself
/// and from parent schemas. When a schema is created for a type that uses generic
/// type parameters from a parent schema, the parent generics should be extracted
/// from the schema string using `parse_parent_generics_from_schema` and added
/// to the `parent_generics` field using `add_parent_generics`.
/// 
/// Example:
/// ```
/// // When processing a schema string that might contain parent generics:
/// let parent_generics = Schema::parse_parent_generics_from_schema(&schema_str);
/// let mut schema = Schema::new("MyType".to_string(), SchemaType::Struct);
/// schema.add_parent_generics(&parent_generics);
/// ```
#[derive(Debug)]
pub struct Schema {
    name: String,
    /// Generic type parameters defined by this type (in declaration order)
    pub generics: Vec<String>,
    /// Generic type parameters from parent schemas that are used by this type
    pub parent_generics: Vec<String>,
    stype: SchemaType,
    pub fields: Vec<SchemaField>,
    pub variants: Vec<SchemaVariant>,
    // Clean def ==> Full def
    pub def: HashMap<String, String>,
}

impl Schema {
    pub fn new(name: String, stype: SchemaType) -> Self {
        Self {
            name,
            generics: Vec::new(),
            parent_generics: Vec::new(),
            stype,
            fields: Vec::new(),
            variants: Vec::new(),
            def: HashMap::new(),
        }
    }
    
    // Add parent generic type names to this schema
    pub fn add_parent_generics(&mut self, parent_generics: &[String]) {
        for generic in parent_generics {
            if !self.parent_generics.contains(generic) {
                self.parent_generics.push(generic.clone());
            }
        }
    }
    
    // Parse a schema string to extract parent generic type names
    pub fn parse_parent_generics_from_schema(schema_str: &str) -> Vec<String> {
        let mut result = Vec::new();
        
        // Look for the parent_generics field in the schema string
        if let Some(start_idx) = schema_str.find("\"parent_generics\":[") {
            // Find the start of the array
            let array_start = start_idx + "\"parent_generics\":[".len();
            // Find the end of the array
            if let Some(array_end) = schema_str[array_start..].find(']') {
                // Extract the array content
                let array_content = &schema_str[array_start..array_start + array_end];
                // Split by comma and process each item
                for item in array_content.split(',') {
                    // Remove quotes and whitespace
                    let clean_item = item.trim().trim_matches('"');
                    if !clean_item.is_empty() {
                        result.push(clean_item.to_string());
                    }
                }
            }
        }
        
        result
    }

    pub fn add_generic(&mut self, ident: Ident) {
        let s = ident.to_string();
        if !self.generics.contains(&s) {
            self.generics.push(s);
        }
    }

    pub fn add_variant(
        &mut self,
        name: String,
        fields: &Fields,
        discriminant: &Option<(Token![=], Expr)>,
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
            self.add_variant_field(name, &field.ty);
        }
    }

    pub fn add_variant_field(&mut self, name: String, stype: &Type) {
        self.process_type(stype);
        self.variants.last_mut().unwrap().fields.push(SchemaField {
            name,
            sref: match stype {
                Type::Array(t) => {
                    SchemaFieldRef::ItemsRefs(format!("{}", t.elem.to_token_stream().to_string()))
                }
                Type::Path(t) => SchemaFieldRef::Refs(format!("{}", remove_create_type_path(t))),
                _ => SchemaFieldRef::Type(stype.to_token_stream().to_string()),
            },
        });
    }

    fn process_type(&mut self, stype: &Type) {
        let type_string = stype.to_token_stream().to_string();

        // Si le type est un type primitif ou générique (local ou parent), on ne fait rien
        if is_primitive_type(&type_string) || 
           self.generics.iter().any(|g| g == &type_string) || 
           self.parent_generics.iter().any(|g| g == &type_string) {
            return;
        }

        // Si le type est un type générique (ex: Option<T>, Vec<T>, etc.)
        if let Type::Path(type_path) = stype {
            let segments = &type_path.path.segments;
            // Check if there is a '::' separator and the first segment is a known generic.
            // For associated types, something like T::Info would have at least two segments.
            if segments.len() > 1 {
                let first_segment = segments.first().unwrap().ident.to_string();
                // If the first segment matches a known generic parameter, skip adding this to `def`.
                if self.generics.contains(&first_segment) {
                    // Do not insert T::Info into def, just return.
                    return;
                }
            }

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
                    // panic!(
                    //     "Unknown type: {}, ident: {}, type_string: {}",
                    //     remove_create_type_path(type_path),
                    //     ident,
                    //     type_string
                    // );
                    // Type non primitif et non générique connu, on l'ajoute aux définitions
                    // self.def.insert(ident.clone(), type_string.clone());
                    // self.def.insert(type_string.clone(), type_string.clone());
                    // self.def.insert(ident.clone(), type_string.clone());
                    self.def.insert(
                        remove_create_type_path(type_path).replace(" ", ""),
                        type_path.to_token_stream().to_string().replace(" ", ""),
                    );
                    // self.dep
                    //     .insert(type_path.to_token_stream().to_string().replace(" ", ""));

                    // Vous pouvez également traiter les sous-types si ce type contient des types internes
                    if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                        for arg in &args.args {
                            if let GenericArgument::Type(inner_type) = arg {
                                self.process_type(inner_type);
                            }
                        }
                    } else {
                        let _type_string = last_segment.ident.to_string();
                        // self.def.insert(_type_string.clone(), type_string);
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

    pub fn add_field(&mut self, name: String, stype: &Type) {
        self.process_type(stype);
        self.fields.push(SchemaField {
            name,
            sref: match stype {
                Type::Array(t) => {
                    SchemaFieldRef::ItemsRefs(format!("{}", t.elem.to_token_stream().to_string()))
                }
                Type::Path(t) => SchemaFieldRef::Refs(format!("{}", remove_create_type_path(t))),
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
                let final_type =
                    replace_types(&sref, &hashmap_to_hashset(self.def.clone()), &self.generics, &self.parent_generics)
                        .replace(" ", "");
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
                        replace_types(&sref, &hashmap_to_hashset(self.def.clone()), &self.generics, &self.parent_generics)
                            .replace(" ", "");
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
                // panic!("def: {:?}", self.def);
                for field in &variant.fields {
                    let sref = field.sref.to_string().replace(" ", "");
                    // .replace("<", " < ")
                    // .replace(">", " >");
                    if self.def.contains_key(&sref) && !self.generics.contains(&sref) {
                        let def_name = sref
                            .replace(|c: char| !c.is_alphanumeric(), "_")
                            .replace(" ", "")
                            .replace("__", "_")
                            .replace("__", "_")
                            .trim_end_matches('_')
                            .trim_start_matches('_')
                            .to_uppercase();
                        // let def_key = sref.replace("\n", "").replace(" ", "");
                        let def_key = &sref.replace("\n", "").replace(" ", "");
                        // panic!("def_key: {:?} in def: {:?}", def_key, self.def);
                        s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def_key, def_name));
                    } else {
                        let type_names = extract_type_names(&sref);
                        for type_name in type_names {
                            if self.def.contains_key(&type_name)
                                && !self.generics.contains(&type_name)
                            {
                                let def_name = type_name
                                    .replace(|c: char| !c.is_alphanumeric(), "_")
                                    .replace(" ", "")
                                    .replace("__", "_")
                                    .replace("__", "_")
                                    .trim_end_matches('_')
                                    .trim_start_matches('_')
                                    .to_uppercase();
                                // let def_key = type_name.replace("\n", "").replace(" ", "");
                                let def_key = &type_name.replace("\n", "").replace(" ", "");
                                s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def_key, def_name));
                            } else {
                                // panic!("def not found: {} in def: {:?}", type_name, self.def);
                            }
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
            for (def, _) in &self.def {
                let def = def.replace("\n", "").replace(" ", "");
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
                if self.def.contains_key(&def) {
                    let def = def.replace("\n", "").replace(" ", "");
                    // panic!("def: {:?} in def: {:?}", def, self.def);
                    s.push_str(&format!("    \"{}\": &&&{}&&&,\n", def, _def));
                } else {
                    panic!("def not found: {} in def: {:?}", def, self.def);
                }
            }
            s.push_str("  },\n");
        }

        // Generics part
        s.push_str("  \"generics\": {\n");
        
        // Include both local generics and parent generics
        // First add local generics
        for generic in &self.generics {
            s.push_str(&format!("    \"{}\": &&&&{}&&&&,\n", generic, generic));
        }
        
        // Then add parent generics
        for generic in &self.parent_generics {
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

fn remove_create_type_path(type_path: &syn::TypePath) -> String {
    fn simplify_type(ty: &syn::Type) -> String {
        match ty {
            syn::Type::Path(type_path) => {
                let segment = type_path.path.segments.last().unwrap();
                let mut type_str = segment.ident.to_string();
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    let args_str = args
                        .args
                        .iter()
                        .filter_map(|arg| match arg {
                            syn::GenericArgument::Type(ty) => Some(simplify_type(ty)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    type_str.push('<');
                    type_str.push_str(&args_str);
                    type_str.push('>');
                }
                type_str
            }
            _ => ty.to_token_stream().to_string(),
        }
    }

    simplify_type(&syn::Type::Path(type_path.clone()))
}

fn replace_types(sref: &str, defs: &HashSet<String>, generics: &[String], parent_generics: &[String]) -> String {
    let mut result = String::new();
    let sref = sref.replace(" ", "");

    if sref.is_empty() {
        return result;
    }
    // Check if the type is in defs and not in either generics or parent_generics
    if defs.contains(&sref.to_string()) && 
       !generics.contains(&sref.to_string()) && 
       !parent_generics.contains(&sref.to_string()) {
        return format!("#/definitions/{}", sref);
    }

    if sref.contains("::") {
        let parts: Vec<&str> = sref.split("::").collect();
        if parts.len() == 2 && 
           (generics.contains(&parts[0].to_string()) || 
            parent_generics.contains(&parts[0].to_string())) {
            return sref.to_string();
        }
    }

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
            let replaced_inner = replace_types(&inner_type[..inner_type.len() - 1], defs, generics, parent_generics);
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
            if defs.contains(&type_name) && !generics.contains(&type_name) {
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

fn hashmap_to_hashset(hashmap: HashMap<String, String>) -> HashSet<String> {
    hashmap.into_iter().map(|(k, _)| k).collect()
}

/// Extracts generic parameters from a type reference string.
/// 
/// For example, given "ASample<C>", this function returns ["C"].
/// For "HashMap<String, D>", it returns ["String", "D"].
/// 
/// # Arguments
/// 
/// * `type_str` - The type reference string to parse
/// 
/// # Returns
/// 
/// A vector of strings representing the generic parameters
pub fn extract_generic_parameters(type_str: &str) -> Vec<String> {
    let mut result = Vec::new();
    
    // Find the opening angle bracket
    if let Some(start_idx) = type_str.find('<') {
        // Find the closing angle bracket
        if let Some(end_idx) = type_str.rfind('>') {
            // Extract the content between the angle brackets
            let content = &type_str[start_idx + 1..end_idx];
            
            // Handle nested angle brackets
            let mut bracket_count = 0;
            let mut current_param = String::new();
            
            for c in content.chars() {
                if c == '<' {
                    bracket_count += 1;
                    current_param.push(c);
                } else if c == '>' {
                    bracket_count -= 1;
                    current_param.push(c);
                } else if c == ',' && bracket_count == 0 {
                    // End of a parameter
                    if !current_param.is_empty() {
                        result.push(current_param.trim().to_string());
                        current_param = String::new();
                    }
                } else {
                    current_param.push(c);
                }
            }
            
            // Add the last parameter
            if !current_param.is_empty() {
                result.push(current_param.trim().to_string());
            }
        }
    }
    
    result
}

// fn get_last_type_from_angle_brackets(type_string: String, generics: Vec<String>) -> String {
//     if !type_string.contains('<') && !type_string.contains('>') && !type_string.contains(',') {
//         return type_string.trim().to_string();
//     }
//     let start = type_string.find('<').unwrap_or(0);
//     let end = type_string.rfind('>').unwrap_or(type_string.len());
//     // if start == 0 || end == type_string.len() {
//     //     return type_string;
//     // }
//     let _type_string = type_string[start + 1..end].to_string();
//     if _type_string.contains('<') {
//         return get_last_type_from_angle_brackets(_type_string, generics);
//     }
//     if generics.iter().any(|g| _type_string.contains(g)) {
//         return type_string.trim().to_string();
//     }
//     return _type_string.trim().to_string();
// }

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

mod tests {
    #[test]
    fn test_is_primitive_type() {
        use super::is_primitive_type;

        assert_eq!(is_primitive_type("usize"), true);
        assert_eq!(is_primitive_type("isize"), true);
        assert_eq!(is_primitive_type("i8"), true);
        assert_eq!(is_primitive_type("i16"), true);
        assert_eq!(is_primitive_type("i32"), true);
        assert_eq!(is_primitive_type("i64"), true);
        assert_eq!(is_primitive_type("u8"), true);
        assert_eq!(is_primitive_type("u16"), true);
        assert_eq!(is_primitive_type("u32"), true);
        assert_eq!(is_primitive_type("u64"), true);
        assert_eq!(is_primitive_type("f32"), true);
        assert_eq!(is_primitive_type("f64"), true);
        assert_eq!(is_primitive_type("bool"), true);
        assert_eq!(is_primitive_type("char"), true);
        assert_eq!(is_primitive_type("String"), true);
        assert_eq!(is_primitive_type("Uuid"), true);
        assert_eq!(is_primitive_type("NaiveDateTime"), true);
        assert_eq!(is_primitive_type("Option<usize>"), false);
        assert_eq!(is_primitive_type("Vec<usize>"), false);
        assert_eq!(is_primitive_type("Option<Vec<usize>>"), false);
    }

    #[test]
    fn test_extract_type_names() {
        use super::extract_type_names;

        assert_eq!(extract_type_names("usize"), vec!["usize"]);
        assert_eq!(extract_type_names("Option<usize>"), vec!["Option", "usize"]);
        assert_eq!(extract_type_names("Vec<usize>"), vec!["Vec", "usize"]);
        assert_eq!(
            extract_type_names("Option<Vec<usize>>"),
            vec!["Option", "Vec", "usize"]
        );
        assert_eq!(
            extract_type_names("HashMap<String, usize>"),
            vec!["HashMap", "String", "usize"]
        );
    }

    #[test]
    fn test_remove_create_type_path() {
        use syn::{parse_quote, TypePath};

        mod my_fake_module {
            pub struct HashMap<K, V> {
                pub key: K,
                pub value: V,
            }
        }

        mod my_fake_module2 {
            mod my_fake_module {
                pub struct HashMap<K, V> {
                    pub key: K,
                    pub value: V,
                }
            }
        }

        use super::remove_create_type_path;

        let type_path: TypePath = parse_quote!(std::collections::HashMap<String, usize>);
        assert_eq!(
            remove_create_type_path(&type_path),
            "HashMap<String, usize>"
        );

        let type_path: TypePath = parse_quote!(usize);
        assert_eq!(remove_create_type_path(&type_path), "usize");

        let type_path: TypePath = parse_quote!(Option<usize>);
        assert_eq!(remove_create_type_path(&type_path), "Option<usize>");

        let type_path: TypePath = parse_quote!(Vec<usize>);
        assert_eq!(remove_create_type_path(&type_path), "Vec<usize>");

        let type_path: TypePath = parse_quote!(my_fake_module::HashMap<String, usize>);
        assert_eq!(
            remove_create_type_path(&type_path),
            "HashMap<String, usize>"
        );

        let type_path: TypePath = parse_quote!(
            my_fake_module2::my_fake_module::HashMap<my_fake_module::HashMap<String, usize>, usize>
        );
        assert_eq!(
            remove_create_type_path(&type_path),
            "HashMap<HashMap<String, usize>, usize>"
        );
    }

    // #[test]
    // fn test_replace_types() {
    //     use std::collections::HashMap;

    //     let mut defs = HashMap::new();
    //     defs.insert("MyObject".to_string(), "MyObject".to_string());
    //     defs.insert("Params".to_string(), "Params".to_string());

    //     let generics = vec!["T".to_string(), "Complex".to_string()];

    //     assert_eq!(
    //         super::replace_types("MyObject", &defs, &generics),
    //         "#/definitions/MyObject".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Params", &defs, &generics),
    //         "#/definitions/Params".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("MyObject<Params>", &defs, &generics),
    //         "#/definitions/MyObject<#/definitions/Params>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("MyObject<Params, T>", &defs, &generics),
    //         "#/definitions/MyObject<#/definitions/Params, T>".to_string()
    //     );
    //     assert_eq!(super::replace_types("T", &defs, &generics), "T".to_string());
    //     assert_eq!(
    //         super::replace_types("Complex", &defs, &generics),
    //         "Complex".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Option<T>", &defs, &generics),
    //         "Option<T>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Vec<Complex>", &defs, &generics),
    //         "Vec<Complex>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Option<MyObject>", &defs, &generics),
    //         "Option<#/definitions/MyObject>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Vec<MyObject>", &defs, &generics),
    //         "Vec<#/definitions/MyObject>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("Option<Vec<MyObject>>", &defs, &generics),
    //         "Option<Vec<#/definitions/MyObject>>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("HashMap<String, usize>", &defs, &generics),
    //         "HashMap<String, usize>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("HashMap<String, MyObject>", &defs, &generics),
    //         "HashMap<String, #/definitions/MyObject>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("HashMap<String, Params>", &defs, &generics),
    //         "HashMap<String, #/definitions/Params>".to_string()
    //     );
    //     assert_eq!(
    //         super::replace_types("HashMap<String, MyObject, Params>", &defs, &generics),
    //         "HashMap<String, #/definitions/MyObject, #/definitions/Params>".to_string()
    //     );
    // }

    // #[test]
    // fn test_process_type() {
    //     let mut schema = super::Schema::new("MyObject".to_string(), super::SchemaType::Struct);
    //     schema.process_type(&syn::parse_quote!(usize));
    //     schema.process_type(&syn::parse_quote!(String));
    //     schema.process_type(&syn::parse_quote!(Option<usize>));
    //     schema.process_type(&syn::parse_quote!(Vec<usize>));
    //     schema.process_type(&syn::parse_quote!(Option<Vec<usize>>));
    //     schema.process_type(&syn::parse_quote!(HashMap<String, usize>));
    //     assert_eq!(schema.def.len(), 0);

    //     let mut schema = super::Schema::new("MyObject".to_string(), super::SchemaType::Struct);
    //     schema.add_generic(syn::Ident::new("T", proc_macro2::Span::call_site()));
    //     schema.process_type(&syn::parse_quote!(MyObject));
    //     schema.process_type(&syn::parse_quote!(Params));
    //     schema.process_type(&syn::parse_quote!(T));
    //     schema.process_type(&syn::parse_quote!(Complex));
    //     assert_eq!(schema.def.len(), 3);
    //     assert_eq!(schema.def.get("MyObject"), Some(&"MyObject".to_string()));
    //     assert_eq!(schema.def.get("Params"), Some(&"Params".to_string()));
    //     assert_eq!(schema.def.get("Complex"), Some(&"Complex".to_string()));
    //     assert_eq!(schema.def.get("T"), None);
    //     assert_eq!(schema.generics.len(), 1);
    //     assert_eq!(schema.generics[0], "T");

    //     let mut schema = super::Schema::new("MyObject".to_string(), super::SchemaType::Struct);
    //     schema.add_generic(syn::Ident::new("T", proc_macro2::Span::call_site()));
    //     schema.process_type(&syn::parse_quote!(MyObject<Params>));
    //     println!("{:?}", schema.def);
    //     assert_eq!(schema.def.len(), 2);
    //     assert_eq!(schema.def.get("Params"), Some(&"Params".to_string()));
    //     assert_eq!(
    //         schema.def.get("MyObject<Params>"),
    //         Some(&"MyObject < Params >".to_string())
    //     );
    //     schema.process_type(&syn::parse_quote!(MyObject<Params, T>));
    //     println!("{:?}", schema.def);
    //     assert_eq!(schema.def.len(), 3);
    //     assert_eq!(
    //         schema.def.get("MyObject<Params, T>"),
    //         Some(&"MyObject < Params , T >".to_string())
    //     );
    //     assert_eq!(
    //         schema.def.get("MyObject<Params>"),
    //         Some(&"MyObject < Params >".to_string())
    //     );
    //     assert_eq!(schema.def.get("Params"), Some(&"Params".to_string()));
    //     assert_eq!(schema.def.get("T"), None);
    // }

    #[test]
    fn test_add_field() {
        let mut schema = super::Schema::new("MyObject".to_string(), super::SchemaType::Struct);
        schema.add_field("id".to_string(), &syn::parse_quote!(usize));
        schema.add_field("name".to_string(), &syn::parse_quote!(String));
        schema.add_field("age".to_string(), &syn::parse_quote!(u8));
        schema.add_field("is_active".to_string(), &syn::parse_quote!(bool));
        schema.add_field("created_at".to_string(), &syn::parse_quote!(NaiveDateTime));
        schema.add_field("updated_at".to_string(), &syn::parse_quote!(NaiveDateTime));
        assert_eq!(schema.fields.len(), 6);
        assert_eq!(schema.fields[0].name, "id");
        assert_eq!(schema.fields[1].name, "name");
        assert_eq!(schema.fields[2].name, "age");
        assert_eq!(schema.fields[3].name, "is_active");
        assert_eq!(schema.fields[4].name, "created_at");
        assert_eq!(schema.fields[5].name, "updated_at");

        // TODO: Verify is coherence with the expected result
    }

    #[test]
    fn test_add_variant() {
        // TODO: Add tests
    }

    #[test]
    fn test_to_string() {
        // Create a schema with a struct
        let mut schema = super::Schema::new("MyObject".to_string(), super::SchemaType::Struct);
        schema.add_field("id".to_string(), &syn::parse_quote!(usize));
        schema.add_field("name".to_string(), &syn::parse_quote!(String));

        let expected = r#"{
            "type": "struct",
            "name": "MyObject",
            "fields": [
                {
                    "name": "id",
                    "type": "usize"
                },
                {
                    "name": "name",
                    "type": "String"
                },
            ],
            "definitions": {},
            "generics": {}
        }"#;
        assert_eq!(
            schema.to_string().replace(' ', "").replace('\n', ""),
            expected.replace(' ', "").replace('\n', "")
        );
    }
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
