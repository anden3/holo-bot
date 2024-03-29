use syn::ext::IdentExt;

use super::prelude::*;

wrap_vectors!(
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
    pub enum_type: Option<Type>,
}

impl InteractionOpt {
    pub fn to_json_tokens(&self) -> TokenStream2 {
        let ty = &self.ty;
        let desc = &self.desc;
        let req = self.required;

        let choices_array;

        if let Some(enum_type) = &self.enum_type {
            choices_array = quote! {
                #enum_type::iter().map(|e| ::serde_json::json!({
                    "name": e.to_string(),
                    "value": e.to_string()
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
            if current.enum_type.is_some() {
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

        let required;

        if input.peek(Ident) && input.peek2(Ident) {
            match input.parse::<Ident>() {
                Ok(ident) => match ident.to_string().as_str() {
                    "req" => required = true,
                    _ => return Err(Error::new(ident.span(), "Only valid modifier is `req`.")),
                },
                Err(e) => return Err(e),
            }
        } else {
            required = false;
        }

        let mut names = vec![input.parse()?];

        if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;
            names.extend(Punctuated::<Ident, Token![|]>::parse_separated_nonempty(
                input,
            )?);
        }

        let names = names.iter().map(|i| i.unraw()).collect();

        input.parse::<Token![:]>()?;

        let ty = input.parse::<syn::Type>()?;
        let ty = match ty {
            Type::Path(p) => match p.path.get_ident() {
                Some(ident) => match ident.to_string().as_str() {
                    "String" | "Integer" | "Boolean" | "User" | "Channel" | "Role"
                    | "Mentionable" | "SubCommand" | "SubCommandGroup" => Ok(ident.to_owned()),
                    _ => Err(Error::new(p.span(), "Type not supported.")),
                },
                None => Err(Error::new(p.span(), "Not supported.")),
            },
            _ => Err(Error::new(ty.span(), "Not supported.")),
        }?;

        let mut choices = Vec::new();
        let mut options = Vec::new();

        let mut enum_type = None;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;

            if input.peek(Token![enum]) {
                input.parse::<Token![enum]>()?;
                enum_type = Some(input.parse::<Type>()?);
            } else {
                let content;
                bracketed!(content in input);

                match ty.to_string().as_str() {
                    "String" | "Integer" => {
                        choices =
                            Punctuated::<InteractionOptChoice, Token![,]>::parse_terminated_with(
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
            }
        }

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(InteractionOpt {
            required,
            names,
            desc,
            ty,
            choices,
            options,
            enum_type,
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
