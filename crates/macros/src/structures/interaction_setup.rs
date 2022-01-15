use crate::consts::INTERACTION;

use super::{
    prelude::*, Check, InteractionField, InteractionOpt, InteractionRestriction, RateLimit,
};

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
    is_enabled: Option<ExprClosure>,
}

impl Parse for InteractionSetup {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let mut fields = Vec::new();

        while input.peek(Ident) {
            fields.push(input.parse::<InteractionField>()?);
        }

        let mut name = String::new();
        let mut group = String::new();
        let mut description = String::new();
        let mut options = Vec::new();
        let mut restrictions = Vec::new();
        let mut is_enabled = None;

        for field in fields {
            match field {
                InteractionField::Name(s) => name = s,
                InteractionField::Group(g) => group = g,
                InteractionField::Description(s) => description = s,
                InteractionField::Options(o) => options.extend(o),
                InteractionField::Restrictions(r) => restrictions.extend(r),
                InteractionField::IsEnabled(e) => is_enabled = Some(e),
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
            is_enabled,
        })
    }
}

#[allow(unstable_name_collisions)]
impl ToTokens for InteractionSetup {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let sub_command_count: usize = std::cmp::max(
            1,
            self.options
                .iter()
                .map(|o| match o.ty.to_string().as_str() {
                    "SubCommand" => o.names.len(),
                    "SubCommandGroup" => {
                        o.options
                            .iter()
                            .filter(|o| o.ty.to_string().as_str() == "SubCommand")
                            .map(|o| o.names.len())
                            .sum::<usize>()
                            * o.names.len()
                    }
                    _ => 0,
                })
                .sum(),
        );

        if sub_command_count > 25 {
            let error = quote! { compile_error!("Too many subcommands!"); };
            error.to_tokens(tokens);
            return;
        }

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
        let is_enabled = match &self.is_enabled {
            Some(e) => quote! { Some(#e) },
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
        let slice_name = format_ident!("{}_COMMANDS", group.to_uppercase());
        let declaration_path = quote!(DeclaredInteraction);

        let result = quote! {
            use super::#slice_name;

            #[allow(missing_docs)]
            #[distributed_slice(#slice_name)]
            pub static #n: #declaration_path = #declaration_path {
                name: #name,
                group: #group,
                enabled: #is_enabled,
                setup: setup,
                func: #name_ident,
            };

            #[allow(missing_docs)]
            pub fn setup<'fut>(guild: &'fut Guild) -> ::futures::future::BoxFuture<'fut, anyhow::Result<(::bytes::Bytes, InteractionOptions)>> {
                use ::futures::future::FutureExt;
                use ::serenity::{model::interactions::application_command::ApplicationCommand, http::{request::RequestBuilder, routing::RouteInfo}};
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
