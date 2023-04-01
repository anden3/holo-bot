use std::{
    fs,
    io::{ErrorKind, Read},
    path::Path,
};

use anyhow::{bail, Context};
use serde::{de::DeserializeOwned, Serialize};
use tracing::warn;

use crate::here;

pub(crate) fn load_toml_file_or_create_default<T>(path: &Path) -> anyhow::Result<T>
where
    T: Serialize,
    T: DeserializeOwned,
    T: std::default::Default,
{
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => match e.kind() {
            ErrorKind::NotFound => {
                let default_value = T::default();
                let default_file = toml::to_string_pretty(&default_value).context(here!())?;
                fs::write(path, default_file).context(here!())?;

                warn!(
                    "Config file not found! Creating a default file at {}.",
                    path.display()
                );

                return Ok(default_value);
            }
            ErrorKind::PermissionDenied => bail!(
                "Insufficient permissions to open config file at {}: {}.",
                path.display(),
                e
            ),
            _ => bail!("Could not open config file at {}: {}", path.display(), e),
        },
    };

    let mut file_str = String::new();
    file.read_to_string(&mut file_str).context(here!())?;

    let data: T = toml::from_str(&file_str).context(here!())?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn serenity_id_serialization() {
        use serenity::model::id::UserId;

        let id = UserId(123456789012345678);
        let serialized = toml::to_string(&id).unwrap();
        assert_eq!(serialized, "123456789012345678");

        let deserialized: UserId = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized, id);
    }
}
