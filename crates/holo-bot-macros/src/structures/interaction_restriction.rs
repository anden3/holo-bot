use super::{prelude::*, Check, RateLimit, RateLimitGrouping};

wrap_vectors!(
    InteractionRestrictions | Vec<InteractionRestriction>
);

#[derive(Debug)]
pub enum InteractionRestriction {
    OwnersOnly,
    Checks(Vec<Check>),
    AllowedRoles(Vec<Lit>),
    RateLimit(RateLimit),
}

impl Parse for InteractionRestriction {
    fn parse(input: ParseStream) -> Result<Self> {
        let label = input.parse::<Ident>()?;
        let name = label.to_string();

        let restriction = match name.as_str() {
            "owners_only" => Ok(Self::OwnersOnly),
            "rate_limit" => {
                input.parse::<Token![=]>()?;
                let count = input.parse::<LitInt>()?.base10_parse::<u32>()?;
                input.parse::<Token![in]>()?;

                let interval_unit_count = input.parse::<LitInt>()?.base10_parse::<u32>()?;
                let interval_unit = input.parse::<Ident>()?;
                let interval_unit_str = interval_unit.to_string();

                let interval_sec = match interval_unit_str.as_str() {
                    "s" | "sec" | "second" | "seconds" => interval_unit_count,
                    "m" | "min" | "minute" | "minutes" => interval_unit_count * 60,
                    "h" | "hour" | "hours" => interval_unit_count * 60 * 60,
                    _ => return Err(Error::new(interval_unit.span(), "Unknown time unit.")),
                };

                let grouping;

                if input.peek(Token![for]) {
                    input.parse::<Token![for]>()?;

                    let group = input.parse::<Ident>()?;
                    let group_str = group.to_string();

                    grouping = match group_str.as_str() {
                        "user" | "User" => RateLimitGrouping::User,
                        "all" | "everyone" => RateLimitGrouping::Everyone,
                        _ => return Err(Error::new(group.span(), "Unknown grouping.")),
                    }
                } else {
                    grouping = RateLimitGrouping::Everyone;
                }

                Ok(Self::RateLimit(RateLimit {
                    count,
                    interval_sec,
                    grouping,
                }))
            }
            "checks" => {
                input.parse::<Token![=]>()?;

                let content;
                bracketed!(content in input);

                let checks: Vec<_> =
                    Punctuated::<Check, Token![,]>::parse_terminated_with(&content, Check::parse)?
                        .into_iter()
                        .collect();

                Ok(Self::Checks(checks))
            }
            "allowed_roles" => {
                input.parse::<Token![=]>()?;

                let content;
                bracketed!(content in input);

                let roles: Vec<_> =
                    Punctuated::<Lit, Token![,]>::parse_terminated_with(&content, Lit::parse)?
                        .into_iter()
                        .collect();

                Ok(Self::AllowedRoles(roles))
            }
            _ => Err(input.error("Unknown restriction.")),
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        restriction
    }
}
