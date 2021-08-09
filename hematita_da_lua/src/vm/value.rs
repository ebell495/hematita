pub use self::{super::{Chunk, VirtualMachine}, Nillable::{Nil, NonNil}};
use std::{
	borrow::Borrow,
	collections::HashMap,
	fmt::{Debug, Display, Formatter, Result as FMTResult},
	hash::{Hash, Hasher},
	mem::take,
	ptr::{eq, hash},
	sync::{Arc, Mutex}
};

macro_rules! value_conversions {
	(
		impl$(<$($param:tt),*>)? for $convert:ident @ $for:ty $code:block
		$($rest:tt)*
	) => {
		impl$(<$($param),*>)? From<$for> for Value {
			fn from($convert: $for) -> Value {
				$code
			}
		}

		value_conversions! {$($rest)*}
	};
	() => {}
}

macro_rules! nillable_conversions {
	(
		impl$(<$($param:tt),*>)? all
		for $convert:ident @ $for:ty $code:block $($rest:tt)*
	) => {
		impl$(<$($param),*>)? IntoNillable for $for {
			#[inline]
			fn nillable(self) -> Nillable {
				let $convert = self;
				$code
			}
		}

		impl$(<$($param),*>)? From<$for> for Nillable {
			#[inline]
			fn from($convert: $for) -> Self {
				$code
			}
		}

		nillable_conversions! {$($rest)*}
	};
	(
		impl$(<$($param:tt),*>)?
		for $convert:ident @ $for:ty $code:block $($rest:tt)*
	) => {
		impl$(<$($param),*>)? IntoNillable for $for {
			#[inline]
			fn nillable(self) -> Nillable {
				let $convert = self;
				$code
			}
		}

		nillable_conversions! {$($rest)*}
	};
	() => {}
}

#[macro_export]
macro_rules! lua_value {
	($raw:literal) => {$crate::vm::value::Value::from($raw)};
	($($other:tt)*) => {Value::Table(lua_table! {$($other)*}.arc())}
}

#[macro_export]
macro_rules! lua_table {
	($($arm:tt)*) => {{
		#[allow(unused_assignments, unused_mut, unused_variables, unused_imports)]
		{
			use std::{collections::HashMap, default::Default, sync::Mutex};
			use $crate::{vm::value::{Table, Value}, lua_table_inner, lua_value};

			let mut table = HashMap::<Value, Value>::new();
			let mut counter = 1;

			lua_table_inner!(table counter {$($arm)*});

			Table {data: Mutex::new(table), ..Default::default()}
		}
	}}
}

#[macro_export]
macro_rules! lua_table_inner {
	($table:ident $counter:ident {[$key:expr] = $value:expr $(, $($rest:tt)*)?}) => {
		{
			$table.insert(lua_table_inner!($key), lua_table_inner!($value));
		}

		lua_table_inner!($table $counter {$($($rest)*)?});
	};
	($table:ident $counter:ident {$key:ident = $value:expr $(, $($rest:tt)*)?}) => {
		{
			$table.insert(Value::from(stringify!($key)), lua_table_inner!($value));
		}

		lua_table_inner!($table $counter {$($($rest)*)?});
	};
	($table:ident $counter:ident {$value:expr $(, $($rest:tt)*)?}) => {
		{
			$table.insert(Value::from($counter), lua_table_inner!($value));
			$counter += 1;
		}

		lua_table_inner!($table $counter {$($($rest)*)?});
	};
	($table:ident $counter:ident {$($rest:tt)*}) => {};

	($value:literal) => {lua_value!($value)};
	($value:expr) => {$value}
}

#[macro_export]
macro_rules! lua_tuple {
	($($arm:tt)*) => {{
		#[allow(unused_assignments, unused_mut, unused_variables, unused_imports)]
		{
			use std::{collections::HashMap, default::Default, sync::Mutex};
			use $crate::{vm::value::{Table, Value}, lua_tuple_inner, lua_value};

			let mut table = HashMap::<Value, Value>::new();
			let mut counter = 0;

			lua_tuple_inner!(table counter {$($arm)*});
			table.insert(Value::Integer(0), Value::Integer(counter));

			Table {data: Mutex::new(table), ..Default::default()}
		}
	}}
}

#[macro_export]
macro_rules! lua_tuple_inner {
	($table:ident $counter:ident {$value:expr $(, $($rest:tt)*)?}) => {
		{
			$counter += 1;
			if let NonNil(value) = IntoNillable::nillable(lua_tuple_inner!($value).clone()) {
				$table.insert(Value::Integer($counter), value);
			}
		}

		lua_tuple_inner!($table $counter {$($($rest)*)?});
	};
	($table:ident $counter:ident {}) => {};

	($value:literal) => {lua_value!($value)};
	($value:expr) => {$value}
}

pub trait UserData {
	fn type_name(&self) -> &'static str;
}

pub type NativeFunction<'r> = &'r dyn Fn(Arc<Table>, &VirtualMachine)
	-> Result<Arc<Table>, String>;

/// Represents a lua value.
// TODO: Add floats.
#[derive(Clone)]
pub enum Value {
	Integer(i64),
	String(Box<str>),
	Boolean(bool),
	Table(Arc<Table>),
	UserData {
		data: &'static dyn UserData,
		meta: Option<Arc<Table>>
	},
	Function(Arc<Function>),
	NativeFunction(NativeFunction<'static>)
}

impl Value {
	pub fn new_string(string: impl AsRef<str>) -> Self {
		Self::String(string.as_ref().to_owned().into_boxed_str())
	}

	pub fn type_name(&self) -> &'static str {
		match self {
			Self::Integer(_) => "number",
			Self::String(_) => "string",
			Self::Boolean(_) => "boolean",
			Self::Table(_) => "table",
			Self::UserData {data, ..} => data.type_name(),
			Self::Function(_) | Self::NativeFunction(_) => "function"
		}
	}

	/// Coerces this value to a bool. The rules are as follows; If the value is
	/// not a boolean, then true is returned, otherwise the value of the bool
	/// is returned.
	pub fn coerce_to_bool(&self) -> bool {
		match self {
			Self::Boolean(value) => *value,
			_ => true
		}
	}

	/// Like [coerce_to_bool], but wraps the result in a value.
	pub fn coerce_to_boolean(&self) -> Value {
		Value::Boolean(self.coerce_to_bool())
	}

	pub fn integer(&self) -> Option<i64> {
		match self {
			Self::Integer(integer) => Some(*integer),
			_ => None
		}
	}

	pub fn string(&self) -> Option<&str> {
		match self {
			Self::String(string) => Some(string),
			_ => None
		}
	}

	pub fn boolean(&self) -> Option<bool> {
		match self {
			Self::Boolean(boolean) => Some(*boolean),
			_ => None
		}
	}

	pub fn table(&self) -> Option<&Arc<Table>> {
		match self {
			Self::Table(table) => Some(table),
			_ => None
		}
	}

	pub fn function(&self) -> Option<&Arc<Function>> {
		match self {
			Self::Function(function) => Some(function),
			_ => None
		}
	}
}

impl Display for Value {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		match self {
			Self::Integer(integer) => write!(f, "{}", integer),
			Self::String(string) => write!(f, "{}", string),
			Self::Boolean(boolean) => write!(f, "{}", boolean),
			Self::Table(table) => write!(f, "{}", table),
			Self::UserData {..} => todo!(),
			Self::Function(function) => write!(f, "{}", function),
			Self::NativeFunction(function) => write!(f, "function: {:p}", *function)
		}
	}
}

impl Debug for Value {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		match self {
			Self::Integer(integer) => Debug::fmt(integer, f),
			Self::String(string) => Debug::fmt(string, f),
			Self::Boolean(boolean) => Debug::fmt(boolean, f),
			Self::Table(table) => Debug::fmt(table, f),
			Self::UserData {..} => todo!(),
			Self::Function(function) => Debug::fmt(function, f),
			Self::NativeFunction(function) => write!(f, "function: {:p}", function)
		}
	}
}

impl Eq for Value {}

impl PartialEq for Value {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Integer(a), Self::Integer(b)) => *a == *b,
			(Self::String(a), Self::String(b)) => *a == *b,
			(Self::Boolean(a), Self::Boolean(b)) => *a == *b,
			(Self::Function(a), Self::Function(b)) =>
				Arc::as_ptr(a) == Arc::as_ptr(b),
			(Self::Table(a), Self::Table(b)) =>
				Arc::as_ptr(a) == Arc::as_ptr(b),
			(Self::NativeFunction(a), Self::NativeFunction(b)) =>
				eq(*a as *const _ as *const u8, *b as *const _ as *const u8),
			_ => false
		}
	}
}

impl Hash for Value {
	fn hash<H>(&self, state: &mut H)
			where H: Hasher {
		match self {
			Self::Integer(integer) => integer.hash(state),
			Self::String(string) => string.hash(state),
			Self::Boolean(boolean) => boolean.hash(state),
			Self::Table(arc) => Arc::as_ptr(arc).hash(state),
			Self::UserData {data, ..} => hash(data, state),
			Self::Function(arc) => Arc::as_ptr(arc).hash(state),
			Self::NativeFunction(func) => hash(func, state)
		}
	}
}

value_conversions! {
	impl for value @ i64 {Value::Integer(value)}
	impl<'r> for value @ &'r str {Value::String(value.into())}
	impl for value @ Box<str> {Value::String(value)}
	impl for value @ String {Value::String(value.into_boxed_str())}
	impl for value @ bool {Value::Boolean(value)}
	impl for value @ Table {Value::Table(value.arc())}
	impl for value @ Arc<Table> {Value::Table(value)}
}

/// Represents a lua value that may be nil. This type has a lot in common with
/// the [Option] type, but this type has purpose built methods and trait
/// implementations for handling lua nil values. Unlike option, NillableValue
/// can only hold [Value]s or references to them.
#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Nillable {
	/// Variant for when the value is not nil.
	NonNil(Value),
	/// Variant for when the value is nil.
	Nil
}

impl Nillable {
	/// Get the human readable name of the type of this value.
	pub fn type_name(&self) -> &'static str {
		match self {
			NonNil(value) => value.borrow().type_name(),
			Nil => "nil"
		}
	}

	/// Convenience method for using [Into::into] or [From::from].
	pub fn option(self) -> Option<Value> {
		self.into()
	}

	/// Like [Value::coerce_to_bool], but also handles nil cases. The rules are as
	/// follows; If the value is nil or false, false is returned, otherwise true
	/// is.
	pub fn coerce_to_bool(&self) -> bool {
		match self {
			NonNil(value) => value.borrow().coerce_to_bool(),
			Nil => false
		}
	}

	/// Like [coerce_to_bool], but wraps the result in a value.
	pub fn coerce_to_boolean(&self) -> Value {
		Value::Boolean(self.coerce_to_bool())
	}

	pub fn is_nil(&self) -> bool {
		matches!(self, Nil)
	}

	pub fn is_non_nil(&self) -> bool {
		matches!(self, NonNil(_))
	}
}

impl Display for Nillable {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		match self {
			Nillable::NonNil(value) => write!(f, "{}", value.borrow()),
			Nil => write!(f, "nil")
		}
	}
}

impl Debug for Nillable {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		match self {
			Nillable::NonNil(value) => write!(f, "{:?}", value.borrow()),
			Nil => write!(f, "nil")
		}
	}
}

impl Default for Nillable {
	fn default() -> Self {
		Nil
	}
}

pub trait IntoNillable: Sized {
	fn nillable(self) -> Nillable;
}

nillable_conversions! {
	// From Option

	impl all for value @ Option<Value> {
		match value {
			Some(value) => NonNil(value),
			None => Nil
		}
	}

	impl<'r> for value @ Option<&'r Value> {
		match value {
			Some(value) => NonNil(value.clone()),
			None => Nil
		}
	}

	// From Self or Value

	impl for value @ Nillable {value}
	impl all for value @ Value {NonNil(value)}

	// From Into<Value>

	impl all for value @ i64 {NonNil(value.into())}
	impl<'r> all for value @ &'r str {NonNil(value.into())}
	impl all for value @ Box<str> {NonNil(value.into())}
	impl all for value @ String {NonNil(value.into())}
	impl all for value @ bool {NonNil(value.into())}
	impl all for value @ Table {NonNil(value.into())}
	impl all for value @ Arc<Table> {NonNil(value.into())}
	impl all for _value @ () {Nil}
}

impl From<Nillable> for Option<Value> {
	fn from(nillable: Nillable) -> Self {
		match nillable {
			NonNil(value) => Some(value),
			Nil => None
		}
	}
}

#[derive(Clone, Debug)]
pub enum MaybeUpValue {
	UpValue(Arc<Mutex<Nillable>>),
	Normal(Nillable)
}

impl MaybeUpValue {
	pub fn up_value(&mut self) -> &Arc<Mutex<Nillable>> {
		match self {
			Self::UpValue(up_value) => up_value,
			Self::Normal(normal) => {
				let normal = Arc::new(Mutex::new(std::mem::replace(normal, Nil)));
				*self = Self::UpValue(normal);
				match self {
					Self::UpValue(up_value) => up_value,
					_ => unreachable!()
				}
			}
		}
	}
}

impl Default for MaybeUpValue {
	fn default() -> Self {
		Self::Normal(Nil)
	}
}

#[derive(Default)]
pub struct Table {
	pub data: Mutex<HashMap<Value, Value>>,
	pub metatable: Mutex<Option<Arc<Table>>>
}

impl Table {
	/// Inserts a value into this table as if it was an array.
	#[inline]
	pub fn array_insert(&self, index: i64, mut value: Nillable) {
		let len = self.array_len();
		let mut data = self.data.lock().unwrap();

		(index..=(len.max(1) + 1))
			.for_each(|index| match take(&mut value) {
				NonNil(new) =>
					value = data.insert(Value::Integer(index), new).nillable(),
				Nil =>
					value = data.remove(&Value::Integer(index)).nillable()
			});
	}

	#[inline]
	pub fn array_remove(&self, index: i64) -> Nillable {
		let len = self.array_len();
		let mut data = self.data.lock().unwrap();

		let mut value = Nil;
		(index..=len).rev()
			.for_each(|index| match take(&mut value) {
				NonNil(new) =>
					value = data.insert(Value::Integer(index as i64), new).nillable(),
				Nil =>
					value = data.remove(&Value::Integer(index as i64)).nillable()
			});
		value
	}

	#[inline]
	pub fn array_push(&self, value: Nillable) {
		self.array_insert(self.array_len(), value)
	}

	pub fn array_len(&self) -> i64 {
		self.data.lock().unwrap().iter()
			.filter_map(|(key, _)| key.integer())
			.fold(0, |result, index| result.max(index))
	}

	pub fn array_is_empty(&self) -> bool {
		self.data.lock().unwrap().iter()
			.any(|(key, _)| key.integer().is_some())
	}

	/// Inserts a value into this table as if it was a tuple.
	#[inline]
	pub fn tuple_insert(&self, index: i64, mut value: Nillable) {
		let len = self.tuple_len();
		let mut data = self.data.lock().unwrap();
		data.insert(Value::Integer(0), Value::Integer(len + 1));

		(index..=(len.max(1) + 1))
			.for_each(|index| match take(&mut value) {
				NonNil(new) =>
					value = data.insert(Value::Integer(index), new).nillable(),
				Nil =>
					value = data.remove(&Value::Integer(index)).nillable()
			});
	}

	pub fn tuple_len(&self) -> i64 {
		self.data.lock().unwrap().get(&Value::Integer(0))
			.unwrap().integer().unwrap()
	}

	pub fn index(&self, index: &Value) -> Nillable {
		let data = self.data.lock().unwrap();
		data.get(index).nillable()
	}

	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

impl PartialEq for Table {
	fn eq(&self, other: &Table) -> bool {
		eq(self, other)
	}
}

impl Display for Table {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		write!(f, "table: {:p}", &*self)
	}
}

impl Debug for Table {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		match self.data.try_lock() {
			Ok(data) => {
				let mut first = true;
				let mut comma = || {
					if first {first = false; ""}
					else {", "}
				};

				write!(f, "{{")?;
				let mut array = data.iter()
					.filter_map(|(key, value)| if let Value::Integer(key) = key
						{Some((key, value))} else {None})
					.collect::<Vec<_>>();
				array.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
				if let Some((highest, _)) = array.last() {
					(1..=**highest)
						.map(|index| array.iter().find(|value| *value.0 == index)
							.map(|(_, value)| *value))
						.try_for_each(|value| write!(f, "{}{:?}", comma(), value.nillable()))?;
				}

				data.iter()
					.try_for_each(|(key, value)| match key {
						Value::Integer(_) => Ok(()),
						key => write!(f, "{}[{:?}] = {:?}", comma(), key, value)
					})?;

				write!(f, "}}")
			},
			Err(_) => write!(f, "{{<table is being accessed>}}")
		}
	}
}

#[derive(Debug)]
pub struct Function {
	pub up_values: Box<[Arc<Mutex<Nillable>>]>,
	pub chunk: Arc<Chunk>
}

impl Function {
	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

impl PartialEq for Function {
	fn eq(&self, other: &Function) -> bool {
		eq(self, other)
	}
}

impl Eq for Function {}

impl Display for Function {
	fn fmt(&self, f: &mut Formatter) -> FMTResult {
		write!(f, "function: {:p}", &self)
	}
}

impl From<Chunk> for Function {
	fn from(chunk: Chunk) -> Self {
		Self {chunk: chunk.arc(), up_values: vec![].into_boxed_slice()}
	}
}
