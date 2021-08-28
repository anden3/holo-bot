#![allow(unused_macros)]

// #[macro_use]
macro_rules! wrap_vectors {
    ($($n:ident|Vec<$t:ty>),*) => {
        $(
            #[derive(Debug)]
            pub struct $n(Vec<$t>);

            impl ::std::iter::IntoIterator for $n {
                type Item = $t;
                type IntoIter = ::std::vec::IntoIter<Self::Item>;

                fn into_iter(self) -> Self::IntoIter {
                    self.0.into_iter()
                }
            }

            impl ::syn::parse::Parse for $n {
                fn parse(input: ::syn::parse::ParseStream) -> ::syn::parse::Result<Self> {
                    let content;
                    ::syn::bracketed!(content in input);

                    let mut opts = ::std::vec::Vec::new();

                    while !content.is_empty() {
                        opts.push(content.parse::<$t>()?);
                    }

                    Ok(Self(opts))
                }
            }
        )*
    }
}

// #[macro_use]
macro_rules! keep_syn_variants {
    ($tp:ident, $val:expr, [$($t:ident),*], $msg:literal) => {
        match $val {
            $(
                $tp::$t(_) => Ok($val),
            )*
            _ => Err(::syn::Error::new($val.span(), $msg)),
        }
    };
}

// #[macro_use]
macro_rules! yeet_syn_variants {
    ($tp:ident, $val:expr, [$($t:ident),*], $msg:literal) => {
        match $val {
            $(
                $tp::$t(a) => Err(::syn::Error::new(a.span(), $msg)),
            )*
            _ => Ok($val),
        }
    };
}

// #[macro_use]
macro_rules! propagate_err {
    ($tokens:ident, $res:expr) => {{
        match $res {
            Ok(v) => v,
            Err(e) => {
                $crate::util::into_stream(e).to_tokens($tokens);
                return;
            }
        }
    }};
}

// #[macro_use]
macro_rules! match_options {
    ($tokens:ident, $v:expr, $values:ident, $options:ident, $span:expr => [$($name:ident);*]) => {
        match $v {
            $(
                stringify!($name) => $options.$name = propagate_err!($tokens, $crate::attributes::parse($values)),
            )*
            _ => {
                return ::syn::parse::Error::new($span, format_args!("invalid attribute: {:?}", $v))
                    .to_compile_error()
                    .to_tokens($tokens);
            },
        }
    };
}
