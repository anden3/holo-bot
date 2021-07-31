use super::prelude::*;

use super::{InteractionOpts, InteractionRestrictions};

#[derive(Debug)]
pub enum InteractionField {
    Name(String),
    Group(String),
    Description(String),
    Options(InteractionOpts),
    Restrictions(InteractionRestrictions),
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
            _ => Err(Error::new(label.span(), "Unknown field!")),
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        value
    }
}
