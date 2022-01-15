use syn::{PatType, Path, PathArguments, PathSegment, TypePath, TypeReference};

use super::prelude::*;

use super::{InteractionOpts, InteractionRestrictions};

#[derive(Debug)]
pub enum InteractionField {
    Name(String),
    Group(String),
    Description(String),
    Options(InteractionOpts),
    Restrictions(InteractionRestrictions),
    IsEnabled(ExprClosure),
}

impl Parse for InteractionField {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let label: Ident = input.parse()?;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
        } else if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
        } else {
            return Err(Error::new(label.span(), "No value set for field!"));
        }

        let value = match label.to_string().as_str() {
            "name" => Ok(InteractionField::Name(input.parse::<LitStr>()?.value())),
            "group" => Ok(InteractionField::Group(input.parse::<LitStr>()?.value())),
            "desc" | "description" => Ok(InteractionField::Description(
                input.parse::<LitStr>()?.value(),
            )),
            "opts" | "options" => Ok(InteractionField::Options(input.parse()?)),
            "restrictions" => Ok(InteractionField::Restrictions(input.parse()?)),
            "enabled" | "is_enabled" | "enable_if" | "enabled_if" => {
                let mut closure = input.parse::<ExprClosure>()?;

                // Make sure the right amount of arguments are given.
                match closure.inputs.len() {
                    1 => (),
                    0 => {
                        return Err(Error::new(
                            closure.inputs.span(),
                            "Expected 1 parameter (`config`) for `is_enabled` closure, got 0!",
                        ))
                    }
                    _ => {
                        return Err(if cfg!(feature = "proc_macro_span") {
                            let extra_args_span = closure
                                .inputs
                                .iter()
                                .map(|i| i.span())
                                .reduce(|s, i| s.join(i).unwrap());

                            if let Some(extra_args_span) = extra_args_span {
                                Error::new(
                                    extra_args_span,
                                    "Too many parameters! Expected 1 parameter (`config`) for `is_enabled` closure!",
                                )
                            } else {
                                Error::new(
                                    closure.inputs.span(),
                                    "Too many parameters! Expected 1 parameter (`config`) for `is_enabled` closure!",
                                )
                            }
                        } else {
                            Error::new(
                                closure.inputs.span(),
                                "Too many parameters! Expected 1 parameter (`config`) for `is_enabled` closure!",
                            )
                        })
                    }
                }

                // Explicitly set the parameter to &Config.
                let config_arg = &mut closure.inputs[0];

                match config_arg {
                    Pat::Ident(i) => {
                        *config_arg = Pat::Type(PatType {
                            attrs: vec![],
                            pat: Box::new(Pat::Ident(i.clone())),
                            colon_token: Token![:](config_arg.span()),
                            ty: Box::new(Type::Reference(TypeReference {
                                and_token: Token![&](config_arg.span()),
                                lifetime: None,
                                mutability: None,
                                elem: Box::new(Type::Path(TypePath {
                                    qself: None,
                                    path: Path::from(PathSegment {
                                        ident: Ident::new("Config", closure.output.span()),
                                        arguments: PathArguments::None,
                                    }),
                                })),
                            })),
                        });
                    }
                    Pat::Type(PatType { ty, .. }) => match &**ty {
                        Type::Reference(r) => {
                            if let Some(lifetime) = &r.lifetime {
                                return Err(Error::new(
                                    lifetime.span(),
                                    "Expected parameter to not have a lifetime!",
                                ));
                            }

                            if let Some(mutability) = r.mutability {
                                return Err(Error::new(
                                    mutability.span(),
                                    "Expected parameter to be immutable!",
                                ));
                            }

                            if let Type::Path(p) = &*r.elem {
                                match p
                                    .path
                                    .segments
                                    .iter()
                                    .map(|s| &s.ident)
                                    .collect::<Vec<_>>()
                                    .as_slice()
                                {
                                    [a] if *a == "Config" => (),
                                    [a, b] if *a == "utility" && *b == "Config" => (),
                                    _ => {
                                        return Err(Error::new(
                                            p.path.span(),
                                            "Expected parameter to be of type `Config`!",
                                        ))
                                    }
                                }
                            } else {
                                return Err(Error::new(
                                    r.elem.span(),
                                    "Expected parameter to be of type `Config`!",
                                ));
                            }
                        }

                        _ => return Err(Error::new(ty.span(), "Expected type `&Config`!")),
                    },

                    _ => {
                        return Err(Error::new(
                            config_arg.span(),
                            "Expected parameter for `is_enabled` closure to be of type `&Config`!",
                        ))
                    }
                }

                // Make sure output type is bool.
                if let ReturnType::Default = closure.output {
                    closure.output = ReturnType::Type(
                        Token![->](closure.output.span()),
                        Box::new(Type::Path(TypePath {
                            qself: None,
                            path: Path::from(PathSegment {
                                ident: Ident::new("bool", closure.output.span()),
                                arguments: PathArguments::None,
                            }),
                        })),
                    );
                }

                // Make sure body is in a block.
                match *closure.body {
                    Expr::Block(_) => (),
                    b => {
                        *closure.body = Expr::Block(ExprBlock {
                            attrs: vec![],
                            label: None,
                            block: Block {
                                brace_token: token::Brace(b.span()),
                                stmts: vec![Stmt::Expr(b)],
                            },
                        });
                    }
                }

                Ok(InteractionField::IsEnabled(closure))
            }
            _ => Err(Error::new(label.span(), "Unknown field!")),
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        value
    }
}
