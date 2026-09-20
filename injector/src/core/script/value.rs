//! SigilScript runtime values.
//!
//! What the interpreter manipulates while evaluating an expression.
//! This is not the same as the AST — many AST nodes collapse into the
//! same runtime value (e.g. `1`, `1i32`, and `0x1` all become
//! `Value::Int(1)`).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::ast::TypeKind;

// ============================================================
// VALUE
// ============================================================

/// A runtime value. `Clone` is cheap because most variants are small
/// and the ones that aren't use `Arc` for interior sharing.
#[derive(Clone)]
pub enum Value {
    // ---- Scalars ----
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Arc<String>),

    // ---- Byte buffer ----
    Bytes(Arc<Vec<u8>>),

    // ---- Composite ----
    Vec(Arc<Vec<Value>>),
    Map(Arc<HashMap<String, Value>>),
    Set(Arc<Vec<Value>>),
    Tuple(Arc<Vec<Value>>),

    /// A user-defined struct instance.
    Struct(Arc<StructInstance>),

    /// An enum variant with optional payload.
    Enum(Arc<EnumValue>),

    // ---- Option / Result ----
    Some(Arc<Value>),
    None,
    Ok(Arc<Value>),
    Err(Arc<Value>),

    // ---- Functions and closures ----
    Func(Arc<FuncValue>),

    // ---- Pointer, distinct from int for type safety ----
    Ptr(u64),

    // ---- Null / unit ----
    Null,
    Void,
}

#[derive(Debug, Clone)]
pub struct StructInstance {
    pub type_name: String,
    pub fields: Vec<(String, Value)>,
}

impl StructInstance {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    pub fn set(&mut self, name: &str, value: Value) -> bool {
        for (n, v) in &mut self.fields {
            if n == name {
                *v = value;
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub type_name: String,
    pub variant: String,
    /// None for unit variants, Some for tuple/struct variants.
    pub payload: Option<Value>,
}

#[derive(Clone)]
pub struct FuncValue {
    pub name: String,
    pub params: Vec<super::ast::Param>,
    pub body: super::ast::Block,
    /// Captured environment for closures.
    pub captures: HashMap<String, Value>,
}

impl fmt::Debug for FuncValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Func({}/{})", self.name, self.params.len())
    }
}

// ============================================================
// CONSTRUCTORS
// ============================================================

impl Value {
    pub fn str(s: impl Into<String>) -> Self {
        Value::Str(Arc::new(s.into()))
    }

    pub fn bytes(b: Vec<u8>) -> Self {
        Value::Bytes(Arc::new(b))
    }

    pub fn vec(v: Vec<Value>) -> Self {
        Value::Vec(Arc::new(v))
    }

    pub fn tuple(v: Vec<Value>) -> Self {
        Value::Tuple(Arc::new(v))
    }

    pub fn some(v: Value) -> Self {
        Value::Some(Arc::new(v))
    }

    pub fn ok(v: Value) -> Self {
        Value::Ok(Arc::new(v))
    }

    pub fn err(v: Value) -> Self {
        Value::Err(Arc::new(v))
    }
}

// ============================================================
// TYPE QUERIES
// ============================================================

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Vec(_) => "vec",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Tuple(_) => "tuple",
            Value::Struct(_) => "struct",
            Value::Enum(_) => "enum",
            Value::Some(_) => "option::some",
            Value::None => "option::none",
            Value::Ok(_) => "result::ok",
            Value::Err(_) => "result::err",
            Value::Func(_) => "func",
            Value::Ptr(_) => "ptr",
            Value::Null => "null",
            Value::Void => "void",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null | Value::None)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_))
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Null | Value::None | Value::Void => false,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::Vec(v) => !v.is_empty(),
            _ => true,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) => Some(*f as i64),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            Value::Ptr(p) => Some(*p as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_ptr(&self) -> Option<u64> {
        match self {
            Value::Ptr(p) => Some(*p),
            Value::Int(n) => Some(*n as u64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    /// Size in bytes when written to memory.
    pub fn byte_size(&self) -> usize {
        match self {
            Value::Int(_) => 8,
            Value::Float(_) => 8,
            Value::Bool(_) => 1,
            Value::Ptr(_) => 8,
            Value::Str(s) => s.len() + 1,
            Value::Bytes(b) => b.len(),
            _ => 8,
        }
    }
}

// ============================================================
// SERIALIZATION TO BYTES
// ============================================================

/// Error raised when a value can't be coerced to a target type.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub expected: String,
    pub got: String,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}, got {}", self.expected, self.got)
    }
}

impl std::error::Error for TypeError {}

/// Serialize a value to little-endian bytes for a target type.
/// Used by `write::<T>(addr, value)` and by the memory editor.
pub fn value_to_bytes(value: &Value, ty: &TypeKind) -> Result<Vec<u8>, TypeError> {
    let mismatch = |expected: &str, got: &Value| TypeError {
        expected: expected.into(),
        got: got.type_name().into(),
    };

    Ok(match ty {
        TypeKind::Int | TypeKind::Int64 => {
            let n = value.as_int().ok_or_else(|| mismatch("int", value))?;
            n.to_le_bytes().to_vec()
        }
        TypeKind::Int8 => {
            let n = value.as_int().ok_or_else(|| mismatch("int8", value))?;
            vec![n as u8]
        }
        TypeKind::Int16 => {
            let n = value.as_int().ok_or_else(|| mismatch("int16", value))?;
            (n as i16).to_le_bytes().to_vec()
        }
        TypeKind::Int32 => {
            let n = value.as_int().ok_or_else(|| mismatch("int32", value))?;
            (n as i32).to_le_bytes().to_vec()
        }
        TypeKind::Uint | TypeKind::Uint64 => {
            let n = value.as_int().ok_or_else(|| mismatch("uint64", value))? as u64;
            n.to_le_bytes().to_vec()
        }
        TypeKind::Uint8 => {
            let n = value.as_int().ok_or_else(|| mismatch("uint8", value))? as u8;
            vec![n]
        }
        TypeKind::Uint16 => {
            let n = value.as_int().ok_or_else(|| mismatch("uint16", value))? as u16;
            n.to_le_bytes().to_vec()
        }
        TypeKind::Uint32 => {
            let n = value.as_int().ok_or_else(|| mismatch("uint32", value))? as u32;
            n.to_le_bytes().to_vec()
        }
        TypeKind::Float | TypeKind::Float64 => {
            let f = value.as_float().ok_or_else(|| mismatch("float", value))?;
            f.to_le_bytes().to_vec()
        }
        TypeKind::Float32 => {
            let f = value.as_float().ok_or_else(|| mismatch("float32", value))?;
            (f as f32).to_le_bytes().to_vec()
        }
        TypeKind::Bool => {
            let b = value.is_truthy();
            vec![if b { 1 } else { 0 }]
        }
        TypeKind::Bool8 => {
            let b = value.is_truthy();
            vec![if b { 1 } else { 0 }]
        }
        TypeKind::String | TypeKind::CString => {
            let s = value.as_str().ok_or_else(|| mismatch("string", value))?;
            let mut b = s.as_bytes().to_vec();
            b.push(0);
            b
        }
        TypeKind::WString => {
            let s = value.as_str().ok_or_else(|| mismatch("wstring", value))?;
            let mut out = Vec::with_capacity(s.len() * 2 + 2);
            for u in s.encode_utf16() {
                out.extend_from_slice(&u.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
            out
        }
        TypeKind::Bytes => {
            let b = value.as_bytes().ok_or_else(|| mismatch("bytes", value))?;
            b.to_vec()
        }
        TypeKind::Ptr => {
            let p = value.as_ptr().ok_or_else(|| mismatch("ptr", value))?;
            p.to_le_bytes().to_vec()
        }
        _ => return Err(mismatch("memory-compatible type", value)),
    })
}

/// Deserialize a value from little-endian bytes according to a type.
pub fn bytes_to_value(bytes: &[u8], ty: &TypeKind) -> Result<Value, TypeError> {
    let short = |expected: usize, got: usize| TypeError {
        expected: format!("{} bytes for {}", expected, ty_name(ty)),
        got: format!("{} bytes", got),
    };

    let take = |n: usize| -> Result<&[u8], TypeError> {
        if bytes.len() < n {
            Err(short(n, bytes.len()))
        } else {
            Ok(&bytes[..n])
        }
    };

    Ok(match ty {
        TypeKind::Int | TypeKind::Int64 => {
            let b = take(8)?;
            Value::Int(i64::from_le_bytes(b.try_into().unwrap()))
        }
        TypeKind::Int8 => {
            let b = take(1)?;
            Value::Int(b[0] as i8 as i64)
        }
        TypeKind::Int16 => {
            let b = take(2)?;
            Value::Int(i16::from_le_bytes(b.try_into().unwrap()) as i64)
        }
        TypeKind::Int32 => {
            let b = take(4)?;
            Value::Int(i32::from_le_bytes(b.try_into().unwrap()) as i64)
        }
        TypeKind::Uint | TypeKind::Uint64 => {
            let b = take(8)?;
            Value::Int(u64::from_le_bytes(b.try_into().unwrap()) as i64)
        }
        TypeKind::Uint8 => {
            let b = take(1)?;
            Value::Int(b[0] as i64)
        }
        TypeKind::Uint16 => {
            let b = take(2)?;
            Value::Int(u16::from_le_bytes(b.try_into().unwrap()) as i64)
        }
        TypeKind::Uint32 => {
            let b = take(4)?;
            Value::Int(u32::from_le_bytes(b.try_into().unwrap()) as i64)
        }
        TypeKind::Float | TypeKind::Float64 => {
            let b = take(8)?;
            Value::Float(f64::from_le_bytes(b.try_into().unwrap()))
        }
        TypeKind::Float32 => {
            let b = take(4)?;
            Value::Float(f32::from_le_bytes(b.try_into().unwrap()) as f64)
        }
        TypeKind::Bool | TypeKind::Bool8 => {
            let b = take(1)?;
            Value::Bool(b[0] != 0)
        }
        TypeKind::String | TypeKind::CString => {
            let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            let s = String::from_utf8_lossy(&bytes[..nul]).to_string();
            Value::str(s)
        }
        TypeKind::WString => {
            let mut u16s = Vec::new();
            for chunk in bytes.chunks_exact(2) {
                let u = u16::from_le_bytes([chunk[0], chunk[1]]);
                if u == 0 {
                    break;
                }
                u16s.push(u);
            }
            Value::str(String::from_utf16_lossy(&u16s))
        }
        TypeKind::Bytes => Value::bytes(bytes.to_vec()),
        TypeKind::Ptr => {
            let b = take(8)?;
            Value::Ptr(u64::from_le_bytes(b.try_into().unwrap()))
        }
        _ => {
            return Err(TypeError {
                expected: "memory-compatible type".into(),
                got: ty_name(ty).into(),
            })
        }
    })
}

fn ty_name(t: &TypeKind) -> &'static str {
    match t {
        TypeKind::Int => "int",
        TypeKind::Int8 => "int8",
        TypeKind::Int16 => "int16",
        TypeKind::Int32 => "int32",
        TypeKind::Int64 => "int64",
        TypeKind::Uint => "uint",
        TypeKind::Uint8 => "uint8",
        TypeKind::Uint16 => "uint16",
        TypeKind::Uint32 => "uint32",
        TypeKind::Uint64 => "uint64",
        TypeKind::Float => "float",
        TypeKind::Float32 => "float32",
        TypeKind::Float64 => "float64",
        TypeKind::Bool => "bool",
        TypeKind::Bool8 => "bool8",
        TypeKind::String => "string",
        TypeKind::CString => "cstring",
        TypeKind::WString => "wstring",
        TypeKind::Bytes => "bytes",
        TypeKind::Ptr => "ptr",
        TypeKind::Void => "void",
        TypeKind::Null => "null",
        TypeKind::Named(_) => "named",
        _ => "composite",
    }
}

// ============================================================
// DISPLAY
// ============================================================

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(x) => {
                if x.fract() == 0.0 {
                    write!(f, "{:.1}", x)
                } else {
                    write!(f, "{}", x)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Str(s) => write!(f, "{}", s),
            Value::Bytes(b) => {
                write!(f, "b[")?;
                for (i, x) in b.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:02x}", x)?;
                }
                write!(f, "]")
            }
            Value::Vec(v) => {
                write!(f, "[")?;
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Set(s) => {
                write!(f, "{{")?;
                for (i, x) in s.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, "}}")
            }
            Value::Tuple(v) => {
                write!(f, "(")?;
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, ")")
            }
            Value::Struct(s) => {
                write!(f, "{} {{", s.type_name)?;
                for (i, (n, v)) in s.fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, " {}: {}", n, v)?;
                }
                write!(f, " }}")
            }
            Value::Enum(e) => match &e.payload {
                Some(p) => write!(f, "{}::{}({})", e.type_name, e.variant, p),
                None => write!(f, "{}::{}", e.type_name, e.variant),
            },
            Value::Some(v) => write!(f, "some({})", v),
            Value::None => write!(f, "none"),
            Value::Ok(v) => write!(f, "ok({})", v),
            Value::Err(v) => write!(f, "err({})", v),
            Value::Func(fv) => write!(f, "<func {}>", fv.name),
            Value::Ptr(p) => write!(f, "{:#018x}", p),
            Value::Null => write!(f, "null"),
            Value::Void => write!(f, "void"),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

// ============================================================
// EQUALITY
// ============================================================

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Ptr(a), Value::Ptr(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::None, Value::None) => true,
            (Value::Vec(a), Value::Vec(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Value {}

// ============================================================
// TESTS
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_roundtrip() {
        let v = Value::Int(12345);
        let bytes = value_to_bytes(&v, &TypeKind::Int32).unwrap();
        assert_eq!(bytes, vec![0x39, 0x30, 0, 0]);
        let back = bytes_to_value(&bytes, &TypeKind::Int32).unwrap();
        assert_eq!(back, Value::Int(12345));
    }

    #[test]
    fn float_roundtrip() {
        let v = Value::Float(3.14);
        let bytes = value_to_bytes(&v, &TypeKind::Float32).unwrap();
        let back = bytes_to_value(&bytes, &TypeKind::Float32).unwrap();
        if let Value::Float(f) = back {
            assert!((f - 3.14).abs() < 0.001);
        } else {
            panic!("expected float");
        }
    }

    #[test]
    fn string_roundtrip() {
        let v = Value::str("hello");
        let bytes = value_to_bytes(&v, &TypeKind::CString).unwrap();
        assert_eq!(bytes, b"hello\0");
        let back = bytes_to_value(&bytes, &TypeKind::CString).unwrap();
        assert_eq!(back.as_str(), Some("hello"));
    }

    #[test]
    fn truthiness() {
        assert!(!Value::Int(0).is_truthy());
        assert!(Value::Int(1).is_truthy());
        assert!(!Value::Null.is_truthy());
        assert!(Value::str("x").is_truthy());
    }
}