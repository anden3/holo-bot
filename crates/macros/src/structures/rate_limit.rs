use super::prelude::*;

#[derive(Debug)]
pub struct RateLimit {
    pub count: u32,
    pub interval_sec: u32,
    pub grouping: RateLimitGrouping,
}

impl ToTokens for RateLimit {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let count = &self.count;
        let interval_sec = &self.interval_sec;
        let grouping = &self.grouping;

        let result = quote! {
            RateLimit {
                count: #count,
                interval_sec: #interval_sec,
                grouping: #grouping,
            },
        };

        result.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub enum RateLimitGrouping {
    User,
    Everyone,
}

impl quote::ToTokens for RateLimitGrouping {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let val = format_ident!(
            "{}",
            match *self {
                RateLimitGrouping::Everyone => "Everyone",
                RateLimitGrouping::User => "User",
            }
        );

        let result = quote! {
            RateLimitGrouping::#val
        };

        result.to_tokens(tokens);
    }
}
