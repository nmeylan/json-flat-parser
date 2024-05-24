use std::fmt::{Display};
use std::hash::{Hash, Hasher};
use crate::lexer::Lexer;
use crate::parser::Parser;

pub mod parser;
pub mod lexer;
mod serializer;

pub(crate) type Arena = bumpalo::Bump;
// pub(crate) type Arena = typed_arena::Arena<u8>;

pub struct JSONParser<'a> {
    pub parser: Parser<'a>,
}

#[derive(Clone)]
pub struct ParseOptions {
    pub parse_array: bool,
    pub keep_object_raw_data: bool,
    pub max_depth: u8,
    pub start_parse_at: Option<String>,
    pub prefix: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            parse_array: true,
            keep_object_raw_data: true,
            max_depth: 10,
            start_parse_at: None,
            prefix: None,
        }
    }
}

impl ParseOptions {
    pub fn parse_array(mut self, parse_array: bool) -> Self {
        self.parse_array = parse_array;
        self
    }

    pub fn start_parse_at(mut self, pointer: String) -> Self {
        self.start_parse_at = Some(pointer);
        self
    }
    pub fn max_depth(mut self, max_depth: u8) -> Self {
        self.max_depth = max_depth;
        self
    }
    pub fn prefix(mut self, prefix: String) -> Self {
        self.prefix = Some(prefix);
        self
    }
    pub fn keep_object_raw_data(mut self, keep_object_raw_data: bool) -> Self {
        self.keep_object_raw_data = keep_object_raw_data;
        self
    }
}

#[derive(Debug, Clone)]
pub struct JsonArrayEntries<'arena> {
    entries: FlatJsonValue<'arena>,
    index: usize,
}

impl <'arena>JsonArrayEntries<'arena> {
    pub fn entries(&self) -> &FlatJsonValue<'arena> {
        &self.entries
    }
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn find_node_at(&self, pointer: &str) -> Option<&(PointerKey, Option<String>)> {
        self.entries().iter().find(|(p, _)| p.pointer.eq(pointer))
    }
}


#[derive(Debug, Default, Clone)]
pub struct PointerKey<'arena> {
    pub pointer: &'arena str,
    pub value_type: ValueType,
    pub depth: u8,    // depth of the pointed value in the json
    pub index: usize, // index in the root json array
    pub position: usize, // position on the original json
}

impl <'arena>PartialEq<Self> for PointerKey<'arena> {
    fn eq(&self, other: &Self) -> bool {
        self.pointer.eq(other.pointer)
    }
}

impl <'arena>Eq for PointerKey<'arena> {}

impl <'arena>Hash for PointerKey<'arena> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pointer.hash(state);
    }
}

impl <'arena>PointerKey<'arena> {
    pub fn parent(&self) -> &str {
        let index = self.pointer.rfind('/').unwrap_or(0);
        let parent_pointer = if index == 0 {
            "/"
        } else {
            &self.pointer[0..index]
        };
        parent_pointer
    }
}

macro_rules! concat_string {
    () => { String::with_capacity(0) };
    ($($s:expr),+) => {{
        use std::ops::AddAssign;
        let mut len = 0;
        $(len.add_assign(AsRef::<str>::as_ref(&$s).len());)+
        let mut buf = String::with_capacity(len);
        $(buf.push_str($s.as_ref());)+
        buf
    }};
}

impl <'arena>PointerKey<'arena> {
    pub fn from_pointer(pointer: &'arena str, value_type: ValueType, depth: u8, position: usize) -> Self {
        Self {
            pointer,
            value_type,
            depth,
            position,
            index: 0,
        }
    }
}

#[derive(Eq, Hash, PartialEq, Debug, Clone, Copy)]
#[derive(Default)]
pub enum ValueType {
    Array(usize),
    Object,
    Number,
    String,
    Bool,
    Null,
    #[default]
    None,
}


type PointerFragment = Vec<String>;

pub type FlatJsonValue<'arena> = Vec<(PointerKey<'arena>, Option<String>)>;


#[derive(Clone)]
pub struct ParseResult<'arena> {
    pub json: FlatJsonValue<'arena>,
    pub max_json_depth: usize,
    pub parsing_max_depth: u8,
    pub started_parsing_at: Option<String>,
    pub parsing_prefix: Option<String>,
}

impl <'arena>ParseResult<'arena> {
    pub fn clone_except_json(&self) -> Self {
        Self {
            json: Default::default(),
            max_json_depth: self.max_json_depth,
            parsing_max_depth: self.parsing_max_depth,
            started_parsing_at: self.started_parsing_at.clone(),
            parsing_prefix: self.parsing_prefix.clone(),
        }
    }
}

#[macro_export]
macro_rules! concat_string {
    () => { String::with_capacity(0) };
    ($($s:expr),+) => {{
        use std::ops::AddAssign;
        let mut len = 0;
        $(len.add_assign(AsRef::<str>::as_ref(&$s).len());)+
        let mut buf = String::with_capacity(len);
        $(buf.push_str($s.as_ref());)+
        buf
    }};
}


impl<'a> JSONParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lexer = Lexer::new(input.as_bytes());
        let parser = Parser::new(lexer);

        Self { parser }
    }
    pub fn parse<'arena>(&mut self, options: ParseOptions, arena: &'arena Arena) -> Result<ParseResult<'arena>, String> {
        self.parser.parse(&options, 1, arena)
    }

    pub fn change_depth<'arena>(previous_parse_result: ParseResult<'arena>, mut parse_options: ParseOptions, arena: &'arena Arena) -> Result<ParseResult<'arena>, String> {
        if previous_parse_result.parsing_max_depth < parse_options.max_depth {
            let previous_len = previous_parse_result.json.len();
            let mut new_flat_json_structure = FlatJsonValue::with_capacity(previous_len + (parse_options.max_depth - previous_parse_result.parsing_max_depth) as usize * (previous_len / 3));
            for (k, v) in previous_parse_result.json {
                if !matches!(k.value_type, ValueType::Object) {
                    new_flat_json_structure.push((k, v));
                } else {
                    if k.depth == previous_parse_result.parsing_max_depth as u8 {
                        if let Some(mut v) = v {
                            new_flat_json_structure.push((k.clone(), Some(v.clone())));
                            let lexer = Lexer::new(v.as_bytes());
                            let mut parser = Parser::new(lexer);
                            parse_options.prefix = Some(k.pointer.to_string());
                            let res = parser.parse(&parse_options, k.depth + 1, &arena)?;
                            new_flat_json_structure.extend(res.json);
                        }
                    } else {
                        new_flat_json_structure.push((k, v));
                    }

                }
            }
            Ok(ParseResult {
                json: new_flat_json_structure,
                max_json_depth: previous_parse_result.max_json_depth,
                parsing_max_depth: parse_options.max_depth,
                started_parsing_at: previous_parse_result.started_parsing_at,
                parsing_prefix: previous_parse_result.parsing_prefix,
            })
        } else {
            Ok(previous_parse_result)
        }
    }


    pub fn filter_non_null_column<'arena>(previous_parse_result: &Vec<JsonArrayEntries<'arena>>, prefix: &str, non_null_columns: &Vec<String>) -> Vec<JsonArrayEntries<'arena>> {
        let mut res: Vec<JsonArrayEntries> = Vec::with_capacity(previous_parse_result.len());
        for row in previous_parse_result {
            let mut should_add_row = true;
            for pointer in non_null_columns {
                let pointer_to_find = concat_string!(prefix, "/", row.index().to_string(), pointer);
                if let Some((_, value)) = row.find_node_at(&pointer_to_find) {
                    if value.is_none() {
                        should_add_row = false;
                        break;
                    }
                } else {
                    should_add_row = false;
                    break;
                }
            }

            if should_add_row {
                res.push(row.clone());
            }
        }
        res
    }
}


#[inline]
pub fn string_from_bytes(bytes: &[u8]) -> Option<&str> {
    #[cfg(feature = "simdutf8")]{
        simdutf8::basic::from_utf8(bytes).ok()
    }
    #[cfg(not(feature = "simdutf8"))]{
        std::str::from_utf8(bytes).ok()
    }
}