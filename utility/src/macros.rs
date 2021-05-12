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
macro_rules! define_interaction_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            pub mod $c;
        )*

        paste::paste! {
            pub static [<$g:upper _INTERACTION_GROUP>]: InteractionGroup = InteractionGroup {
                name: stringify!($g),
                interactions: &[$(
                    &$c::[<$c:upper _INTERACTION>],
                )*],
            };
        }
    }
}

#[macro_export]
macro_rules! setup_interaction_groups {
    ($guild:ident, [$($grp:ident),*]) => {{
        let mut cmds = Vec::new();

        $(
            for interaction in paste::paste! { commands::[<$grp:upper _INTERACTION_GROUP>] }.interactions {
                match (interaction.setup)(&$guild).await {
                    Ok((c, o)) => cmds.push(RegisteredInteraction {
                        name: interaction.name,
                        command: None,
                        func: interaction.func,
                        options: o,
                        config_json: c,
                        global_rate_limits: ::tokio::sync::RwLock::new((0, ::chrono::Utc::now())),
                        user_rate_limits: ::tokio::sync::RwLock::new(HashMap::new()),
                    }),
                    Err(e) => ::log::error!("{:?} {}", e, here!()),
                }
            }
        )*

        cmds
    }}
}
