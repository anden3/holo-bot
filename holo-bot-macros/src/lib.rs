extern crate proc_macro;

mod attributes;
mod consts;

#[macro_use]
mod structures;
#[macro_use]
mod util;

use std::ops::Deref;

use quote::quote;
use syn::{
    bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
    Ident, Lit, LitStr, Token, Type,
};

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

#[derive(Debug)]
struct InteractionFields(Vec<InteractionField>);

impl Parse for InteractionFields {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let mut fields = Vec::new();

        while let Ok(opt) = input.parse::<InteractionField>() {
            fields.push(opt);
        }

        Ok(Self(fields))
    }
}

impl Deref for InteractionFields {
    type Target = Vec<InteractionField>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::iter::IntoIterator for InteractionFields {
    type Item = InteractionField;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug)]
enum InteractionField {
    Name(LitStr),
    Description(LitStr),
    Options(InteractionOpts),
}

impl Parse for InteractionField {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let label: Ident = input.parse()?;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
        } else {
            return Err(syn::Error::new(label.span(), "No value set for field!"));
        }

        let value = match label.to_string().as_str() {
            "name" => Ok(InteractionField::Name(input.parse::<LitStr>()?)),
            "desc" | "description" => Ok(InteractionField::Description(input.parse::<LitStr>()?)),
            "opts" | "options" => Ok(InteractionField::Options(input.parse::<InteractionOpts>()?)),
            _ => Err(syn::Error::new(label.span(), "Unknown field!")),
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        value
    }
}

#[derive(Debug)]
struct InteractionOpts(Vec<InteractionOpt>);

impl Parse for InteractionOpts {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let content;
        bracketed!(content in input);

        let mut opts = Vec::new();

        while let Ok(opt) = content.parse::<InteractionOpt>() {
            opts.push(opt);
        }

        Ok(Self(opts))
    }
}

impl Deref for InteractionOpts {
    type Target = Vec<InteractionOpt>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::iter::IntoIterator for InteractionOpts {
    type Item = InteractionOpt;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug)]
struct InteractionOpt {
    required: bool,
    name: Ident,
    desc: LitStr,
    ty: Ident,
    opts: Vec<Lit>,
    ending: Option<Token![,]>,
}
/* impl quote::ToTokens for InteractionOpt {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        tokens.append(Punct::new('#', Spacing::Joint));
        tokens.append(Punct::new('!', Spacing::Alone));

        let mut doc = TokenStream::new();
        doc.append(proc_macro2::Ident::new("doc", Span::call_site()));
        doc.append(Punct::new('=', Spacing::Alone));
        doc.append(proc_macro2::Literal::string(&self.desc.value()));

        tokens.append(Group::new(Delimiter::Bracket, doc));

        if self.required {
            tokens.append(proc_macro2::Ident::new("req", Span::call_site()));
        }

        tokens.append(proc_macro2::Ident::new(
            &self.name.to_string(),
            Span::call_site(),
        ));
        tokens.append(Punct::new(':', Spacing::Alone));
        self.ty.to_tokens(tokens);

        if !self.opts.is_empty() {
            tokens.append(Punct::new('=', Spacing::Alone));

            let mut opts = TokenStream::new();
            opts.append_separated(&self.opts, Punct::new(',', Spacing::Alone));

            tokens.append(Group::new(Delimiter::Bracket, opts));
        }

        if let Some(token) = self.ending {
            token.to_tokens(tokens);
        }
    }
} */

impl Parse for InteractionOpt {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        input.parse::<Token![#]>()?;
        input.parse::<Token![!]>()?;

        let doc;
        bracketed!(doc in input);

        doc.parse::<Ident>()?;
        doc.parse::<Token![=]>()?;
        let desc = doc.parse::<LitStr>()?;

        let mut required = false;

        if input.peek(Ident) && input.peek2(Ident) {
            match input.parse::<Ident>() {
                Ok(ident) => match ident.to_string().as_str() {
                    "req" => required = true,
                    _ => {
                        return Err(syn::Error::new(
                            ident.span(),
                            "Only valid modifier is `req`.",
                        ))
                    }
                },
                Err(e) => return Err(e),
            }
        }

        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;

        let ty = input.parse::<syn::Type>()?;
        let ty = match ty {
            Type::Path(p) => {
                /*  let ident = */
                match p.path.get_ident() {
                    Some(ident) => ident.to_owned(),
                    None => {
                        return Err(syn::Error::new(
                            p.span(),
                            "Only `String` and `Integer` are supported.",
                        ))
                    }
                }
            }
            _ => {
                return Err(syn::Error::new(
                    ty.span(),
                    "Only `String` and `Integer` are supported.",
                ))
            }
        };

        let mut opts = Vec::new();

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;

            let content;
            bracketed!(content in input);
            opts = Punctuated::<Lit, Token![,]>::parse_terminated_with(&content, Lit::parse)?
                .into_iter()
                .collect();
        }

        let mut ending = None;

        if input.peek(Token![,]) {
            ending = Some(input.parse::<Token![,]>()?);
        }

        Ok(InteractionOpt {
            required,
            name,
            desc,
            ty,
            opts,
            ending,
        })
    }
}

///```rs
///interaction_options! {
///    /// How many beans can you eat?
///    req beans: String = ["bean 1", "bean 2"];
///}
///```
#[proc_macro]
pub fn interaction_setup(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let setup = parse_macro_input!(input as InteractionFields);

    let mut name = String::new();
    let mut description = String::new();
    let mut options = Vec::new();

    for field in setup {
        match field {
            InteractionField::Name(s) => name = s.value(),
            InteractionField::Description(s) => description = s.value(),
            InteractionField::Options(o) => options.extend(o),
        }
    }

    let option_choices = options
        .iter()
        .map(|opt| {
            let opts = opt.opts.iter();

            let name = &opt.name.to_string();
            let desc = &opt.desc.value();
            let ty = &opt.ty;

            quote! {
                .create_interaction_option(|o|
                    o.name(#name)
                        .description(#desc)
                        .kind(::serenity::model::interactions::ApplicationCommandOptionType::#ty)
                        #(
                            .add_string_choice(#opts, #opts)
                        )*
                    )
            }
        })
        .collect::<Vec<_>>();

    let result = quote! {
        #[allow(missing_docs)]
        pub fn setup<'fut>(ctx: &'fut Ctx, guild: &'fut Guild, app_id: u64) -> ::futures::future::BoxFuture<'fut, anyhow::Result<ApplicationCommand>> {
            use ::futures::future::FutureExt;
            async move {
                let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
                    i.name(#name).description(#description)
                    #(
                        #option_choices
                    )*
                }).await
                .context(here!())?;

                Ok(cmd)
            }.boxed()
        }
    };

    proc_macro::TokenStream::from(result)
}
