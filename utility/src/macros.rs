#[macro_export]
macro_rules! here {
    () => {
        concat!("at ", file!(), ":", line!(), ":", column!())
    };
}

#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

#[macro_export]
macro_rules! get_interactions {
    ($v:ident, $($g:ident),*) => {
        {
            let mut cmds = Vec::new();

            $(
                paste::paste! {
                    for cmd in commands::[<$g:upper _INTERACTION_GROUP_OPTIONS>].commands {
                        cmds.push((cmd, commands::[<$g:upper _INTERACTION_GROUP>].options));
                    }
                }
            )*

            cmds
        }
    }
}

#[macro_export]
macro_rules! client_data_types {
    ($($t:ty),*) => {
        $(
            impl TypeMapKey for $t {
                type Value = Self;
            }
        )*
    }
}

#[macro_export]
macro_rules! wrap_type_aliases {
    ($($n:ident|$t:ty),*) => {
        $(
            pub struct $n(pub $t);

            impl Deref for $n {
                type Target = $t;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
        )*
    }
}

#[macro_export]
macro_rules! define_command_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            mod $c;
        )*

        $(
            paste::paste! { use $c::[<$c:upper _COMMAND>]; }
        )*

        #[group]
        #[commands(
            $(
                $c,
            )*
        )]
        struct $g;
    }
}

#[macro_export]
macro_rules! define_interactions {
    ($($c:ident),*) => {
        $(
            pub mod $c;
        )*
    }
}

#[macro_export]
macro_rules! define_slash_command_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            pub mod $c;
        )*

        $(
            paste::paste! { use $c::[<$c:upper _INTERACTION>]; }
        )*

        #[interaction_group]
        #[commands(
            $(
                $c,
            )*
        )]
        struct $g;
    }
}

#[macro_export]
macro_rules! setup_interactions {
    (/* $ctx:ident, $guild:ident, $id:ident, $client:ident, $token:expr,  */[$($cmd:ident),*]) => {
        {
            let mut cmds = Vec::new();

            $(
                match commands::$cmd::setup(/* &$ctx, &$guild, $id, &$client, $token */).await {
                    Ok((c, o)) => cmds.push(RegisteredInteraction {
                        name: stringify!($cmd),
                        command: None,
                        func: commands::$cmd::$cmd,
                        options: o,
                        config_json: c,
                    }),
                    Err(e) => ::log::error!("{:?} {}", e, here!()),
                }
            )*

            cmds
        }
    }
}
