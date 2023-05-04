use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Field, Index, Type};

#[proc_macro_derive(BeeSerde, attributes(bee_serde))]
pub fn derive_bee_serialize(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
    let (ser, des) = process_data(&input.data);

    let expanded = quote! {
        impl bee_serde::BeeSerde for #name {
            fn serialize(&self, ser: &mut bee_serde::Serializer<'_>) -> anyhow::Result<()> {
                #ser
                Ok(())
            }

            fn deserialize(des: &mut bee_serde::Deserializer<'_>) -> anyhow::Result<Self> {
                #des
            }
        }
    };

    expanded.into()
}

fn process_data(data: &Data) -> (TokenStream, TokenStream) {
    match *data {
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
            syn::Fields::Unit => unimplemented!(),
        },
        Data::Enum(_) => unimplemented!(),
        Data::Union(_) => unimplemented!(),
    }
}

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

fn build_field_actions(field: &Field, index: usize) -> (TokenStream, TokenStream) {
    let mut serde_as: Option<Type> = None;
    let target_type = field.ty.clone();
    let name = &field.ident;

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

    let (ser, des) = if let Some(serde_as) = serde_as {
        (
            quote! {
                <#serde_as>::serialize_as(&#field, ser)?;
            },
            quote! {
                <#serde_as>::deserialize_as(des)?,
            },
        )
    } else {
        (
            quote! {
                bee_serde::BeeSerde::serialize(&#field, ser)?;
            },
            quote! {
                <#target_type as bee_serde::BeeSerde>::deserialize(des)?,
            },
        )
    };

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
