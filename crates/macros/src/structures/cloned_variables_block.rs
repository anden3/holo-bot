use super::prelude::*;

#[derive(Debug)]
pub struct ClonedVariablesBlock {
    variables: Vec<ClonedVariable>,
    body: Vec<Stmt>,
}

impl Parse for ClonedVariablesBlock {
    fn parse(input: ParseStream) -> Result<Self> {
        let variables = Punctuated::<ClonedVariable, Token![,]>::parse_separated_nonempty(input)?
            .into_iter()
            .collect();

        input.parse::<Token![;]>()?;

        let body = if input.peek(token::Brace) {
            input.parse::<Block>()?.stmts
        } else {
            vec![input.parse::<Stmt>()?]
        };

        Ok(Self { variables, body })
    }
}

impl ToTokens for ClonedVariablesBlock {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let vars = &self.variables;
        let body = &self.body;

        let output = quote! {{
            #( #vars )*
            async move { #( #body )*}
        }};

        output.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub struct ClonedVariable {
    name: Ident,
    mutable: bool,
}

impl Parse for ClonedVariable {
    fn parse(input: ParseStream) -> Result<Self> {
        let mutable = if input.peek(Token![mut]) {
            input.parse::<Token![mut]>()?;
            true
        } else {
            false
        };

        let name = input.parse::<Ident>()?;

        Ok(Self { name, mutable })
    }
}

impl ToTokens for ClonedVariable {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;

        let output = if self.mutable {
            quote! { let mut #name = #name.clone(); }
        } else {
            quote! { let #name = #name.clone(); }
        };

        output.to_tokens(tokens);
    }
}
