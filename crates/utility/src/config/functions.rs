use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{ErrorKind, Read},
    path::Path,
};

use anyhow::{bail, Context};
use serde::{de::DeserializeOwned, Serialize};
use tracing::warn;

use crate::here;

use super::ConfigUpdate;

pub(crate) fn get_set_updates<'a, T, Add, Del>(
    old: &'a HashSet<T>,
    new: &'a HashSet<T>,
    if_removed: Del,
    if_added: Add,
) -> impl Iterator<Item = ConfigUpdate> + 'a
where
    T: Eq + Copy + std::hash::Hash,
    Add: 'a + Fn(T) -> ConfigUpdate,
    Del: 'a + Fn(T) -> ConfigUpdate,
{
    old.difference(new)
        .map(move |r| if_removed(*r))
        .chain(new.difference(old).map(move |r| if_added(*r)))
}

pub(crate) fn get_map_updates<'a, Output, Key, Val, Add, Del, Changed>(
    old: &'a HashMap<Key, Val>,
    new: &'a HashMap<Key, Val>,
    if_removed: Del,
    if_added: Add,
    if_changed: Changed,
) -> impl Iterator<Item = Output> + 'a
where
    Key: Eq + Copy + std::hash::Hash,
    Val: Clone + std::cmp::PartialEq,
    Add: 'a + Fn(Key, Val) -> Output,
    Del: 'a + Fn(Key) -> Output,
    Changed: 'a + Fn(Key, Val) -> Output,
{
    old.keys()
        .filter(move |k| !new.contains_key(k))
        .map(move |k| if_removed(*k))
        .chain(
            new.iter()
                .filter(move |(k, _)| !old.contains_key(k))
                .map(move |(k, v)| if_added(*k, v.clone())),
        )
        .chain(new.iter().filter_map(move |(k, v)| {
            if old.get(k) == Some(v) {
                None
            } else {
                Some(if_changed(*k, v.clone()))
            }
        }))
}

pub(crate) fn get_nested_map_updates<'a, Output, FirstKey, SecondKey, Val, Add, Del, Changed>(
    old: &'a HashMap<FirstKey, HashMap<SecondKey, Val>>,
    new: &'a HashMap<FirstKey, HashMap<SecondKey, Val>>,
    if_removed: Del,
    if_added: Add,
    if_changed: Changed,
) -> impl Iterator<Item = Output> + 'a
where
    FirstKey: Eq + Copy + std::hash::Hash,
    SecondKey: Eq + Copy + std::hash::Hash,
    Val: Clone + std::cmp::PartialEq,
    Add: 'a + Fn((FirstKey, SecondKey), Val) -> Output,
    Del: 'a + Fn((FirstKey, SecondKey)) -> Output,
    Changed: 'a + Fn((FirstKey, SecondKey), Val) -> Output,
{
    enum State<K1, K2, V> {
        Removed(K1, K2),
        Added((K1, K2), V),
        Changed((K1, K2), V),
    }

    let removed_entries = old
        .iter()
        .filter(move |(k, _)| !new.contains_key(k))
        .flat_map(|(k1, m)| m.keys().map(move |k2| (k1, k2)))
        .map(move |(k1, k2)| State::Removed(*k1, *k2));

    let added_entries = new
        .iter()
        .filter(move |(k, _)| !old.contains_key(k))
        .flat_map(|(k1, m)| m.iter().map(move |(k2, v)| ((*k1, *k2), v.clone())))
        .map(move |((k1, k2), v)| State::Added((k1, k2), v));

    let nested_entries = new
        .iter()
        .filter(move |(k1, _)| old.contains_key(k1))
        .flat_map(move |(k1, m)| {
            get_map_updates(
                old.get(k1).unwrap(),
                m,
                move |k2| State::Removed(*k1, k2),
                move |k2, v| State::Added((*k1, k2), v),
                move |k2, v| State::Changed((*k1, k2), v),
            )
        });

    removed_entries
        .chain(added_entries)
        .chain(nested_entries)
        .map(move |state| match state {
            State::Removed(k1, k2) => if_removed((k1, k2)),
            State::Added((k1, k2), v) => if_added((k1, k2), v),
            State::Changed((k1, k2), v) => if_changed((k1, k2), v),
        })
}

pub(crate) fn load_toml_file_or_create_default<T>(path: &Path) -> anyhow::Result<T>
where
    T: Serialize,
    T: DeserializeOwned,
    T: std::default::Default,
{
    let mut file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => match e.kind() {
            ErrorKind::NotFound => {
                let default_value = T::default();
                let default_file = toml::to_string_pretty(&default_value).context(here!())?;
                fs::write(&path, default_file).context(here!())?;

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
