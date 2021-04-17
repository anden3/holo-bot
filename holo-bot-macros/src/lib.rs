extern crate proc_macro;

mod attributes;
mod consts;

#[macro_use]
mod structures;
#[macro_use]
mod util;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, spanned::Spanned, Lit};

use attributes::*;
use consts::*;
use structures::*;
use util::*;

#[proc_macro_attribute]
pub fn slash_command(attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let mut options = Options::new();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            checks;
            bucket;
            required_permissions;
            allowed_roles;
            owners_only;
            owner_privilege
        ]);
    }

    let Options {
        checks,
        bucket,
        allowed_roles,
        required_permissions,
        owners_only,
        owner_privilege,
    } = options;

    propagate_err!(create_declaration_validations(&mut fun, DeclarFor::Command));

    let res = parse_quote!(super::CommandResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let options = name.with_suffix(COMMAND_OPTIONS);
    let body = fun.body;
    let ret = fun.ret;

    let n = name.with_suffix(COMMAND);

    let cooked = fun.cooked.clone();

    let options_path = quote!(super::CommandOptions);
    let command_path = quote!(super::Command);

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            checks: #checks,
            bucket: #bucket,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #required_permissions,
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
        };

        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #command_path = #command_path {
            fun: #name,
            options: &#options,
        };

        #(#cooked)*
        #[allow(missing_docs)]
        #visibility fn #name<'fut> (#(#args),*) -> ::futures::future::BoxFuture<'fut, #ret> {
            use ::futures::future::FutureExt;
            async move { #(#body)* }.boxed()
        }
    })
    .into()
}
