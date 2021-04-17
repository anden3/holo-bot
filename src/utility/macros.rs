#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

#[macro_export]
macro_rules! setup_interactions {
    ([$($i:ident),*]; $x:ident, $g:ident, $a:ident) => {
        $(
            if let Err(err) = commands::$i::setup_interaction(&$x, &$g, $a).await {
                error!("{}", err);
                return;
            }
        )*
    }
}

#[macro_export]
macro_rules! on_interactions {
    ([$($c:ident),*]; $x:ident, $i:ident) => {
        match $i.data.as_ref().unwrap().name.as_str() {
            $(
                stringify!($c) => crate::on_interaction!($c, $x, $i),
            )*
            _ => (),
        }
    }
}

#[macro_export]
macro_rules! on_interaction {
    ($c:ident, $x:ident, $i:ident) => {
        if let Err(err) = commands::$c::on_interaction(&$x, &$i).await {
            error!("{}", err);
            return;
        }
    };
}
