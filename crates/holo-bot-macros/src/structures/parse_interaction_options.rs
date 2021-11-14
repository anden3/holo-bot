use super::prelude::*;

#[derive(Debug)]
pub struct ParseInteractionOptions {
    pub data: Expr,
    pub options: Vec<ParseInteractionOption>,
}

impl Parse for ParseInteractionOptions {
    fn parse(input: ParseStream) -> Result<Self> {
        let data = input.parse()?;
        input.parse::<Token![,]>()?;

        let content;
        bracketed!(content in input);

        let options = Punctuated::<ParseInteractionOption, Token![,]>::parse_terminated_with(
            &content,
            ParseInteractionOption::parse,
        )?
        .into_iter()
        .collect();

        Ok(Self { data, options })
    }
}

impl ToTokens for ParseInteractionOptions {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let data = &self.data;
        let options = self.options.iter();
        let declarations = self.options.iter().map(|o| o.declare_variable());

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

        output.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub struct ParseInteractionOption {
    name: String,
    ident: Ident,
    ty: Ident,
    is_enum: bool,
    is_required: bool,
    default: Option<Expr>,
}

impl ParseInteractionOption {
    pub fn declare_variable(&self) -> TokenStream2 {
        let ident = &self.ident;
        let ty = &self.ty;

        if let Some(d) = &self.default {
            quote! { let mut #ident: #ty = #d; }
        } else if self.is_required {
            quote! { let mut #ident: #ty = Default::default(); }
        } else {
            quote! { let mut #ident: Option<#ty> = None; }
        }
    }
}

impl Parse for ParseInteractionOption {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        let name = ident.to_string();

        input.parse::<Token![:]>()?;

        let (is_required, is_enum, ty) = {
            if input.peek(Token![enum]) {
                input.parse::<Token![enum]>()?;
                (true, true, input.parse()?)
            } else {
                let ident = input.parse::<Ident>()?;

                match ident.to_string().as_str() {
                    "Option" => {
                        input.parse::<Token![<]>()?;

                        let (ty, is_enum) = if input.peek(Token![enum]) {
                            input.parse::<Token![enum]>()?;
                            (input.parse::<Ident>()?, true)
                        } else {
                            (input.parse::<Ident>()?, false)
                        };

                        input.parse::<Token![>]>()?;

                        (false, is_enum, ty)
                    }
                    "req" => return Err(Error::new(ident.span(), "'req' is not a valid type.")),
                    _ => (true, false, ident),
                }
            }
        };

        let default;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            default = Some(input.parse()?);
        } else {
            default = None;
        }

        let is_required = is_required || default.is_some();

        Ok(Self {
            name,
            ident,
            ty,
            is_enum,
            is_required,
            default,
        })
    }
}

impl ToTokens for ParseInteractionOption {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let ident = &self.ident;
        let ty = &self.ty;

        let output = /* match self.is_enum {
            true => {
                if self.default.is_some() {
                    quote! {
                        #name => {
                            #ident = #ty::from_str(&::serde_json::from_value::<String>(value.clone()).context(::utility::here!())?).unwrap()
                        }
                    }
                } else {
                    quote! {
                        #name => {
                            #ident = #ty::from_str(&::serde_json::from_value::<String>(value.clone()).context(::utility::here!())?).ok()
                        }
                    }
                }
            }
            false => {
                 */if self.is_required || self.default.is_some() {
                    quote! {
                        #name => {
                            #ident = ::serde_json::from_value::<#ty>(value.clone()).context(::utility::here!())?
                        }
                    }
                } else {
                    quote! {
                        #name => {
                            #ident = Some(::serde_json::from_value::<#ty>(value.clone()).context(::utility::here!())?)
                        }
                    }
                /* }
            } */
        };

        output.to_tokens(tokens);
    }
}
