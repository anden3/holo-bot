extern crate proc_macro;

#[macro_use]
mod macros;

mod structures;
mod util;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;

use structures::*;

#[proc_macro]
pub fn clone_variables(input: TokenStream) -> TokenStream {
    let clone_block = parse_macro_input!(input as ClonedVariablesBlock);
    clone_block.into_token_stream().into()
}
