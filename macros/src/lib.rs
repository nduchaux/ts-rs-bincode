#![macro_use]
// #![deny(unused)]

use std::collections::{HashMap, HashSet};

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse_quote, spanned::Spanned, ConstParam, GenericParam, Generics, Item, LifetimeParam, Path,
    Result, Type, TypeArray, TypeParam, TypeParen, TypePath, TypeReference, TypeSlice, TypeTuple,
    WhereClause, WherePredicate,
};

use crate::{deps::Dependencies, utils::format_generics};

#[macro_use]
mod utils;
mod attr;
mod deps;
mod schem;
mod types;

struct DerivedTS {
    crate_rename: Path,
    ts_name: String,
    docs: String,
    inline: TokenStream,
    inline_flattened: Option<TokenStream>,
    dependencies: Dependencies,
    concrete: HashMap<Ident, Type>,
    bound: Option<Vec<WherePredicate>>,
    schema: Option<schem::Schema>,

    export: bool,
    export_to: Option<String>,
}

impl DerivedTS {
    fn into_impl(mut self, rust_ty: Ident, generics: Generics) -> TokenStream {
        #[cfg(feature = "default-export")]
        let default_export = true;
        #[cfg(not(feature = "default-export"))]
        let default_export = false;

        let export =
            (self.export || default_export).then(|| self.generate_export_test(&rust_ty, &generics));

        let output_path_fn = {
            let path = match self.export_to.as_deref() {
                Some(dirname) if dirname.ends_with('/') => {
                    format!("{}{}.ts", dirname, self.ts_name)
                }
                Some(filename) => filename.to_owned(),
                None => format!("{}.ts", self.ts_name),
            };

            quote! {
                fn output_path() -> Option<&'static std::path::Path> {
                    Some(std::path::Path::new(#path))
                }
            }
        };

        let docs = match &*self.docs {
            "" => None,
            docs => Some(quote!(const DOCS: Option<&'static str> = Some(#docs);)),
        };

        let crate_rename = self.crate_rename.clone();

        let ident = self.ts_name.clone();
        let impl_start = generate_impl_block_header(
            &crate_rename,
            &rust_ty,
            &generics,
            self.bound.as_deref(),
            &self.dependencies,
        );
        let assoc_type = generate_assoc_type(&rust_ty, &crate_rename, &generics, &self.concrete);
        let name = self.generate_name_fn(&generics);
        let inline = self.generate_inline_fn();
        let decl = self.generate_decl_fn(&rust_ty, &generics);
        let dependencies = &self.dependencies;
        let generics_fn = self.generate_generics_fn(&generics);
        let schem = self.generate_schem_fn(&rust_ty, &generics, &self.dependencies);
        let schema_deps_fn = self.generate_schema_deps_fn(&rust_ty, &generics);

        let final_q = quote! {
            #impl_start {
                #assoc_type

                fn ident() -> String {
                    #ident.to_owned()
                }

                #docs
                #name
                #decl
                #inline
                #schem
                #schema_deps_fn
                #generics_fn
                #output_path_fn

                fn visit_dependencies(v: &mut impl #crate_rename::TypeVisitor)
                where
                    Self: 'static,
                {
                    #dependencies
                }
            }

            #export
        };
        // write impl to file for debugging
        // use std::fs::File;
        // use std::io::Write;
        // let folder = std::fs::create_dir_all("example/bindings/rs").unwrap();
        // let mut file = File::create(format!("{}/{}.rs", "example/bindings/rs", rust_ty)).unwrap();
        // write!(file, "{}", final_q).unwrap();
        final_q
    }

    /// Returns an expression which evaluates to the TypeScript name of the type, including generic
    /// parameters.
    fn name_with_generics(&self, generics: &Generics) -> TokenStream {
        let name = &self.ts_name;
        let crate_rename = &self.crate_rename;
        let mut generics_ts_names = generics
            .type_params()
            .filter(|ty| !self.concrete.contains_key(&ty.ident))
            .map(|ty| &ty.ident)
            .map(|generic| quote!(<#generic as #crate_rename::TS>::name()))
            .peekable();

        if generics_ts_names.peek().is_some() {
            quote! {
                format!("{}<{}>", #name, vec![#(#generics_ts_names),*].join(", "))
            }
        } else {
            quote!(#name.to_owned())
        }
    }

    /// Generate a dummy unit struct for every generic type parameter of this type.
    /// # Example:
    /// ```compile_fail
    /// struct Generic<A, B, const C: usize> { /* ... */ }
    /// ```
    /// has two generic type parameters, `A` and `B`. This function will therefore generate
    /// ```compile_fail
    /// struct A;
    /// impl ts_rs::TS for A { /* .. */ }
    ///
    /// struct B;
    /// impl ts_rs::TS for B { /* .. */ }
    /// ```
    fn generate_generic_types(&self, generics: &Generics) -> TokenStream {
        let crate_rename = &self.crate_rename;
        let generics = generics
            .type_params()
            .filter(|ty| !self.concrete.contains_key(&ty.ident))
            .map(|ty| ty.ident.clone());
        let name = quote![<Self as #crate_rename::TS>::name()];
        quote! {
            #(
                #[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
                struct #generics;
                impl std::fmt::Display for #generics {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "{:?}", self)
                    }
                }
                impl #crate_rename::TS for #generics {
                    type WithoutGenerics = #generics;
                    fn name() -> String { stringify!(#generics).to_owned() }
                    fn inline() -> String { panic!("{} cannot be inlined", #name) }
                    fn inline_flattened() -> String { stringify!(#generics).to_owned() }
                    fn decl() -> String { panic!("{} cannot be declared", #name) }
                    fn decl_concrete() -> String { panic!("{} cannot be declared", #name) }
                    fn schema(export: bool) -> String { panic!("{} cannot have a schema", #name) }
                }
            )*
        }
    }

    fn generate_export_test(&self, rust_ty: &Ident, generics: &Generics) -> TokenStream {
        let test_fn = format_ident!(
            "export_bindings_{}",
            rust_ty.to_string().to_lowercase().replace("r#", "")
        );
        let crate_rename = &self.crate_rename;
        let generic_params = generics
            .type_params()
            .map(|ty| match self.concrete.get(&ty.ident) {
                None => quote! { #crate_rename::Dummy },
                Some(ty) => quote! { #ty },
            });
        let ty = quote!(<#rust_ty<#(#generic_params),*> as #crate_rename::TS>);

        quote! {
            #[cfg(test)]
            #[test]
            fn #test_fn() {
                #ty::export_all().expect("could not export type");
            }
        }
    }

    fn generate_generics_fn(&self, generics: &Generics) -> TokenStream {
        let crate_rename = &self.crate_rename;
        let generics = generics
            .type_params()
            .filter(|ty| !self.concrete.contains_key(&ty.ident))
            .map(|TypeParam { ident, .. }| {
                quote![
                    v.visit::<#ident>();
                    <#ident as #crate_rename::TS>::visit_generics(v);
                ]
            });
        quote! {
            fn visit_generics(v: &mut impl #crate_rename::TypeVisitor)
            where
                Self: 'static,
            {
                #(#generics)*
            }
        }
    }

    /// Generates `visit_schema_dependencies` which visits all types referenced in the
    /// schema `definitions` section. This covers types that might be #[ts(inline)] and
    /// therefore absent from the regular `visit_dependencies`.
    fn generate_schema_deps_fn(&self, _rust_ty: &Ident, _generics: &Generics) -> TokenStream {
        let crate_rename = &self.crate_rename;
        let _o_name = self.ts_name.clone();

        let Some(schema) = &self.schema else {
            return quote! {};
        };

        // Collect all unique types from schema.def, excluding self-references
        // For generic instantiations like Point<Gender>, we visit the base type
        // (Point) via the full type, and also each concrete generic arg (Gender).
        let visits: Vec<TokenStream> = schema.def.iter().filter_map(|(_, full_ty)| {
            let full_ty_clean = full_ty.replace(" ", "");
            if full_ty_clean == _o_name {
                return None; // skip self-reference
            }
            let ty: TokenStream = match full_ty.parse() {
                Ok(t) => t,
                Err(_) => return None,
            };
            // Visit the type itself (handles both simple and generic instantiations)
            Some(quote! {
                v.visit::<#ty>();
            })
        }).collect();

        if visits.is_empty() {
            return quote! {};
        }

        quote! {
            fn visit_schema_dependencies(v: &mut impl #crate_rename::TypeVisitor)
            where
                Self: 'static,
            {
                #(#visits)*
            }
        }
    }

    fn generate_schem_fn(
        &self,
        _rust_ty: &Ident,
        _generics: &Generics,
        _dependencies: &Dependencies,
    ) -> TokenStream {
        let crate_rename = &self.crate_rename;
        let _o_name = self.ts_name.clone();
        let name = &self.ts_name;
        let name = format!("{}Schema", name);
        if let Some(schema) = &self.schema {
            // get only values of the map (def)
            let def_type_list: HashMap<String, String> = schema.def.clone();
            let schema = schema.to_string();
            let dependencies = def_type_list
                .into_iter()
                .map(|(ty, full_ty)| {
                    if ty.is_empty() {
                        panic!("ty is empty")
                    }
                    let __ty: TokenStream = full_ty.parse().unwrap();
                    let _ty: TokenStream = ty
                        .replace(|c: char| !c.is_alphanumeric(), "_")
                        .replace("__", "_")
                        .replace("__", "_")
                        .trim_end_matches('_')
                        .trim_start_matches('_')
                        .to_lowercase()
                        .parse()
                        .unwrap();
                    (_ty, __ty)
                })
                .collect::<Vec<(TokenStream, TokenStream)>>();

            // Build the schema variable reference for each child dependency.
            // For simple non-generic types: "RoleSchema"
            // For generic instantiations like Point<Gender>:
            //   "{ ...PointSchema, \"generics\": { \"T\": GenderSchema } }"
            let def_dependencies = dependencies.clone().into_iter().map(|(ty, _ty)| {
                if _ty.to_token_stream().to_string().replace(" ", "") == _o_name {
                    // Self-reference: emit empty string placeholder (handled in repl)
                    quote! {}
                } else {
                    let type_str = _ty.to_token_stream().to_string().replace(" ", "");

                    // Extract generic arguments from the type string e.g. "Point<Gender>" -> ["Gender"]
                    let child_generic_params: Vec<String> = {
                        let mut params = Vec::new();
                        if let Some(start_idx) = type_str.find('<') {
                            if let Some(end_idx) = type_str.rfind('>') {
                                let content = &type_str[start_idx + 1..end_idx];
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
                                        if !current_param.is_empty() {
                                            params.push(current_param.trim().to_string());
                                            current_param = String::new();
                                        }
                                    } else {
                                        current_param.push(c);
                                    }
                                }
                                if !current_param.is_empty() {
                                    params.push(current_param.trim().to_string());
                                }
                            }
                        }
                        params
                    };

                    if child_generic_params.is_empty() {
                        // Simple type: use schema_var_name() for exportable types (custom structs/enums),
                        // and schema(false) for primitives (no output file).
                        quote! {
                            let #ty: String = if <#_ty as #crate_rename::TS>::output_path().is_some() {
                                <#_ty as #crate_rename::TS>::schema_var_name()
                            } else {
                                <#_ty as #crate_rename::TS>::schema(false)
                            };
                        }
                    } else {
                        // Generic instantiation: build spread expression with overridden generics.
                        // We need the generic param *names* of the base type (e.g. "T" for Point<T>).
                        // At macro time we can't easily get them, but at runtime we can use a helper.
                        // Instead, we emit code that at runtime:
                        //   1. Gets the base type schema var name (e.g. "PointSchema")
                        //   2. Gets each concrete type's schema var name (e.g. "GenderSchema")
                        //   3. Combines them into a spread expression.
                        //
                        // We use a Vec of (param_index, concrete_schema_var) pairs and the base
                        // type's generic names via schema output (we can parse them at runtime from
                        // the base schema). Instead, we use a simpler approach: output a JavaScript
                        // spread with the generics as an indexed array that the runtime maps to names.
                        //
                        // Simplest approach: emit the concrete schema var names as an ordered list
                        // and generate a JS spread that overrides "generics" at runtime using the
                        // base type's schema to get param names.

                        // Parse the base type (without generics) to get schema var name
                        // We need to construct the base type token stream without generics.
                        // Use _ty to get ident() at runtime.
                        let concrete_schema_vars: Vec<TokenStream> = child_generic_params.iter().map(|param| {
                            // param is a type name like "Gender", "u64", etc.
                            // Use schema_var_name() for exportable types (custom structs/enums),
                            // and schema(false) for primitive types (no output file).
                            let param_ty: TokenStream = param.parse().unwrap();
                            quote! {
                                if <#param_ty as #crate_rename::TS>::output_path().is_some() {
                                    <#param_ty as #crate_rename::TS>::schema_var_name()
                                } else {
                                    <#param_ty as #crate_rename::TS>::schema(false)
                                }
                            }
                        }).collect();

                        // Get generic param *names* from the base schema at runtime.
                        // The base schema's "generics" section has the param names as keys.
                        // We need to call the base type (without generic args) schema.
                        // At macro time we have `_ty` = `Point<Gender>`. We need just `Point`.
                        // We use <#_ty as TS>::schema_var_name() which gives "PointSchema" (base ident),
                        // then read the base schema to get generic param names.
                        //
                        // Actually: let's emit a helper that builds the spread at runtime.
                        // The base type's schema is called with no generics (dummy), but we can
                        // use the generic names directly since we know them from the macro context.
                        //
                        // The generic param names of #_ty (e.g. Point<T>) are the type param names
                        // of the base Rust type. We get them at runtime by parsing the base schema's
                        // "generics" object keys.

                        quote! {
                            let #ty: String = {
                                // Get the base schema variable name (e.g. "PointSchema")
                                let base_schema_var = <#_ty as #crate_rename::TS>::schema_var_name();
                                // Get the base (non-instantiated) schema string to extract generic param names
                                // We call schema(false) on the concrete instantiation... but we need the
                                // param names from the generic base. Use ident() to get the base name.
                                // For now, emit a spread that carries schema_var_name for each concrete type.
                                let concrete_vars: Vec<String> = vec![
                                    #(#concrete_schema_vars),*
                                ];
                                // Build: { ...PointSchema, "generics": { "T": GenderSchema, ... } }
                                // But we need the generic param names. Get them by parsing the
                                // generic base schema. We use a helper: schema_generic_param_names.
                                let base_schema_str = {
                                    // Call schema(false) with dummy generics to get the base schema structure
                                    // This is available as the non-instantiated type's schema
                                    <#_ty as #crate_rename::TS>::schema(false)
                                };
                                // Extract the generic parameter names from the base schema's top-level
                                // "generics" section (use rfind to skip nested "generics" in definitions).
                                let param_names = {
                                    let mut names = Vec::new();
                                    if let Some(gen_start) = base_schema_str.rfind("\"generics\"") {
                                        if let Some(brace_start) = base_schema_str[gen_start..].find('{') {
                                            let after_brace = &base_schema_str[gen_start + brace_start + 1..];
                                            // Count nested braces to find the closing } of the generics object
                                            let mut depth = 1usize;
                                            let mut brace_end = 0usize;
                                            for (i, ch) in after_brace.char_indices() {
                                                match ch {
                                                    '{' => depth += 1,
                                                    '}' => {
                                                        depth -= 1;
                                                        if depth == 0 {
                                                            brace_end = i;
                                                            break;
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            let gen_content = &after_brace[..brace_end];
                                            // Extract keys: split by comma at depth 0
                                            let mut entry_depth = 0usize;
                                            let mut entry = String::new();
                                            for ch in gen_content.chars() {
                                                match ch {
                                                    '{' => { entry_depth += 1; entry.push(ch); }
                                                    '}' => { entry_depth -= 1; entry.push(ch); }
                                                    ',' if entry_depth == 0 => {
                                                        let e = entry.trim().to_string();
                                                        if !e.is_empty() {
                                                            if let Some(ks) = e.find('"') {
                                                                let after_k = &e[ks + 1..];
                                                                if let Some(ke) = after_k.find('"') {
                                                                    let key = &after_k[..ke];
                                                                    if !key.is_empty() {
                                                                        names.push(key.to_string());
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        entry = String::new();
                                                    }
                                                    _ => { entry.push(ch); }
                                                }
                                            }
                                            // Handle last entry (no trailing comma)
                                            let e = entry.trim().to_string();
                                            if !e.is_empty() {
                                                if let Some(ks) = e.find('"') {
                                                    let after_k = &e[ks + 1..];
                                                    if let Some(ke) = after_k.find('"') {
                                                        let key = &after_k[..ke];
                                                        if !key.is_empty() {
                                                            names.push(key.to_string());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    names
                                };
                                // Build the generics override object
                                let generics_obj = param_names.iter()
                                    .zip(concrete_vars.iter())
                                    .map(|(name, var)| format!("\"{}\": {}", name, var))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{{ ...{}, \"generics\": {{ {} }} }}", base_schema_var, generics_obj)
                            };
                        }
                    }
                }
            });

            let def_generics = _generics.type_params().map(|ty| {
                let _ty = ty.ident.to_string();
                let _ty = _ty.to_lowercase();
                let _ty: TokenStream = _ty.parse().unwrap();
                quote! {
                    let #_ty: String = if <#ty as #crate_rename::TS>::output_path().is_some() {
                        <#ty as #crate_rename::TS>::schema_var_name()
                    } else {
                        <#ty as #crate_rename::TS>::schema(false)
                    };
                }
            });
            let repl_dependencies = dependencies.into_iter().map(|(ty, _ty)| {
                if _ty.to_token_stream().to_string().replace(" ", "") == _o_name {
                    let fmt_def: String =
                        format!("#/definitions/{}", _ty.to_token_stream().to_string().replace(" ", ""));
                    let fmt: String =
                        format!("&&&{}&&&", ty.to_token_stream().to_string().to_uppercase());
                    quote! {
                        let schem = schem.replace(#fmt_def, "#");
                        let schem = schem.replace(#fmt, "{}");
                    }
                } else {
                    let fmt: String =
                        format!("&&&{}&&&", ty.to_token_stream().to_string().to_uppercase());
                    quote! {
                        let schem = schem.replace(#fmt, &#ty);
                    }
                }
            });
            let repl_generics = _generics.type_params().map(|ty| {
                let ty_ident_string = ty.ident.to_string();
                let _ty = ty_ident_string.to_lowercase();
                let _ty: TokenStream = _ty.parse().unwrap();
                let fmt: String = format!("&&&&{}&&&&", ty_ident_string);
                quote! {
                    let schem = schem.replace(#fmt, &#_ty);
                }
            });
            return quote! {
                fn schema(export: bool) -> String {
                    #(#def_dependencies)*
                    #(#def_generics)*
                    let mut schem = "".to_string();
                    if (export) {
                        schem = format!("const {} = {}", #name, #schema);
                    } else {
                        schem = format!("{}", #schema);
                    }
                    #(#repl_dependencies)*
                    #(#repl_generics)*
                    schem
                }
            };
        } else {
            return quote! {
                fn schema(export: bool) -> String {
                    if (export) {
                        format!("const {} = {}", #name, "{}")
                    } else {
                        format!("{}", "{}")
                    }
                }
            };
        };
    }

    fn generate_name_fn(&self, generics: &Generics) -> TokenStream {
        let name = self.name_with_generics(generics);
        quote! {
            fn name() -> String {
                #name
            }
        }
    }

    fn generate_inline_fn(&self) -> TokenStream {
        let inline = &self.inline;
        let crate_rename = &self.crate_rename;

        let inline_flattened = self.inline_flattened.as_ref().map_or_else(
            || {
                quote! {
                    fn inline_flattened() -> String {
                        panic!("{} cannot be flattened", <Self as #crate_rename::TS>::name())
                    }
                }
            },
            |inline_flattened| {
                quote! {
                    fn inline_flattened() -> String {
                        #inline_flattened
                    }
                }
            },
        );
        let inline = quote! {
            fn inline() -> String {
                #inline
            }
        };
        quote! {
            #inline
            #inline_flattened
        }
    }

    /// Generates the `decl()` and `decl_concrete()` methods.
    /// `decl_concrete()` is simple, and simply defers to `inline()`.
    /// For `decl()`, however, we need to change out the generic parameters of the type, replacing
    /// them with the dummy types generated by `generate_generic_types()`.
    fn generate_decl_fn(&mut self, rust_ty: &Ident, generics: &Generics) -> TokenStream {
        let name = &self.ts_name;
        let crate_rename = &self.crate_rename;
        let generic_types = self.generate_generic_types(generics);
        let ts_generics = format_generics(
            &mut self.dependencies,
            crate_rename,
            generics,
            &self.concrete,
        );

        use GenericParam as G;
        // These are the generic parameters we'll be using.
        let generic_idents = generics.params.iter().filter_map(|p| match p {
            G::Lifetime(_) => None,
            G::Type(TypeParam { ident, .. }) => match self.concrete.get(ident) {
                // Since we named our dummy types the same as the generic parameters, we can just keep
                // the identifier of the generic parameter - its name is shadowed by the dummy struct.
                None => Some(quote!(#ident)),
                // If the type parameter is concrete, we use the type the user provided using
                // `#[ts(concrete)]`
                Some(concrete) => Some(quote!(#concrete)),
            },
            // We keep const parameters as they are, since there's no sensible default value we can
            // use instead. This might be something to change in the future.
            G::Const(ConstParam { ident, .. }) => Some(quote!(#ident)),
        });
        quote! {
            fn decl_concrete() -> String {
                format!("type {} = {};", #name, <Self as #crate_rename::TS>::inline())
            }
            fn decl() -> String {
                #generic_types
                let inline = <#rust_ty<#(#generic_idents,)*> as #crate_rename::TS>::inline();
                let generics = #ts_generics;
                format!("type {}{generics} = {inline};", #name)
            }
        }
    }
}

fn generate_assoc_type(
    rust_ty: &Ident,
    crate_rename: &Path,
    generics: &Generics,
    concrete: &HashMap<Ident, Type>,
) -> TokenStream {
    use GenericParam as G;

    let generics_params = generics.params.iter().map(|x| match x {
        G::Type(ty) => match concrete.get(&ty.ident) {
            None => quote! { #crate_rename::Dummy },
            Some(ty) => quote! { #ty },
        },
        G::Const(ConstParam { ident, .. }) => quote! { #ident },
        G::Lifetime(LifetimeParam { lifetime, .. }) => quote! { #lifetime },
    });

    quote! { type WithoutGenerics = #rust_ty<#(#generics_params),*>; }
}

// generate start of the `impl TS for #ty` block, up to (excluding) the open brace
fn generate_impl_block_header(
    crate_rename: &Path,
    ty: &Ident,
    generics: &Generics,
    bounds: Option<&[WherePredicate]>,
    dependencies: &Dependencies,
) -> TokenStream {
    use GenericParam as G;

    let params = generics.params.iter().map(|param| match param {
        G::Type(TypeParam {
            ident,
            colon_token,
            bounds,
            ..
        }) => quote!(#ident #colon_token #bounds),
        G::Lifetime(LifetimeParam {
            lifetime,
            colon_token,
            bounds,
            ..
        }) => quote!(#lifetime #colon_token #bounds),
        G::Const(ConstParam {
            const_token,
            ident,
            colon_token,
            ty,
            ..
        }) => quote!(#const_token #ident #colon_token #ty),
    });
    let type_args = generics.params.iter().map(|param| match param {
        G::Type(TypeParam { ident, .. }) | G::Const(ConstParam { ident, .. }) => quote!(#ident),
        G::Lifetime(LifetimeParam { lifetime, .. }) => quote!(#lifetime),
    });

    let where_bound = match bounds {
        Some(bounds) => quote! { where #(#bounds),* },
        None => {
            let bounds = generate_where_clause(crate_rename, generics, dependencies);
            quote! { #bounds }
        }
    };

    quote!(impl <#(#params),*> #crate_rename::TS for #ty <#(#type_args),*> #where_bound)
}

fn generate_where_clause(
    crate_rename: &Path,
    generics: &Generics,
    dependencies: &Dependencies,
) -> WhereClause {
    let used_types = {
        let is_type_param = |id: &Ident| generics.type_params().any(|p| &p.ident == id);

        let mut used_types = HashSet::new();
        for ty in dependencies.used_types() {
            used_type_params(&mut used_types, ty, is_type_param);
        }
        used_types.into_iter()
    };

    let existing = generics.where_clause.iter().flat_map(|w| &w.predicates);
    parse_quote! {
        where #(#existing,)* #(#used_types: #crate_rename::TS),*
    }
}

// Extracts all type parameters which are used within the given type.
// Associated types of a type parameter are extracted as well.
// Note: This will not extract `I` from `I::Item`, but just `I::Item`!
fn used_type_params<'ty, 'out>(
    out: &'out mut HashSet<&'ty Type>,
    ty: &'ty Type,
    is_type_param: impl Fn(&'ty Ident) -> bool + Copy + 'out,
) {
    use syn::{
        AngleBracketedGenericArguments as GenericArgs, GenericArgument as G, PathArguments as P,
    };

    match ty {
        Type::Array(TypeArray { elem, .. })
        | Type::Paren(TypeParen { elem, .. })
        | Type::Reference(TypeReference { elem, .. })
        | Type::Slice(TypeSlice { elem, .. }) => used_type_params(out, elem, is_type_param),
        Type::Tuple(TypeTuple { elems, .. }) => elems
            .iter()
            .for_each(|elem| used_type_params(out, elem, is_type_param)),
        Type::Path(TypePath { qself: None, path }) => {
            let first = path.segments.first().unwrap();
            if is_type_param(&first.ident) {
                // The type is either a generic parameter (e.g `T`), or an associated type of that
                // generic parameter (e.g `I::Item`). Either way, we return it.
                out.insert(ty);
                return;
            }

            let last = path.segments.last().unwrap();
            if let P::AngleBracketed(GenericArgs { ref args, .. }) = last.arguments {
                for generic in args {
                    if let G::Type(ty) = generic {
                        used_type_params(out, ty, is_type_param);
                    }
                }
            }
        }
        _ => (),
    }
}

/// Derives [TS](./trait.TS.html) for a struct or enum.
/// Please take a look at [TS](./trait.TS.html) for documentation.
#[proc_macro_derive(TS, attributes(ts))]
pub fn typescript(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match entry(input) {
        Err(err) => err.to_compile_error(),
        Ok(result) => result,
    }
    .into()
}

fn entry(input: proc_macro::TokenStream) -> Result<TokenStream> {
    let input = syn::parse::<Item>(input)?;
    let (ts, ident, generics) = match input {
        Item::Struct(s) => (types::struct_def(&s)?, s.ident, s.generics),
        Item::Enum(e) => (types::enum_def(&e)?, e.ident, e.generics),
        _ => syn_err!(input.span(); "unsupported item"),
    };

    Ok(ts.into_impl(ident, generics))
}
