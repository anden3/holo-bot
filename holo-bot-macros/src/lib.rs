extern crate proc_macro;

mod attributes;
mod consts;

#[macro_use]
mod structures;
#[macro_use]
mod util;

use quote::{quote, ToTokens};
use syn::{parse_macro_input, spanned::Spanned, Lit};

use attributes::*;
use consts::*;
use structures::*;
use util::*;

#[proc_macro_attribute]
pub fn interaction_cmd(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let mut options = InteractionOptions::new();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            required_permissions
        ]);
    }

    propagate_err!(create_declaration_validations(&mut fun, DeclarFor::Command));

    let name = fun.name.clone();
    let body = fun.body;

    let cooked = fun.cooked.clone();

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub fn #name<'fut> (#(#args),*) -> ::futures::future::BoxFuture<'fut, ::anyhow::Result<()>> {
            use ::futures::future::FutureExt;
            async move { #(#body)* }.boxed()
        }
    })
    .into()
}

#[proc_macro_attribute]
pub fn interaction_group(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let group = parse_macro_input!(input as GroupStruct);

    let name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        group.name.to_string()
    };

    let mut options = GroupOptions::new();

    for attribute in &group.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            required_permissions;
            commands;
            sub_groups
        ]);
    }

    let GroupOptions {
        required_permissions,
        commands,
        sub_groups,
    } = options;

    let cooked = group.cooked.clone();
    let n = group.name.with_suffix(INTERACTION_GROUP);

    let commands = commands
        .into_iter()
        .map(|c| c.with_suffix(INTERACTION))
        .collect::<Vec<_>>();

    let sub_groups = sub_groups
        .into_iter()
        .map(|c| c.with_suffix(INTERACTION_GROUP))
        .collect::<Vec<_>>();

    let options = group.name.with_suffix(INTERACTION_GROUP_OPTIONS);
    let options_path = quote!(interactions::InteractionGroupOptions);
    let group_path = quote!(interactions::InteractionGroup);

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            required_permissions: #required_permissions,
            commands: &[#(&#commands),*],
            sub_groups: &[#(&#sub_groups),*],
        };

        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #group_path = #group_path {
            name: #name,
            options: &#options,
        };

        #group
    })
    .into()
}

#[proc_macro]
pub fn interaction_setup(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let setup = parse_macro_input!(input as InteractionSetup);

    proc_macro::TokenStream::from(setup.into_token_stream())
}

#[proc_macro]
pub fn parse_interaction_options(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let params = parse_macro_input!(input as ParseInteractionOptions);

    let data = params.data;
    let options = params.options.iter();
    let declarations = params.options.iter().map(|o| o.declare_variable());

    let output = quote! {
        #(#declarations)*

        for option in &#data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    #(#options)*

                    _ => ::log::error!(
                        "Unknown option '{}' found for command '{}'.",
                        option.name,
                        file!()
                    ),
                }
            }
        }

    };

    output.into()
}
