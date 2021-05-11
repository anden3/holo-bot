use std::collections::{HashSet, VecDeque};

use proc_macro2::{Punct, Spacing, TokenStream as TokenStream2};
use quote::{format_ident, quote, ToTokens, TokenStreamExt};
use syn::{
    braced, bracketed,
    parse::{Error, Parse, ParseStream, Result},
    punctuated::Punctuated,
    spanned::Spanned,
    Attribute, Block, Expr, ExprClosure, FnArg, Ident, Lit, LitInt, LitStr, Pat, ReturnType, Stmt,
    Token, Type, Visibility,
};

use crate::{
    consts::suffixes::INTERACTION,
    util::{Argument, IdentExt2, Parenthesised},
};
use crate::{consts::CHECK, util};

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

                    while let Ok(opt) = content.parse::<$t>() {
                        opts.push(opt);
                    }

                    Ok(Self(opts))
                }
            }
        )*
    }
}

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

#[derive(Debug, Default)]
pub struct InteractionOptions {
    pub checks: Checks,
    pub allowed_roles: Vec<String>,
    pub required_permissions: Permissions,
    pub owners_only: bool,
    pub owner_privilege: bool,
}

impl InteractionOptions {
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }
}

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

        let cooked = remove_cooked(&mut attributes);
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

/* #[derive(Debug, Default)]
pub struct GroupOptions {
    pub required_permissions: Permissions,
    pub commands: Vec<Ident>,
    pub sub_groups: Vec<Ident>,
}

impl GroupOptions {
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }
} */

#[derive(Debug, Default)]
pub struct Checks(pub Vec<Ident>);

impl ToTokens for Checks {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let v = self.0.iter().map(|i| i.with_suffix(CHECK));

        stream.extend(quote!(&[#(&#v),*]));
    }
}

#[derive(Debug, Default)]
pub struct Permissions(pub u64);

impl Permissions {
    pub fn from_str(s: &str) -> Option<Self> {
        Some(Permissions(match s.to_uppercase().as_str() {
            "PRESET_GENERAL" => 0b0000_0110_0011_0111_1101_1100_0100_0001,
            "PRESET_TEXT" => 0b0000_0000_0000_0111_1111_1100_0100_0000,
            "PRESET_VOICE" => 0b0000_0011_1111_0000_0000_0000_0000_0000,
            "CREATE_INVITE" => 0b0000_0000_0000_0000_0000_0000_0000_0001,
            "KICK_MEMBERS" => 0b0000_0000_0000_0000_0000_0000_0000_0010,
            "BAN_MEMBERS" => 0b0000_0000_0000_0000_0000_0000_0000_0100,
            "ADMINISTRATOR" => 0b0000_0000_0000_0000_0000_0000_0000_1000,
            "MANAGE_CHANNELS" => 0b0000_0000_0000_0000_0000_0000_0001_0000,
            "MANAGE_GUILD" => 0b0000_0000_0000_0000_0000_0000_0010_0000,
            "ADD_REACTIONS" => 0b0000_0000_0000_0000_0000_0000_0100_0000,
            "VIEW_AUDIT_LOG" => 0b0000_0000_0000_0000_0000_0000_1000_0000,
            "PRIORITY_SPEAKER" => 0b0000_0000_0000_0000_0000_0001_0000_0000,
            "READ_MESSAGES" => 0b0000_0000_0000_0000_0000_0100_0000_0000,
            "SEND_MESSAGES" => 0b0000_0000_0000_0000_0000_1000_0000_0000,
            "SEND_TTS_MESSAGES" => 0b0000_0000_0000_0000_0001_0000_0000_0000,
            "MANAGE_MESSAGES" => 0b0000_0000_0000_0000_0010_0000_0000_0000,
            "EMBED_LINKS" => 0b0000_0000_0000_0000_0100_0000_0000_0000,
            "ATTACH_FILES" => 0b0000_0000_0000_0000_1000_0000_0000_0000,
            "READ_MESSAGE_HISTORY" => 0b0000_0000_0000_0001_0000_0000_0000_0000,
            "MENTION_EVERYONE" => 0b0000_0000_0000_0010_0000_0000_0000_0000,
            "USE_EXTERNAL_EMOJIS" => 0b0000_0000_0000_0100_0000_0000_0000_0000,
            "CONNECT" => 0b0000_0000_0001_0000_0000_0000_0000_0000,
            "SPEAK" => 0b0000_0000_0010_0000_0000_0000_0000_0000,
            "MUTE_MEMBERS" => 0b0000_0000_0100_0000_0000_0000_0000_0000,
            "DEAFEN_MEMBERS" => 0b0000_0000_1000_0000_0000_0000_0000_0000,
            "MOVE_MEMBERS" => 0b0000_0001_0000_0000_0000_0000_0000_0000,
            "USE_VAD" => 0b0000_0010_0000_0000_0000_0000_0000_0000,
            "CHANGE_NICKNAME" => 0b0000_0100_0000_0000_0000_0000_0000_0000,
            "MANAGE_NICKNAMES" => 0b0000_1000_0000_0000_0000_0000_0000_0000,
            "MANAGE_ROLES" => 0b0001_0000_0000_0000_0000_0000_0000_0000,
            "MANAGE_WEBHOOKS" => 0b0010_0000_0000_0000_0000_0000_0000_0000,
            "MANAGE_EMOJIS" => 0b0100_0000_0000_0000_0000_0000_0000_0000,
            _ => return None,
        }))
    }
}

impl ToTokens for Permissions {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let bits = self.0;

        let path = quote!(serenity::model::permissions::Permissions);

        stream.extend(quote! {
            #path { bits: #bits }
        });
    }
}

#[derive(Debug)]
pub struct CommandFun {
    /// `#[...]`-style attributes.
    pub attributes: Vec<Attribute>,
    /// Populated cooked attributes. These are attributes outside of the realm of this crate's procedural macros
    /// and will appear in generated output.
    pub cooked: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: Ident,
    pub args: Vec<Argument>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

impl Parse for CommandFun {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;

        // Rename documentation comment attributes (`#[doc = "..."]`) to `#[description = "..."]`.
        util::rename_attributes(&mut attributes, "doc", "description");

        let cooked = remove_cooked(&mut attributes);
        let visibility = input.parse::<Visibility>()?;

        input.parse::<Token![async]>()?;
        input.parse::<Token![fn]>()?;

        let name = input.parse()?;

        // (...)
        let Parenthesised(args) = input.parse::<Parenthesised<FnArg>>()?;

        let ret = match input.parse::<ReturnType>()? {
            ReturnType::Type(_, t) => (*t).clone(),
            ReturnType::Default => {
                return Err(input
                    .error("expected a result type of either `CommandResult` or `CheckResult`"))
            }
        };

        // { ... }
        let bcont;
        braced!(bcont in input);
        let body = bcont.call(Block::parse_within)?;

        let args = args
            .into_iter()
            .map(parse_argument)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            attributes,
            cooked,
            visibility,
            name,
            args,
            ret,
            body,
        })
    }
}

wrap_vectors!(
    InteractionOpts | Vec<InteractionOpt>,
    InteractionRestrictions | Vec<InteractionRestriction>
);

#[derive(Debug)]
pub struct InteractionSetup {
    name: String,
    group: String,
    description: String,
    options: Vec<InteractionOpt>,
    owners_only: bool,
    allowed_roles: HashSet<Lit>,
    checks: Vec<Check>,
    rate_limit: Option<RateLimit>,
}

impl Parse for InteractionSetup {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let mut fields = Vec::new();

        while let Ok(opt) = input.parse::<InteractionField>() {
            fields.push(opt);
        }

        let mut name = String::new();
        let mut group = String::new();
        let mut description = String::new();
        let mut options = Vec::new();
        let mut restrictions = Vec::new();

        for field in fields {
            match field {
                InteractionField::Name(s) => name = s,
                InteractionField::Group(g) => group = g,
                InteractionField::Description(s) => description = s,
                InteractionField::Options(o) => options.extend(o),
                InteractionField::Restrictions(r) => restrictions.extend(r),
            }
        }

        let mut owners_only = false;
        let mut allowed_roles = HashSet::new();
        let mut checks = Vec::new();
        let mut rate_limit = None;

        for restriction in restrictions {
            match restriction {
                InteractionRestriction::OwnersOnly => owners_only = true,
                InteractionRestriction::AllowedRoles(r) => allowed_roles.extend(r),
                InteractionRestriction::Checks(c) => checks.extend(c),
                InteractionRestriction::RateLimit(r) => rate_limit = Some(r),
            }
        }

        Ok(Self {
            name,
            group,
            description,
            options,
            owners_only,
            allowed_roles,
            checks,
            rate_limit,
        })
    }
}

#[allow(unstable_name_collisions)]
impl ToTokens for InteractionSetup {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let includes_enum_option = self.options.iter().any(|o| o.contains_enum_option());

        let option_choices = self
            .options
            .iter()
            .map(|opt| opt.to_json_tokens())
            .collect::<Vec<_>>();

        let name = &self.name;
        let group = &self.group;
        let description = &self.description;
        let owners_only = self.owners_only;
        let rate_limit = match &self.rate_limit {
            Some(r) => quote! { Some(#r) },
            None => quote! { None },
        };

        let mut allowed_roles = Vec::new();

        for role in &self.allowed_roles {
            let role = keep_syn_variants!(
                Lit,
                role,
                [Str, Int],
                "Expected role name or ID, got something else."
            );

            allowed_roles.push(match role {
                Ok(Lit::Str(s)) => quote! {
                    *guild.role_by_name(#s)
                        .ok_or_else(|| ::anyhow::anyhow!("Could not find role: {}", #s))
                        .context(here!())?
                        .id.as_u64()
                },
                Ok(Lit::Int(i)) => quote! { #i },
                Ok(_) => unreachable!(),
                Err(e) => e.to_compile_error(),
            });
        }

        let checks = &self.checks;

        let default_permission = !&owners_only && self.allowed_roles.is_empty();

        let mut imports: Vec<TokenStream2> = Vec::new();

        if includes_enum_option {
            let enum_iter = quote! { use strum::IntoEnumIterator; };
            imports.push(enum_iter);
        }

        let name_ident = format_ident!("{}", name);
        let n = format_ident!("{}_{}", name.to_uppercase(), INTERACTION);
        let declaration_path = quote!(DeclaredInteraction);

        let result = quote! {
            #[allow(missing_docs)]
            pub static #n: #declaration_path = #declaration_path {
                name: #name,
                group: #group,
                setup: setup,
                func: #name_ident,
            };

            #[allow(missing_docs)]
            pub fn setup<'fut>(guild: &'fut Guild) -> ::futures::future::BoxFuture<'fut, anyhow::Result<(::bytes::Bytes, InteractionOptions)>> {
                use ::futures::future::FutureExt;
                use ::serenity::{model::interactions::ApplicationCommand, http::{request::RequestBuilder, routing::RouteInfo}};
                #( #imports )*

                async move {
                    let owner_id = guild.owner_id.as_u64();

                    let body = ::serde_json::json!({
                        "name": #name,
                        "description": #description,
                        "options": [
                            #(
                                #option_choices
                            )*
                        ],
                        "default_permission": #default_permission
                    });

                    let body = ::serde_json::to_vec(&body)?.into();

                    let settings = InteractionOptions {
                        rate_limit: #rate_limit,
                        checks: &[
                            #(
                                #checks
                            )*
                        ],
                        allowed_roles: HashSet::from_iter(vec![
                            #(
                                ::serenity::model::id::RoleId(#allowed_roles),
                            )*
                        ]),
                        permissions: vec![
                            #(InteractionPermission {
                                id: #allowed_roles,
                                permission_type: 1,
                                permission: true
                            },)*
                            InteractionPermission {
                                id: *owner_id,
                                permission_type: 2,
                                permission: true
                            }
                        ],
                        owners_only: #owners_only,
                    };

                    Ok((body, settings))
                }.boxed()
            }
        };

        result.to_tokens(tokens);
    }

    fn to_token_stream(&self) -> TokenStream2 {
        let mut tokens = TokenStream2::new();
        self.to_tokens(&mut tokens);
        tokens
    }

    fn into_token_stream(self) -> TokenStream2
    where
        Self: Sized,
    {
        self.to_token_stream()
    }
}

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

#[derive(Debug)]
pub struct InteractionOpt {
    pub required: bool,
    pub name: Ident,
    pub desc: String,
    pub ty: Ident,

    pub choices: Vec<InteractionOptChoice>,
    pub options: Vec<InteractionOpt>,
    pub enum_type: Option<Type>,
}

impl InteractionOpt {
    pub fn to_json_tokens(&self) -> TokenStream2 {
        let ty = &self.ty;
        let name = &self.name.to_string();
        let desc = &self.desc;
        let req = self.required;

        let choices_array;

        if let Some(enum_type) = &self.enum_type {
            choices_array = quote! {
                #enum_type::iter().map(|e| ::serde_json::json!({
                    "name": e.to_string(),
                    "value": e.to_string()
                })).collect::<Vec<_>>()
            };
        } else {
            let choices = self
                .choices
                .iter()
                .map(|c| c.to_json_tokens())
                .collect::<Vec<_>>();

            choices_array = quote! {[
                #({
                    #choices
                },)*
            ]};
        }

        let options = &self
            .options
            .iter()
            .map(|o| o.to_json_tokens())
            .collect::<Vec<_>>();

        let result = quote! {{
            "type": ::serenity::model::interactions::ApplicationCommandOptionType::#ty,
            "name": #name,
            "description": #desc,
            "required": #req,
            "choices": #choices_array,
            "options": [
                #(#options)*
            ]
        },};

        result.into_token_stream()
    }

    pub fn contains_enum_option(&self) -> bool {
        let mut remaining: VecDeque<&Self> = VecDeque::new();
        remaining.push_back(self);

        while let Some(current) = remaining.pop_front() {
            if current.enum_type.is_some() {
                return true;
            }

            remaining.extend(current.options.iter());
        }

        false
    }
}

impl Parse for InteractionOpt {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        if !input.peek(Token![#]) || !input.peek2(Token![!]) {
            return Err(Error::new(input.span(), "Missing description"));
        }

        input.parse::<Token![#]>()?;
        input.parse::<Token![!]>()?;

        let doc;
        bracketed!(doc in input);

        doc.parse::<Ident>()?;
        doc.parse::<Token![=]>()?;
        let desc = doc.parse::<LitStr>()?.value();

        let required;

        if input.peek(Ident) && input.peek2(Ident) {
            match input.parse::<Ident>() {
                Ok(ident) => match ident.to_string().as_str() {
                    "req" => required = true,
                    _ => return Err(Error::new(ident.span(), "Only valid modifier is `req`.")),
                },
                Err(e) => return Err(e),
            }
        } else {
            required = false;
        }

        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;

        let ty = input.parse::<syn::Type>()?;
        let ty = match ty {
            Type::Path(p) => match p.path.get_ident() {
                Some(ident) => ident.to_owned(),
                None => return Err(Error::new(p.span(), "Not supported.")),
            },
            _ => return Err(Error::new(ty.span(), "Not supported.")),
        };

        let mut choices = Vec::new();
        let mut options = Vec::new();

        let mut enum_type = None;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;

            if input.peek(Token![enum]) {
                input.parse::<Token![enum]>()?;
                enum_type = Some(input.parse::<Type>()?);
            } else {
                let content;
                bracketed!(content in input);

                match ty.to_string().as_str() {
                    "String" | "Integer" => {
                        choices =
                            Punctuated::<InteractionOptChoice, Token![,]>::parse_terminated_with(
                                &content,
                                InteractionOptChoice::parse,
                            )?
                            .into_iter()
                            .collect();
                    }
                    "SubCommand" | "SubCommandGroup" => {
                        options = Punctuated::<InteractionOpt, Token![,]>::parse_terminated_with(
                            &content,
                            InteractionOpt::parse,
                        )?
                        .into_iter()
                        .collect();
                    }
                    _ => {
                        return Err(Error::new(
                            content.span(),
                            "Option type doesn't support choices.",
                        ))
                    }
                }
            }
        }

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(InteractionOpt {
            required,
            name,
            desc,
            ty,
            choices,
            options,
            enum_type,
        })
    }
}

impl ToTokens for InteractionOpt {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let choices = self.choices.iter();
        let options = self.options.iter();

        let name = &self.name.to_string();
        let desc = &self.desc;
        let ty = &self.ty;

        let option_type = ty.to_string();

        let choices_stream = match option_type.as_str() {
            "String" => quote! { #(o.add_string_choice(#choices);)* },
            "Integer" => quote! { #(o.add_int_choice(#choices);)* },
            "SubCommandGroup" => quote! { #(o.create_sub_option(|o| #options);)* },
            "SubCommand" => quote! { #(o.create_sub_option(|o| #options);)* },
            _ => TokenStream2::new(),
        };

        let stream = quote! {
            o.name(#name);
            o.description(#desc);
            o.kind(::serenity::model::interactions::ApplicationCommandOptionType::#ty);
            #choices_stream
        };

        stream.to_tokens(tokens);
    }

    fn to_token_stream(&self) -> TokenStream2 {
        let mut tokens = TokenStream2::new();
        self.to_tokens(&mut tokens);
        tokens
    }

    fn into_token_stream(self) -> TokenStream2
    where
        Self: Sized,
    {
        self.to_token_stream()
    }
}

#[derive(Debug)]
pub struct InteractionOptChoice {
    name: String,
    value: Expr,
}

impl InteractionOptChoice {
    pub fn to_json_tokens(&self) -> TokenStream2 {
        let name = &self.name;
        let value = &self.value;

        let result = quote! {
            "name": #name,
            "value": #value
        };

        result.into_token_stream()
    }
}

impl Parse for InteractionOptChoice {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = input.parse::<LitStr>()?.value();
        input.parse::<Token![:]>()?;

        let value = input.parse::<Expr>()?;
        let value = yeet_syn_variants!(
            Expr,
            value,
            [
                Array, Assign, AssignOp, Async, Block, Box, Break, Closure, Continue, ForLoop, If,
                Let, Loop, Match, Range, Repeat, Return, Struct, Try, TryBlock, Tuple, Type,
                Unsafe, Verbatim, While, Yield
            ],
            "Value must be either `String` or `Integer`"
        )?;

        Ok(Self { name, value })
    }
}

impl ToTokens for InteractionOptChoice {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let value = &self.value;

        name.to_tokens(tokens);
        tokens.append(Punct::new(',', Spacing::Alone));
        value.to_tokens(tokens);
    }
}

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

#[derive(Debug)]
pub struct RateLimit {
    count: u32,
    interval_sec: u32,
    grouping: RateLimitGrouping,
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

#[derive(Debug)]
pub struct Check {
    name: String,
    func: Expr,
}

impl Parse for Check {
    fn parse(input: ParseStream) -> Result<Self> {
        static CHECK_ARGS: &[&str] = &["ctx", "request", "interaction"];

        let name = input.parse::<Ident>()?.to_string();
        input.parse::<Token![:]>()?;

        let closure = input.parse::<ExprClosure>()?;

        let mut errors = Vec::new();

        if closure.inputs.len() != 3 {
            errors.push(Error::new(
                closure.span(),
                "3 arguments are required: `ctx`, `request`, and `interaction`.",
            ));
        }

        for (j, input) in closure.inputs.iter().enumerate().take(3) {
            match input {
                Pat::Ident(i) => {
                    if i.ident != CHECK_ARGS[j] {
                        errors.push(Error::new(
                            i.span(),
                            format!("Expected {}, got {:?}", CHECK_ARGS[j], i.ident),
                        ));
                    }
                }
                Pat::Path(p) => {
                    if !p.path.is_ident(CHECK_ARGS[j]) {
                        errors.push(Error::new(
                            p.span(),
                            format!("Expected {}, got {:?}", CHECK_ARGS[j], p.path),
                        ));
                    }
                }
                _ => errors.push(Error::new(input.span(), "Invalid argument to check.")),
            }
        }

        if !errors.is_empty() {
            let error = errors
                .into_iter()
                .reduce(|mut a, b| {
                    a.combine(b);
                    a
                })
                .unwrap();
            return Err(error);
        }

        Ok(Self {
            name,
            func: *closure.body,
        })
    }
}

impl ToTokens for Check {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let func = &self.func;

        let result = quote! {
            Check {
                name: #name,
                function: |ctx: &Ctx, request: &Interaction, interaction: &RegisteredInteraction| #func,
            },
        };

        result.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub struct ParseInteractionOptions {
    pub data: Expr,
    pub options: Vec<ParseInteractionOption>,
}

impl Parse for ParseInteractionOptions {
    fn parse(input: ParseStream) -> Result<Self> {
        let data = input.parse()?;
        input.parse::<Token![,]>()?;

        let content;
        bracketed!(content in input);

        let options = Punctuated::<ParseInteractionOption, Token![,]>::parse_terminated_with(
            &content,
            ParseInteractionOption::parse,
        )?
        .into_iter()
        .collect();

        Ok(Self { data, options })
    }
}

#[derive(Debug)]
pub struct ParseInteractionOption {
    name: String,
    ident: Ident,
    ty: Ident,
    is_enum: bool,
    is_required: bool,
    default: Option<Expr>,
}

impl ParseInteractionOption {
    pub fn declare_variable(&self) -> TokenStream2 {
        let ident = &self.ident;
        let ty = &self.ty;

        if self.is_required {
            quote! { let mut #ident: #ty = Default::default(); }
        } else if let Some(d) = &self.default {
            quote! { let mut #ident: #ty = #d; }
        } else {
            quote! { let mut #ident: Option<#ty> = None; }
        }
    }
}

impl Parse for ParseInteractionOption {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        let name = ident.to_string();

        input.parse::<Token![:]>()?;

        let is_required =
            match input.peek(Ident) && (input.peek2(Token![enum]) || input.peek2(Ident)) {
                true => {
                    input.parse::<Ident>()?;
                    true
                }
                false => false,
            };

        let is_enum = match input.peek(Token![enum]) {
            true => {
                input.parse::<Token![enum]>()?;
                true
            }
            false => false,
        };

        let ty = input.parse()?;

        let default;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            default = Some(input.parse()?);
        } else {
            default = None;
        }

        Ok(Self {
            name,
            ident,
            ty,
            is_enum,
            is_required,
            default,
        })
    }
}

impl ToTokens for ParseInteractionOption {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = &self.name;
        let ident = &self.ident;
        let ty = &self.ty;

        let output = match self.is_enum {
            true => {
                if self.default.is_some() {
                    quote! {
                        #name => {
                            #ident = #ty::from_str(&::serde_json::from_value::<String>(value.clone()).context(::utility::here!())?).unwrap()
                        }
                    }
                } else {
                    quote! {
                        #name => {
                            #ident = #ty::from_str(&::serde_json::from_value::<String>(value.clone()).context(::utility::here!())?).ok()
                        }
                    }
                }
            }
            false => {
                if self.is_required || self.default.is_some() {
                    quote! {
                        #name => {
                            #ident = ::serde_json::from_value::<#ty>(value.clone()).context(::utility::here!())?
                        }
                    }
                } else {
                    quote! {
                        #name => {
                            #ident = Some(::serde_json::from_value::<#ty>(value.clone()).context(::utility::here!())?)
                        }
                    }
                }
            }
        };

        output.to_tokens(tokens);
    }
}

pub fn parse_argument(arg: FnArg) -> Result<Argument> {
    match arg {
        FnArg::Typed(typed) => {
            let pat = typed.pat;
            let kind = typed.ty;

            match *pat {
                Pat::Ident(id) => {
                    let name = id.ident;
                    let mutable = id.mutability;

                    Ok(Argument {
                        mutable,
                        name,
                        kind: *kind,
                    })
                }
                Pat::Wild(wild) => {
                    let token = wild.underscore_token;

                    let name = Ident::new("_", token.spans[0]);

                    Ok(Argument {
                        mutable: None,
                        name,
                        kind: *kind,
                    })
                }
                _ => Err(Error::new(
                    pat.span(),
                    format_args!("unsupported pattern: {:?}", pat),
                )),
            }
        }
        FnArg::Receiver(_) => Err(Error::new(
            arg.span(),
            format_args!("`self` arguments are prohibited: {:?}", arg),
        )),
    }
}

/// Removes cooked attributes from a vector of attributes. Uncooked attributes are left in the vector.
///
/// # Return
///
/// Returns a vector of cooked attributes that have been removed from the input vector.
pub fn remove_cooked(attrs: &mut Vec<Attribute>) -> Vec<Attribute> {
    let mut cooked = Vec::new();

    // FIXME: Replace with `Vec::drain_filter` once it is stable.
    let mut i = 0;
    while i < attrs.len() {
        if !is_cooked(&attrs[i]) {
            i += 1;
            continue;
        }

        cooked.push(attrs.remove(i));
    }

    cooked
}

/// Test if the attribute is cooked.
pub fn is_cooked(attr: &Attribute) -> bool {
    const COOKED_ATTRIBUTE_NAMES: &[&str] = &[
        "cfg", "cfg_attr", "derive", "inline", "allow", "warn", "deny", "forbid",
    ];

    COOKED_ATTRIBUTE_NAMES.iter().any(|n| attr.path.is_ident(n))
}

macro_rules! match_options {
    ($v:expr, $values:ident, $options:ident, $span:expr => [$($name:ident);*]) => {
        match $v {
            $(
                stringify!($name) => $options.$name = propagate_err!($crate::attributes::parse($values)),
            )*
            _ => {
                return ::syn::parse::Error::new($span, format_args!("invalid attribute: {:?}", $v))
                    .to_compile_error()
                    .into();
            },
        }
    };
}
