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

    let mut options = SlashOptions::new();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            checks;
            required_permissions;
            allowed_roles;
            owners_only;
            owner_privilege
        ]);
    }

    let SlashOptions {
        checks,
        allowed_roles,
        required_permissions,
        owners_only,
        owner_privilege,
    } = options;

    propagate_err!(create_declaration_validations(&mut fun, DeclarFor::Command));

    let options_path = quote!(super::slash_types::SlashCommandOptions);
    let command_path = quote!(super::slash_types::SlashCommand);

    let res = parse_quote!(super::slash_types::SlashCommandResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let options = name.with_suffix(SLASH_COMMAND_OPTIONS);
    let body = fun.body;
    let ret = fun.ret;

    let n = name.with_suffix(SLASH_COMMAND);

    let cooked = fun.cooked.clone();

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    let name_str = name.to_string();

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            checks: #checks,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #required_permissions,
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
        };

        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #command_path = #command_path {
            name: #name_str,
            fun: #name,
            setup: setup,
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

#[proc_macro_attribute]
pub fn slash_setup(attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let fn_type = quote!(super::slash_types::SlashCommandSetupFn);
    let res = parse_quote!(super::slash_types::SlashCommandSetupResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let body = fun.body;
    let ret = fun.ret;

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    let cooked = fun.cooked;
    let n = name.with_suffix(SLASH_SETUP);

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #fn_type = #name;

        #(#cooked)*
        #[allow(missing_docs)]
        #visibility fn #name<'fut> (#(#args),*) -> ::futures::future::BoxFuture<'fut, #ret> {
            use ::futures::future::FutureExt;
            async move { #(#body)* }.boxed()
        }
    })
    .into()
}

#[proc_macro_attribute]
pub fn slash_group(attr: TokenStream, input: TokenStream) -> TokenStream {
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
            owners_only;
            owner_privilege;
            allowed_roles;
            required_permissions;
            checks;
            default_command;
            commands;
            sub_groups
        ]);
    }

    let GroupOptions {
        owners_only,
        owner_privilege,
        allowed_roles,
        required_permissions,
        checks,
        default_command,
        commands,
        sub_groups,
    } = options;

    let cooked = group.cooked.clone();
    let n = group.name.with_suffix(SLASH_GROUP);

    let default_command = default_command.map(|ident| {
        let i = ident.with_suffix(SLASH_COMMAND);

        quote!(&#i)
    });

    let commands = commands
        .into_iter()
        .map(|c| c.with_suffix(SLASH_COMMAND))
        .collect::<Vec<_>>();

    let sub_groups = sub_groups
        .into_iter()
        .map(|c| c.with_suffix(SLASH_GROUP))
        .collect::<Vec<_>>();

    let options = group.name.with_suffix(SLASH_GROUP_OPTIONS);
    let options_path = quote!(slash_types::SlashGroupOptions);
    let group_path = quote!(slash_types::SlashCommandGroup);

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #required_permissions,
            checks: #checks,
            default_command: #default_command,
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
