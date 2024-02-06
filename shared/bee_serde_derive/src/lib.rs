//! Proc macro definitions for BeeGFS Rust

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Field, Index, Type};

/// Auto implement BeeGFS msg serialization and deserialization for a struct.
///
/// Raw integers are serialized automatically as themselves, other types like String, Vec and
/// Map need a hint. A hint is provided by annotating a struct member with `#[bee_serde(as =
/// HINT)]`. See `shared::bee_serde` for available options.
///
/// The order of the struct members determines the order of (de-)serialization.
///
/// # Example
/// ```ignore
/// #[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
/// pub struct AddStoragePool {
///     pub pool_id: StoragePoolID,
///     #[bee_serde(as = CStr<0>)]
///     pub alias: EntityAlias,
///     #[bee_serde(as = Seq<true, _>)]
///     pub move_target_ids: Vec<TargetID>,
///     #[bee_serde(as = Seq<true, _>)]
///     pub move_buddy_group_ids: Vec<BuddyGroupID>,
/// }
/// ```
#[proc_macro_derive(BeeSerde, attributes(bee_serde))]
pub fn derive_bee_serialize(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
    let (ser, des) = process_data(&input.data);

    // Create and output the actual impl block
    quote! {
        impl crate::bee_serde::Serializable for #name {
            fn serialize(&self, ser: &mut crate::bee_serde::Serializer<'_>) -> anyhow::Result<()> {
                #ser
                Ok(())
            }
        }

        impl crate::bee_serde::Deserializable for #name {
            fn deserialize(des: &mut crate::bee_serde::Deserializer<'_>) -> anyhow::Result<Self> {
                #des
            }
        }
    }
    .into()
}

/// Takes a struct body and forwards named or unnamed fields to [`iterate_fields()`]
fn process_data(data: &Data) -> (TokenStream, TokenStream) {
    match data {
        Data::Struct(ref data) => match data.fields {
            syn::Fields::Named(ref fields) => {
                let (ser, des) = iterate_fields(fields.named.iter());

                (
                    quote! {
                        #(#ser)*
                    },
                    quote! {
                        Ok(Self {
                            #(#des)*
                        })
                    },
                )
            }
            syn::Fields::Unnamed(ref fields) => {
                let (ser, des) = iterate_fields(fields.unnamed.iter());

                (
                    quote! {
                        #(#ser)*
                    },
                    quote! {
                        Ok(Self (#(#des)*))
                    },
                )
            }
            syn::Fields::Unit => unimplemented!("Unit structs are not supported"),
        },
        Data::Enum(_) => unimplemented!("Enums are not supported"),
        Data::Union(_) => unimplemented!("Unions are not supported"),
    }
}

/// Iterate over all struct fields and calls [`build_field_actions()`] on them
fn iterate_fields<'a>(
    fields: impl IntoIterator<Item = &'a Field>,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let mut ser = vec![];
    let mut des = vec![];

    for (i, f) in fields.into_iter().enumerate() {
        let (ser_action, des_action) = build_field_actions(f, i);

        ser.push(ser_action);
        des.push(des_action);
    }

    (ser, des)
}

/// Generate (de-)serialization action for a single struct field
fn build_field_actions(field: &Field, index: usize) -> (TokenStream, TokenStream) {
    let mut serde_as: Option<Type> = None;
    let target_type = field.ty.clone();
    let name = &field.ident;

    // Find the `#[bee_serde(as = AS)]` annotation, if given
    for a in &field.attrs {
        if a.path().is_ident("bee_serde") {
            a.parse_nested_meta(|meta| {
                if meta.path.is_ident("as") {
                    serde_as = Some(meta.value().unwrap().parse().unwrap());
                }

                Ok(())
            })
            .unwrap();
        }
    }

    // Named and unnamed structs are accessed differently by name or index
    let field = if let Some(name) = name {
        quote! {
            self.#name
        }
    } else {
        let index = Index::from(index);

        quote! {
            self.#index
        }
    };

    // If `#[bee_serde(as = AS)]` is given, use the provided serialization helper to (de-)serialize
    // the field
    let (ser, des) = if let Some(serde_as) = serde_as {
        (
            quote! {
                <#serde_as>::serialize_as(&#field, ser)?;
            },
            quote! {
                <#serde_as>::deserialize_as(des)?,
            },
        )
    // If not, call serialize directly using the fields type
    } else {
        (
            quote! {
                crate::bee_serde::Serializable::serialize(&#field, ser)?;
            },
            quote! {
                <#target_type as crate::bee_serde::Deserializable>::deserialize(des)?,
            },
        )
    };

    // Create the actual line calling some `serialize()` with the struct field or filling the struct
    // field using some `deserialize()`. Again, named and unnamed struct need different handling.
    if let Some(name) = name {
        (
            quote! {
                #ser
            },
            quote! {
                #name: #des
            },
        )
    } else {
        (
            quote! {
                #ser
            },
            quote! {
                #des
            },
        )
    }
}
