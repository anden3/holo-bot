pub use std::{
    collections::{HashMap, HashSet, VecDeque},
    vec::IntoIter,
};

pub use proc_macro2::{Punct, Spacing, TokenStream as TokenStream2};
pub use quote::{format_ident, quote, ToTokens, TokenStreamExt};
pub use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream, Result},
    punctuated::Punctuated,
    spanned::Spanned,
    token, Attribute, Block, Error, Expr, ExprBlock, ExprClosure, FnArg, Ident, Lit, LitInt,
    LitStr, Pat, ReturnType, Stmt, Token, Type, Visibility,
};
