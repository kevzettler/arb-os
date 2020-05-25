/*
 * Copyright 2020, Offchain Labs, Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

 use std::collections::HashMap;
use crate::stringtable::StringId;


#[derive(Debug)]
#[derive(Clone)]
pub enum SymTable<'a, T> {
	Empty,
	Single(StringId, &'a T, &'a SymTable<'a, T>),
	Multi(HashMap<StringId, &'a T>, &'a SymTable<'a, T>)
}

impl<'a, T> SymTable<'a, T> {
	pub fn new() -> Self {
		SymTable::Empty
	}

	pub fn push_one(self: &'a SymTable<'a, T>, sid: StringId, t: &'a T) -> Self {
		SymTable::Single(sid, t, self)
	}

	pub fn push_multi(self: &'a SymTable<'a, T>, hm: HashMap<StringId, &'a T>) -> Self {
		SymTable::Multi(hm, self)
	}

	pub fn get(&self, sid: StringId) -> Option<&'a T> {
		match self {
			SymTable::Empty => None,
			SymTable::Single(s, t, rest) => if *s==sid {
					Some(t)
				} else {
					rest.get(sid)
				},
			SymTable::Multi(hm, rest) => match hm.get(&sid) {
				Some(t) => Some(t),
				None => rest.get(sid)
			}
		}
	}
}

#[derive(Clone)]
pub enum CopyingSymTable<'a, T: Copy> {
	Empty,
	Single(StringId, T, &'a CopyingSymTable<'a, T>),
	Multi(HashMap<StringId, T>, &'a CopyingSymTable<'a, T>)
}

impl<'a, T: Copy> CopyingSymTable<'a, T> {
	pub fn new() -> Self {
		CopyingSymTable::Empty
	}

	pub fn push_one(self: &'a CopyingSymTable<'a, T>, sid: StringId, t: T) -> Self {
		CopyingSymTable::Single(sid, t, self)
	}

	pub fn push_multi(self: &'a CopyingSymTable<'a, T>, hm: HashMap<StringId, T>) -> Self {
		CopyingSymTable::Multi(hm, self)
	}

	pub fn get(&self, sid: StringId) -> Option<T> {
		match self {
			CopyingSymTable::Empty => None,
			CopyingSymTable::Single(s, t, rest) => if *s==sid {
					Some(*t)
				} else {
					rest.get(sid)
				},
			CopyingSymTable::Multi(hm, rest) => match hm.get(&sid) {
				Some(t) => Some(*t),
				None => rest.get(sid)
			}
		}
	}
}
