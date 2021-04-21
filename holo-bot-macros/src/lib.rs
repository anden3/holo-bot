extern crate proc_macro;

mod attributes;
mod consts;

#[macro_use]
mod structures;
#[macro_use]
mod util;

use quote::{quote, ToTokens};
use syn::{parse_macro_input, parse_quote, spanned::Spanned, Lit};

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
            checks;
            required_permissions;
            allowed_roles;
            owners_only;
            owner_privilege
        ]);
    }

    let InteractionOptions {
        checks,
        allowed_roles,
        required_permissions,
        owners_only,
        owner_privilege,
    } = options;

    propagate_err!(create_declaration_validations(&mut fun, DeclarFor::Command));

    let options_path = quote!(super::interactions::InteractionOptions);
    let command_path = quote!(super::interactions::InteractionCmd);

    let res = parse_quote!(super::interactions::InteractionResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let options = name.with_suffix(INTERACTION_OPTIONS);
    let body = fun.body;
    let ret = fun.ret;

    let n = name.with_suffix(INTERACTION);

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
pub fn interaction_setup_fn(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let fn_type = quote!(super::interactions::InteractionSetupFn);
    let res = parse_quote!(super::interactions::InteractionSetupResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let body = fun.body;
    let ret = fun.ret;

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    let cooked = fun.cooked;
    let n = name.with_suffix(INTERACTION_SETUP);

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
    let n = group.name.with_suffix(INTERACTION_GROUP);

    let default_command = default_command.map(|ident| {
        let i = ident.with_suffix(INTERACTION);

        quote!(&#i)
    });

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

#[proc_macro]
pub fn interaction_setup(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let setup = parse_macro_input!(input as InteractionFields);

    let mut name = String::new();
    let mut description = String::new();
    let mut options = Vec::new();

    for field in setup {
        match field {
            InteractionField::Name(s) => name = s,
            InteractionField::Description(s) => description = s,
            InteractionField::Options(o) => options.extend(o),
        }
    }

    let option_choices = options.iter().map(|opt| opt.into_token_stream());

    let option_stream = match options.is_empty() {
        true => proc_macro2::TokenStream::new(),
        false => quote! { .create_interaction_option(|o| o #(
            #option_choices
        )*) },
    };

    let result = quote! {
        #[allow(missing_docs)]
        pub fn setup<'fut>(ctx: &'fut Ctx, guild: &'fut Guild, app_id: u64) -> ::futures::future::BoxFuture<'fut, anyhow::Result<ApplicationCommand>> {
            use ::futures::future::FutureExt;
            async move {
                let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
                    i.name(#name).description(#description)
                    #option_stream
                }).await
                .context(here!())?;

                Ok(cmd)
            }.boxed()
        }
    };

    proc_macro::TokenStream::from(result)
}

/* #[proc_macro_derive(InteractionOption)]
pub fn enum_interaction_option(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

    let data = match ast.data {
        Data::Enum(e) => e,
        Data::Struct(s) => {
            return Error::new(s.struct_token.span(), "Only works on enums.")
                .into_compile_error()
                .into();
        }
        Data::Union(u) => {
            return Error::new(u.union_token.span(), "Only works on enums.")
                .into_compile_error()
                .into();
        }
    };

    let type_name = ast.ident;

    let variants = data.variants.iter().map(|v| &v.ident);
    let names = variants.clone().map(|i| i.to_string());

    let tokens = quote! {
        #(
            .add_string_choice(#names, #type_name::#variants.to_string())
        )*
        "Hololive JP": HoloBranch::HoloJP.to_string(),
    };

    tokens.into()
} */
