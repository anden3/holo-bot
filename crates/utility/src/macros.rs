#[macro_export]
macro_rules! here {
    () => {
        concat!("at ", file!(), ":", line!(), ":", column!())
    };
}

#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: ::once_cell::sync::OnceCell<::regex::Regex> = ::once_cell::sync::OnceCell::new();
        RE.get_or_init(|| ::regex::Regex::new($re).unwrap())
    }};
}

#[macro_export]
macro_rules! regex_lazy {
    ($re:literal $(,)?) => {
        ::once_cell::sync::Lazy::<::regex::Regex>::new(|| regex::Regex::new($re).unwrap());
    };
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
    () => {};

    ( mut $n:ident = $t:ty; $($rest:tt)* ) => {
        pub struct $n(pub $t);

        impl Deref for $n {
            type Target = $t;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $n {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl From<$t> for $n {
            fn from(t: $t) -> Self {
                $n(t)
            }
        }

        wrap_type_aliases!($($rest)*);
    };

    ( $n:ident = $t:ty; $($rest:tt)* ) => {
        pub struct $n(pub $t);

        impl Deref for $n {
            type Target = $t;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<$t> for $n {
            fn from(t: $t) -> Self {
                $n(t)
            }
        }

        wrap_type_aliases!($($rest)*);
    };
}

#[macro_export]
macro_rules! define_command_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            mod $c;
        )*

        $(
            use $c::*;
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
