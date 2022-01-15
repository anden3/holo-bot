use crate::{
    attributes::parse_values,
    structures::InteractionOptions,
    util::{self, populate_fut_lifetimes_on_refs, Argument, Parenthesised},
};

use super::prelude::*;

#[derive(Debug)]
pub struct CommandFun {
    /// `#[...]`-style attributes.
    pub attributes: Vec<Attribute>,
    /// Populated cooked attributes. These are attributes outside of the realm of this crate's procedural macros
    /// and will appear in generated output.
    pub cooked: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: Ident,
    pub args: Vec<Argument>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

impl Parse for CommandFun {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;

        // Rename documentation comment attributes (`#[doc = "..."]`) to `#[description = "..."]`.
        util::rename_attributes(&mut attributes, "doc", "description");

        let cooked = util::remove_cooked(&mut attributes);
        let visibility = input.parse::<Visibility>()?;

        input.parse::<Token![async]>()?;
        input.parse::<Token![fn]>()?;

        let name = input.parse()?;

        // (...)
        let Parenthesised(args) = input.parse::<Parenthesised<FnArg>>()?;

        let ret = match input.parse::<ReturnType>()? {
            ReturnType::Type(_, t) => (*t).clone(),
            ReturnType::Default => {
                return Err(input
                    .error("expected a result type of either `CommandResult` or `CheckResult`"))
            }
        };

        // { ... }
        let bcont;
        braced!(bcont in input);
        let body = bcont.call(Block::parse_within)?;

        let args = args
            .into_iter()
            .map(util::parse_argument)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            attributes,
            cooked,
            visibility,
            name,
            args,
            ret,
            body,
        })
    }
}

impl ToTokens for CommandFun {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        /* let _name = if !attr.is_empty() {
            parse_macro_input!(attr as Lit).to_str()
        } else {
            fun.name.to_string()
        }; */

        let mut options = InteractionOptions::new();

        for attribute in &self.attributes {
            let span = attribute.span();
            let values = propagate_err!(tokens, parse_values(attribute));

            let name = values.name.to_string();
            let name = &name[..];

            match_options!(tokens, name, values, options, span => [
                required_permissions
            ]);
        }

        let name = self.name.clone();
        let body = &self.body;

        let cooked = self.cooked.clone();

        let args = populate_fut_lifetimes_on_refs(&self.args);

        let output = quote! {
            #(#cooked)*
            #[instrument(skip(ctx, config))]
            #[allow(missing_docs)]
            pub fn #name<'fut> (#(#[allow(unused_variables)] #args),*) -> ::futures::future::BoxFuture<'fut, ::anyhow::Result<()>> {
                use ::futures::future::FutureExt;
                async move { #(#body)* }.boxed()
            }
        };

        output.to_tokens(tokens);
    }
}
