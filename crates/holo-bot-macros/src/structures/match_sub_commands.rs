use super::{parse_interaction_options::ParseInteractionOption, prelude::*};

#[derive(Debug)]
pub struct MatchSubCommands {
    commands: Vec<MatchSubCommand>,
    result_type: Option<Ident>,
}

impl IntoIterator for MatchSubCommands {
    type Item = MatchSubCommand;
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.into_iter()
    }
}

impl Parse for MatchSubCommands {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut commands = Vec::new();

        let result_type = if input.peek(Token![type]) {
            input.parse::<Token![type]>()?;
            let result = input.parse()?;

            input.parse::<Token![,]>()?;

            let content;
            bracketed!(content in input);

            while !content.is_empty() {
                commands.push(content.parse::<MatchSubCommand>()?);
            }

            Some(result)
        } else {
            while !input.is_empty() {
                commands.push(input.parse::<MatchSubCommand>()?);
            }
            None
        };

        Ok(Self {
            commands,
            result_type,
        })
    }
}

impl ToTokens for MatchSubCommands {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let sub_commands = self
            .commands
            .iter()
            .fold(HashMap::new(), |mut map, cmd| {
                map.entry(&cmd.sub_command_group)
                    .or_insert_with(Vec::new)
                    .push(cmd);
                map
            })
            .into_iter()
            .map(|(group, cmds)| {
                let cmds = cmds.iter().map(|c| {
                    let mut ts = TokenStream2::new();
                    c.to_tokens_with_type(self.result_type.is_some(), &mut ts);
                    ts
                });

                if let Some(grp) = group {
                    quote! {
                        #grp => {
                            for cmd in &cmd.options {
                                match cmd.name.as_str() {
                                    #(#cmds)*
                                    _ => (),
                                }
                            }
                            break;
                        },
                    }
                } else {
                    quote! {
                        #(#cmds)*
                    }
                }
            });

        let output = if self.result_type.is_some() {
            let ty = self.result_type.as_ref().unwrap();

            quote! {{
                let mut return_value: Option<#ty> = None;

                for cmd in &interaction.data.options {
                    match cmd.name.as_str() {
                        #(#sub_commands)*
                        _ => (),
                    }
                }

                return_value
            }}
        } else {
            quote! {
                for cmd in &interaction.data.options {
                    match cmd.name.as_str() {
                        #(#sub_commands)*
                        _ => (),
                    }
                }
            }
        };

        // panic!("{}", output);

        output.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub struct MatchSubCommand {
    aliases: Vec<String>,
    sub_command_group: Option<String>,
    args: Option<Punctuated<ParseInteractionOption, Token![,]>>,
    expr: ExprBlock,
}

impl Parse for MatchSubCommand {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut aliases = vec![input.parse()?];

        if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;

            aliases.extend(Punctuated::<LitStr, Token![|]>::parse_separated_nonempty(
                input,
            )?);
        }

        let aliases = aliases.iter().map(|a| a.value()).collect::<Vec<String>>();

        let (aliases, groups) = aliases.iter().try_fold(
            (Vec::new(), HashSet::new()),
            |(mut names, mut groups), command| {
                let command_parts = command.split_ascii_whitespace().collect::<Vec<_>>();

                let (cmd, grp) = match &command_parts[..] {
                    [] => return Err(input.error("Empty sub-command name.")),
                    [command] => (command.to_string(), None),
                    [group, command] => (command.to_string(), Some(group.to_string())),
                    _ => return Err(input.error("Commands can only be nested two levels deep.")),
                };

                names.push(cmd);
                groups.insert(grp);

                Ok((names, groups))
            },
        )?;

        if groups.len() > 1 {
            return Err(input.error("Sub-command aliases must be in the same group."));
        }

        let sub_command_group = groups.iter().next().unwrap().clone();

        input.parse::<Token![=>]>()?;

        let args = if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;
            let args =
                Punctuated::<ParseInteractionOption, Token![,]>::parse_separated_nonempty(input)?;
            input.parse::<Token![|]>()?;

            Some(args)
        } else {
            None
        };

        let expr = input.parse()?;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(Self {
            aliases,
            sub_command_group,
            args,
            expr,
        })
    }
}

impl MatchSubCommand {
    pub fn to_tokens_with_type(&self, returns_type: bool, tokens: &mut TokenStream2) {
        let args = match &self.args {
            Some(args) => {
                let options = args.iter();
                let declarations = args.iter().map(|o| o.declare_variable());

                quote! {
                    #(#declarations)*

                    for option in &cmd.options {
                        if let Some(value) = &option.value {
                            match option.name.as_str() {
                                #(#options)*

                                _ => {
                                    ::log::error!(
                                        "Unknown option '{}' found for command '{}'.",
                                        option.name,
                                        file!()
                                   );
                                }
                            }
                        }
                    }
                }
            }
            None => TokenStream2::new(),
        };

        let expr = &self.expr;

        let output: TokenStream2 = match returns_type {
            false => self
                .aliases
                .iter()
                .map(|a| {
                    quote! {
                        #a => {
                            #args
                            #expr;
                            break;
                        },
                    }
                })
                .collect(),
            true => self
                .aliases
                .iter()
                .map(|a| {
                    quote! {
                        #a => {
                            #args
                            return_value = Some(#expr);
                            break;
                        },
                    }
                })
                .collect(),
        };

        output.to_tokens(tokens);
    }
}
