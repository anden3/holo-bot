#[macro_export]
macro_rules! define_ids {
    ($($t:ident),*) => {
        $(
            #[derive(
                Debug,
                Clone,
                Copy,
                Default,
                PartialEq,
                Eq,
                PartialOrd,
                Ord,
                Hash,
                SerializeDisplay,
                DeserializeFromStr,
            )]
            pub struct $t(pub u64);

            impl std::fmt::Display for $t {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", self.0)
                }
            }

            impl std::str::FromStr for $t {
                type Err = std::num::ParseIntError;

                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    s.parse::<u64>().map(Self)
                }
            }
        )*
    }
}
