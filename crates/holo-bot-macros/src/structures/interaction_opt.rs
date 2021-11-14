use syn::{ext::IdentExt, GenericArgument, PathArguments, PathSegment};

use super::prelude::*;

wrap_vectors!(
    braced,
    InteractionOpts | Vec<InteractionOpt>
);

#[derive(Debug)]
pub struct InteractionOpt {
    pub required: bool,
    pub names: Vec<Ident>,
    pub desc: String,
    pub ty: Ident,

    pub choices: Vec<InteractionOptChoice>,
    pub options: Vec<InteractionOpt>,
    pub iter_type: Option<Type>,
}

impl InteractionOpt {
    pub fn to_json_tokens(&self) -> TokenStream2 {
        let ty = &self.ty;
        let desc = &self.desc;
        let req = self.required;

        let choices_array;

        if let Some(iter_type) = &self.iter_type {
            choices_array = quote! {
                #iter_type::iter().map(|e| ::serde_json::json!({
                    "name": e,
                    "value": e
                })).collect::<Vec<_>>()
            };
        } else {
            let choices = self
                .choices
                .iter()
                .map(|c| c.to_json_tokens())
                .collect::<Vec<_>>();

            choices_array = quote! {[
                #({
                    #choices
                },)*
            ]};
        }

        let options = &self
            .options
            .iter()
            .map(|o| o.to_json_tokens())
            .collect::<Vec<_>>();

        let output = self.names.iter().map(|n| {
            let name = n.to_string();

            quote! {{
                "type": ::serenity::model::interactions::application_command::ApplicationCommandOptionType::#ty,
                "name": #name,
                "description": #desc,
                "required": #req,
                "choices": #choices_array,
                "options": [
                    #(#options)*
                ]
            },}
        }).collect();

        output
    }

    pub fn contains_enum_option(&self) -> bool {
        let mut remaining: VecDeque<&Self> = VecDeque::new();
        remaining.push_back(self);

        while let Some(current) = remaining.pop_front() {
            if current.iter_type.is_some() {
                return true;
            }

            remaining.extend(current.options.iter());
        }

        false
    }
}

impl Parse for InteractionOpt {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        if !input.peek(Token![#]) || !input.peek2(Token![!]) {
            return Err(Error::new(input.span(), "Missing description"));
        }

        input.parse::<Token![#]>()?;
        input.parse::<Token![!]>()?;

        let doc;
        bracketed!(doc in input);

        doc.parse::<Ident>()?;
        doc.parse::<Token![=]>()?;
        let desc = doc.parse::<LitStr>()?.value();

        let mut names = vec![input.parse()?];

        if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;
            names.extend(Punctuated::<Ident, Token![|]>::parse_separated_nonempty(
                input,
            )?);
        }

        let names = names.iter().map(|i| i.unraw()).collect();

        input.parse::<Token![:]>()?;

        // Unwrap optional Option.
        let (inner, required) = {
            let ty = input.parse::<syn::Type>()?;

            let p = match ty {
                Type::Path(ref p) => p,
                _ => return Err(Error::new(ty.span(), "Expected a path type")),
            };

            let PathSegment { ident, arguments } = match p.path.segments.first() {
                Some(seg) => seg,
                None => return Err(Error::new(p.path.span(), "Type not supported.")),
            };

            match ident.to_string().as_str() {
                "Option" => {
                    let generic_args = match arguments {
                        PathArguments::AngleBracketed(args) => &args.args,
                        _ => return Err(Error::new(arguments.span(), "Type not supported.")),
                    };

                    let generic_arg = match generic_args.len() {
                        1 => &generic_args[0],
                        _ => return Err(Error::new(generic_args.span(), "Too many args.")),
                    };

                    (generic_arg.to_owned(), false)
                }
                _ => (GenericArgument::Type(ty), true),
            }
        };

        let (ident, iter_type) = match inner {
            GenericArgument::Type(ref ty @ Type::Path(ref p)) => {
                match p.path.get_ident() {
                    Some(i) => match i.to_string().as_str() {
                        "String" | "Integer" | "Boolean" | "User" | "Channel" | "Role"
                        | "Mentionable" | "SubCommand" | "SubCommandGroup" => (i.to_owned(), None),

                        /* _ => return Err(Error::new(i.span(), "Type not supported.")), */
                        _ => (Ident::new("String", ty.span()), Some(ty.to_owned())),
                    },
                    /* None => return Err(Error::new(p.path.span(), "Type not supported.")), */
                    None => (Ident::new("String", ty.span()), Some(ty.to_owned())),
                }
            }
            _ => return Err(Error::new(inner.span(), "Type not supported.")),
        };

        let mut choices = Vec::new();
        let mut options = Vec::new();

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;

            let content;
            braced!(content in input);

            match ident.to_string().as_str() {
                "String" | "Integer" => {
                    choices = Punctuated::<InteractionOptChoice, Token![,]>::parse_terminated_with(
                        &content,
                        InteractionOptChoice::parse,
                    )?
                    .into_iter()
                    .collect();
                }
                "SubCommand" | "SubCommandGroup" => {
                    while !content.is_empty() {
                        options.push(content.parse::<InteractionOpt>()?);
                    }
                }
                _ => {
                    return Err(Error::new(
                        content.span(),
                        "Option type doesn't support choices.",
                    ))
                }
            }
            /* } */
        }

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(InteractionOpt {
            required,
            names,
            desc,
            ty: ident,
            choices,
            options,
            iter_type,
        })
    }
}

#[derive(Debug)]
pub struct InteractionOptChoice {
    name: Option<String>,
    value: Expr,
}

impl InteractionOptChoice {
    pub fn to_json_tokens(&self) -> TokenStream2 {
        let value = &self.value;

        let result = match &self.name {
            Some(name) => quote! {
                "name": #name,
                "value": #value
            },
            None => quote! {
                "name": #value.to_string(),
                "value": #value
            },
        };

        result.into_token_stream()
    }
}

impl Parse for InteractionOptChoice {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = if input.peek2(Token![:]) {
            let name = input.parse::<LitStr>()?.value();
            input.parse::<Token![:]>()?;
            Some(name)
        } else {
            None
        };

        let value = input.parse::<Expr>()?;
        let value = keep_syn_variants!(
            Expr,
            value,
            [Await, Binary, Call, Cast, Field, Group, Index, Lit, Macro, MethodCall, Paren],
            "Value must be either `String` or `Integer`"
        )?;

        Ok(Self { name, value })
    }
}

impl ToTokens for InteractionOptChoice {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let value = &self.value;

        name.to_tokens(tokens);
        tokens.append(Punct::new(',', Spacing::Alone));
        value.to_tokens(tokens);
    }
}
