use super::{parse_interaction_options::ParseInteractionOption, prelude::*};

#[derive(Debug)]
pub struct MatchSubCommands(Vec<MatchSubCommand>);

impl IntoIterator for MatchSubCommands {
    type Item = MatchSubCommand;
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Parse for MatchSubCommands {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut opts = Vec::new();

        while !input.is_empty() {
            opts.push(input.parse::<MatchSubCommand>()?);
        }

        Ok(Self(opts))
    }
}

impl ToTokens for MatchSubCommands {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let sub_commands = self
            .0
            .iter()
            .fold(HashMap::new(), |mut map, cmd| {
                map.entry(&cmd.sub_command_group)
                    .or_insert_with(Vec::new)
                    .push(cmd);
                map
            })
            .into_iter()
            .map(|(group, cmds)| {
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

        let output = quote! {
            for cmd in &interaction.data.options {
                match cmd.name.as_str() {
                    #(#sub_commands)*

                    _ => (),
                }
            }
        };

        output.to_tokens(tokens);
    }
}

#[derive(Debug)]
pub struct MatchSubCommand {
    sub_command: String,
    sub_command_group: Option<String>,
    args: Option<Punctuated<ParseInteractionOption, Token![,]>>,
    expr: ExprBlock,
}

impl Parse for MatchSubCommand {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = input.parse::<LitStr>()?.value();
        let command_parts = name.split_ascii_whitespace().collect::<Vec<_>>();

        let (sub_command, sub_command_group) = match &command_parts[..] {
            [] => return Err(input.error("Empty sub-command name.")),
            [command] => (command.to_string(), None),
            [group, command] => (command.to_string(), Some(group.to_string())),
            _ => return Err(input.error("Commands can only be nested two levels deep.")),
        };

        input.parse::<Token![=>]>()?;

        let args = if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;
            let args =
                Punctuated::<ParseInteractionOption, Token![,]>::parse_separated_nonempty(&input)?;
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
            sub_command,
            sub_command_group,
            args,
            expr,
        })
    }
}

impl ToTokens for MatchSubCommand {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
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

                                _ => ::log::error!(
                                    "Unknown option '{}' found for command '{}'.",
                                    option.name,
                                    file!()
                                ),
                            }
                        }
                    }
                    // parse_interaction_options!(cmd, [#args]);
                }
            }
            None => TokenStream2::new(),
        };

        let sub_command = &self.sub_command;
        let expr = &self.expr;

        let output = quote! {
            #sub_command => {
                #args
                #expr
                break;
            },
        };

        /* let output = match &self.sub_command_group {
            Some(group) => quote! {
                #group => {
                    for cmd in &cmd.options {
                        match cmd.name.as_str() {
                            #sub_command
                            _ => (),
                        }
                    }
                    break;
                },
            },
            None => sub_command,
        }; */

        output.to_tokens(tokens);
    }
}
