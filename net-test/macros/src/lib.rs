use proc_macro::TokenStream;
use quote::quote;
use syn::{AttributeArgs, ItemFn};

#[proc_macro_attribute]
pub fn net_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = syn::parse_macro_input!(attr as AttributeArgs);
    let input_fn = syn::parse_macro_input!(item as ItemFn);

    // ensure async fn def
    input_fn.sig.asyncness.expect("net_test requires async fn");

    let mut config = vec![];
    for e in attr {
        match e {
            syn::NestedMeta::Meta(_) => {
                panic!("Only comma separated BeeGFS config arguments are allowed here")
            }
            syn::NestedMeta::Lit(l) => config.push(l),
        }
    }
    let name = input_fn.sig.ident;
    let code = input_fn.block;

    let res = quote! (
        #[test]
        fn #name() {
            // TODO for now, test target has to be switched here. make better.

            // common::run_test_internal(async move {#code});
            common::run_test_docker(async move {#code}, &[#(#config,)*]);
        }
    );

    res.into()
}
