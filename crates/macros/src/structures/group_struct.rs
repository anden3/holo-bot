use super::prelude::*;

use crate::util;

#[derive(Debug)]
pub struct GroupStruct {
    pub visibility: Visibility,
    pub cooked: Vec<Attribute>,
    pub attributes: Vec<Attribute>,
    pub name: Ident,
}

impl Parse for GroupStruct {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;
        util::rename_attributes(&mut attributes, "doc", "description");

        let cooked = util::remove_cooked(&mut attributes);
        let visibility = input.parse()?;
        input.parse::<Token![struct]>()?;

        let name = input.parse()?;
        input.parse::<Token![;]>()?;

        Ok(Self {
            visibility,
            cooked,
            attributes,
            name,
        })
    }
}

impl ToTokens for GroupStruct {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let Self {
            visibility,
            cooked,
            attributes: _,
            name,
        } = self;

        stream.extend(quote! {
            #(#cooked)*
            #visibility struct #name;
        });
    }
}
