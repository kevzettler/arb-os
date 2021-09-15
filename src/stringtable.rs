/*
 * Copyright 2020, Offchain Labs, Inc. All rights reserved.
 */

use serde::{Deserialize, Serialize, Deserializer, de};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;
use serde::de::{Visitor, MapAccess, Error};

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
pub struct StringId {
    path: Vec<String>,
    id: usize,
}

impl fmt::Display for StringId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for component in &self.path {
            write!(f, "{}::", component)?
        }
        write!(f, "{}", self.id)
    }
}

struct StringIdVisitor;

impl<'de> Visitor<'de> for StringIdVisitor {
    type Value = StringId;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expected hex string")
    }
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
    {
        Ok(StringId::new(vec![], v as usize))
    }
    fn visit_map<E>(self, mut map: E) -> Result<Self::Value, E::Error>
        where E: MapAccess<'de> {
        let (path_key, path) = map.next_entry::<&str, Vec<String>>()?.unwrap();
        if path_key != "path" {
            return Err(E::Error::custom("wrong key name: expected \"path\""));
        }
        let (id_key, id) = map.next_entry::<&str, u64>()?.unwrap();
        if id_key != "id" {
            return Err(E::Error::custom("wrong key name: expected \"id\""));
        }
        Ok(StringId::new(path, id as usize))
    }
}

impl<'de> Deserialize<'de> for StringId {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
        where
            D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StringIdVisitor)
    }
}

impl StringId {
    pub(crate) fn new(path: Vec<String>,
                      id: usize,) -> Self {
        Self {
            path,
            id,
        }
    }
}

/// Maps `String`s to `usize` IDs.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StringTable {
    next_id: usize,
    table: HashMap<String, StringId>,
    by_id: Vec<String>,
    path: Vec<String>,
}

impl StringTable {
    pub fn new(path: Vec<String>) -> Self {
        let table: HashMap<String, StringId> = HashMap::new();
        let by_id = Vec::new();
        StringTable {
            next_id: 0,
            table,
            by_id,
            path,
        }
    }

    /// Returns the `StringID` associated with `name` if it exists, if not creates a new entry and
    /// returns the newly created ID.
    pub fn get(&mut self, name: String) -> StringId {
        match self.table.get(&name) {
            Some(id) => id.clone(),
            None => {
                let new_id = self.next_id;
                self.next_id += 1;
                let new_full_id = StringId::new(self.path.clone(), new_id);
                self.table.insert(name.clone(), new_full_id.clone());
                self.by_id.push(name);
                new_full_id
            }
        }
    }

    /// If an ID exists, returns it, if not returns `None`.
    pub fn get_if_exists(&self, name: &str) -> Option<StringId> {
        self.table.get(name).cloned()
    }

    /// Takes a `usize` ID and returns the associated `String`
    pub fn name_from_id(&self, name: StringId) -> &String {
        &self.by_id[name.id as usize]
    }
}
