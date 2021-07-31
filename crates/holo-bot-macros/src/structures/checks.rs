use super::prelude::*;

use crate::{consts::CHECK, util::IdentExt2};

#[derive(Debug, Default)]
pub struct Checks(pub Vec<Ident>);

impl ToTokens for Checks {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let v = self.0.iter().map(|i| i.with_suffix(CHECK));

        stream.extend(quote!(&[#(&#v),*]));
    }
}
