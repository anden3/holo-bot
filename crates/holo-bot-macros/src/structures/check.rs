use super::prelude::*;

#[derive(Debug)]
pub struct Check {
    name: String,
    func: Expr,
}

impl Parse for Check {
    fn parse(input: ParseStream) -> Result<Self> {
        static CHECK_ARGS: &[&str] = &["ctx", "request", "interaction"];

        let name = input.parse::<Ident>()?.to_string();
        input.parse::<Token![:]>()?;

        let closure = input.parse::<ExprClosure>()?;

        let mut errors = Vec::new();

        if closure.inputs.len() != 3 {
            errors.push(Error::new(
                closure.span(),
                "3 arguments are required: `ctx`, `request`, and `interaction`.",
            ));
        }

        for (j, input) in closure.inputs.iter().enumerate().take(3) {
            match input {
                Pat::Ident(i) => {
                    if i.ident != CHECK_ARGS[j] {
                        errors.push(Error::new(
                            i.span(),
                            format!("Expected {}, got {:?}", CHECK_ARGS[j], i.ident),
                        ));
                    }
                }
                Pat::Path(p) => {
                    if !p.path.is_ident(CHECK_ARGS[j]) {
                        errors.push(Error::new(
                            p.span(),
                            format!("Expected {}, got {:?}", CHECK_ARGS[j], p.path),
                        ));
                    }
                }
                _ => errors.push(Error::new(input.span(), "Invalid argument to check.")),
            }
        }

        if !errors.is_empty() {
            let error = errors
                .into_iter()
                .reduce(|mut a, b| {
                    a.combine(b);
                    a
                })
                .unwrap();
            return Err(error);
        }

        Ok(Self {
            name,
            func: *closure.body,
        })
    }
}

impl ToTokens for Check {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let func = &self.func;

        let result = quote! {
            Check {
                name: #name,
                function: |ctx: &Ctx, request: &Interaction, interaction: &RegisteredInteraction| #func,
            },
        };

        result.to_tokens(tokens);
    }
}
