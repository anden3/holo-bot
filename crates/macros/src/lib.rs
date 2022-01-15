extern crate proc_macro;

#[macro_use]
mod macros;

mod attributes;
mod consts;
mod structures;
mod util;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;

use structures::*;

#[proc_macro_attribute]
pub fn interaction_cmd(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let fun = parse_macro_input!(input as CommandFun);
    fun.into_token_stream().into()
}

#[proc_macro]
pub fn interaction_setup(input: TokenStream) -> TokenStream {
    let setup = parse_macro_input!(input as InteractionSetup);
    TokenStream::from(setup.into_token_stream())
}

#[proc_macro]
pub fn parse_interaction_options(input: TokenStream) -> TokenStream {
    let params = parse_macro_input!(input as ParseInteractionOptions);
    params.into_token_stream().into()
}

#[proc_macro]
pub fn match_sub_commands(input: TokenStream) -> TokenStream {
    let params = parse_macro_input!(input as MatchSubCommands);
    params.into_token_stream().into()
}

#[proc_macro]
pub fn clone_variables(input: TokenStream) -> TokenStream {
    let clone_block = parse_macro_input!(input as ClonedVariablesBlock);
    clone_block.into_token_stream().into()
}
