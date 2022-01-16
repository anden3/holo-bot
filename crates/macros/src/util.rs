use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, ToTokens};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream, Result},
    punctuated::Punctuated,
    token::{Comma, Mut},
    Ident, Lit, Type,
};

pub trait IdentExt2: Sized {
    fn to_uppercase(&self) -> Self;
    fn with_suffix(&self, suf: &str) -> Ident;
}

impl IdentExt2 for Ident {
    #[inline]
    fn to_uppercase(&self) -> Self {
        format_ident!("{}", self.to_string().to_uppercase())
    }

    #[inline]
    fn with_suffix(&self, suffix: &str) -> Ident {
        format_ident!("{}_{}", self.to_string().to_uppercase(), suffix)
    }
}

pub trait LitExt {
    fn to_u64(&self) -> u64;
    fn to_str(&self) -> String;
    fn to_bool(&self) -> bool;
    fn to_ident(&self) -> Ident;
}

impl LitExt for Lit {
    fn to_u64(&self) -> u64 {
        match self {
            Lit::Str(s) => s
                .value()
                .parse()
                .expect("string must be parseable into u64"),
            Lit::Char(c) => c.value().into(),
            Lit::Int(i) => i.base10_parse().expect("number must be parseable into u64"),
            _ => panic!("values must be either an integer or a string parseable into u64"),
        }
    }

    fn to_str(&self) -> String {
        match self {
            Lit::Str(s) => s.value(),
            Lit::Char(c) => c.value().to_string(),
            Lit::Byte(b) => (b.value() as char).to_string(),
            _ => panic!("values must be a string or a char"),
        }
    }

    fn to_bool(&self) -> bool {
        if let Lit::Bool(b) = self {
            b.value
        } else {
            self.to_str()
                .parse()
                .unwrap_or_else(|_| panic!("expected bool from {:?}", self))
        }
    }

    #[inline]
    fn to_ident(&self) -> Ident {
        Ident::new(&self.to_str(), self.span())
    }
}

#[derive(Debug, Clone)]
pub struct Argument {
    pub mutable: Option<Mut>,
    pub name: Ident,
    pub kind: Type,
}

impl ToTokens for Argument {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let Argument {
            mutable,
            name,
            kind,
        } = self;

        stream.extend(quote! {
            #mutable #name: #kind
        });
    }
}

#[derive(Debug)]
pub struct Parenthesised<T>(pub Punctuated<T, Comma>);

impl<T: Parse> Parse for Parenthesised<T> {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        parenthesized!(content in input);

        Ok(Parenthesised(content.parse_terminated(T::parse)?))
    }
}

#[derive(Debug)]
pub struct AsOption<T>(pub Option<T>);

impl<T: ToTokens> ToTokens for AsOption<T> {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        match &self.0 {
            Some(o) => stream.extend(quote!(Some(#o))),
            None => stream.extend(quote!(None)),
        }
    }
}

impl<T> Default for AsOption<T> {
    #[inline]
    fn default() -> Self {
        AsOption(None)
    }
}
