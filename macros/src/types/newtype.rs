use quote::{quote, ToTokens};
use syn::{FieldsUnnamed, Result};

use crate::{
    attr::{Attr, ContainerAttr, FieldAttr, StructAttr},
    deps::Dependencies,
    schem::Schema,
    utils::raw_name_to_ts_field,
    DerivedTS,
};

pub(crate) fn newtype(attr: &StructAttr, name: &str, fields: &FieldsUnnamed) -> Result<DerivedTS> {
    let mut schema = Schema::new(name.to_string(), crate::schem::SchemaType::Struct);
    let inner = fields.unnamed.first().unwrap();

    let field_attr = FieldAttr::from_attrs(&inner.attrs)?;
    field_attr.assert_validity(inner)?;

    let crate_rename = attr.crate_rename();

    if field_attr.skip {
        return super::unit::null(attr, name);
    }

    let inner_ty = field_attr.type_as(&inner.ty);

    let mut dependencies = Dependencies::new(crate_rename.clone());

    let mut include_in_def = false;
    match (&field_attr.type_override, field_attr.inline) {
        (Some(_), _) => (),
        (None, true) => dependencies.append_from(&inner_ty),
        (None, false) => {
            include_in_def = true;
            dependencies.push(&inner_ty)
        }
    };

    schema.add_field("0".to_string(), &inner_ty, include_in_def);

    let inline_def = match field_attr.type_override {
        Some(ref o) => quote!(#o.to_owned()),
        None if field_attr.inline => quote!(<#inner_ty as #crate_rename::TS>::inline()),
        None => quote!(<#inner_ty as #crate_rename::TS>::name()),
    };

    Ok(DerivedTS {
        crate_rename,
        inline: inline_def,
        inline_flattened: None,
        docs: attr.docs.clone(),
        dependencies,
        export: attr.export,
        export_to: attr.export_to.clone(),
        ts_name: name.to_owned(),
        concrete: attr.concrete.clone(),
        bound: attr.bound.clone(),
        schema: Some(schema),
    })
}
