//! SigilScript built-in library.
//!
//! Drop 5b-2: adds scanning, pointers, modules, regions, and makes
//! `freeze()` / `unfreeze()` persistent across the Freeze tab.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ast::TypeKind;
use super::interpreter::{BuiltinFn, RuntimeError};
use super::value::{self, Value};
use crate::core::memory::{self, FrozenEntry, MemoryReader, ValueType};

type RResult<T> = Result<T, RuntimeError>;

pub struct BuiltinContext {
    pub reader: Arc<Mutex<Option<MemoryReader>>>,
    pub log: Arc<Mutex<Vec<String>>>,
    pub stop: Arc<AtomicBool>,
    pub start_time: Instant,
    pub frozen: Arc<Mutex<Vec<FrozenEntry>>>,
    pub freeze_active: Arc<AtomicBool>,
}

// ============================================================
// PUBLIC ENTRY POINT
// ============================================================

pub fn make_builtins(ctx: BuiltinContext) -> HashMap<String, BuiltinFn> {
    let mut b: HashMap<String, BuiltinFn> = HashMap::new();

    install_logging(&mut b, ctx.log.clone());
    install_math(&mut b);
    install_strings(&mut b);
    install_memory(&mut b, ctx.reader.clone());
    install_freeze(
        &mut b,
        ctx.reader.clone(),
        ctx.frozen.clone(),
        ctx.freeze_active.clone(),
    );
    install_scanning(&mut b, ctx.reader.clone());
    install_pointers(&mut b, ctx.reader.clone());
    install_modules_and_regions(&mut b, ctx.reader.clone());
    install_utility(&mut b, ctx.start_time, ctx.stop.clone());

    b
}

// ============================================================
// HELPERS
// ============================================================

fn err(msg: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: msg.into(),
        line: 0,
        col: 0,
    }
}

fn expect_args(name: &str, args: &[Value], n: usize) -> RResult<()> {
    if args.len() != n {
        return Err(err(format!(
            "{} expects {} argument{}, got {}",
            name,
            n,
            if n == 1 { "" } else { "s" },
            args.len()
        )));
    }
    Ok(())
}

fn expect_min_args(name: &str, args: &[Value], n: usize) -> RResult<()> {
    if args.len() < n {
        return Err(err(format!(
            "{} expects at least {} argument{}, got {}",
            name,
            n,
            if n == 1 { "" } else { "s" },
            args.len()
        )));
    }
    Ok(())
}

fn as_int(v: &Value, name: &str) -> RResult<i64> {
    v.as_int()
        .ok_or_else(|| err(format!("{}: expected int, got {}", name, v.type_name())))
}

fn as_float(v: &Value, name: &str) -> RResult<f64> {
    v.as_float()
        .ok_or_else(|| err(format!("{}: expected float, got {}", name, v.type_name())))
}

fn as_u64(v: &Value, name: &str) -> RResult<u64> {
    v.as_ptr()
        .ok_or_else(|| err(format!("{}: expected pointer or int, got {}", name, v.type_name())))
}

fn as_str<'a>(v: &'a Value, name: &str) -> RResult<&'a str> {
    v.as_str()
        .ok_or_else(|| err(format!("{}: expected string, got {}", name, v.type_name())))
}

fn with_reader<R>(
    reader: &Arc<Mutex<Option<MemoryReader>>>,
    f: impl FnOnce(&MemoryReader) -> RResult<R>,
) -> RResult<R> {
    let guard = reader.lock().map_err(|_| err("reader mutex poisoned"))?;
    let r = guard.as_ref().ok_or_else(|| err("not attached to a process"))?;
    f(r)
}

// ============================================================
// LOGGING
// ============================================================

fn install_logging(b: &mut HashMap<String, BuiltinFn>, log: Arc<Mutex<Vec<String>>>) {
    fn push(l: &Arc<Mutex<Vec<String>>>, line: String) {
        if let Ok(mut g) = l.lock() {
            g.push(line);
            if g.len() > 5000 {
                let n = g.len() - 5000;
                g.drain(0..n);
            }
        }
    }

    {
        let log = log.clone();
        b.insert(
            "log".into(),
            Arc::new(move |args: &[Value]| {
                let line = args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ");
                push(&log, line);
                Ok(Value::Void)
            }),
        );
    }
    {
        let log = log.clone();
        b.insert(
            "log_hex".into(),
            Arc::new(move |args: &[Value]| {
                expect_args("log_hex", args, 1)?;
                let n = as_int(&args[0], "log_hex")?;
                push(&log, format!("{:#x}", n));
                Ok(Value::Void)
            }),
        );
    }
    {
        let log = log.clone();
        b.insert(
            "warn".into(),
            Arc::new(move |args: &[Value]| {
                let line = args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ");
                push(&log, format!("[warn] {}", line));
                Ok(Value::Void)
            }),
        );
    }
    {
        let log = log.clone();
        b.insert(
            "error".into(),
            Arc::new(move |args: &[Value]| {
                let line = args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ");
                push(&log, format!("[error] {}", line));
                Ok(Value::Void)
            }),
        );
    }
    {
        let log = log.clone();
        b.insert(
            "debug".into(),
            Arc::new(move |args: &[Value]| {
                expect_args("debug", args, 1)?;
                push(&log, format!("[debug] {} = {}", args[0].type_name(), args[0]));
                Ok(Value::Void)
            }),
        );
    }
}

// ============================================================
// MATH
// ============================================================

fn install_math(b: &mut HashMap<String, BuiltinFn>) {
    macro_rules! unary_float {
        ($name:expr, $fn:expr) => {{
            b.insert(
                $name.into(),
                Arc::new(move |args: &[Value]| {
                    expect_args($name, args, 1)?;
                    let x = as_float(&args[0], $name)?;
                    Ok(Value::Float($fn(x)))
                }),
            );
        }};
    }

    unary_float!("sqrt", |x: f64| x.sqrt());
    unary_float!("sin", |x: f64| x.sin());
    unary_float!("cos", |x: f64| x.cos());
    unary_float!("tan", |x: f64| x.tan());
    unary_float!("floor", |x: f64| x.floor());
    unary_float!("ceil", |x: f64| x.ceil());
    unary_float!("round", |x: f64| x.round());
    unary_float!("ln", |x: f64| x.ln());
    unary_float!("log10", |x: f64| x.log10());

    b.insert("abs".into(), Arc::new(|args: &[Value]| {
        expect_args("abs", args, 1)?;
        Ok(match &args[0] {
            Value::Int(n) => Value::Int(n.abs()),
            Value::Float(f) => Value::Float(f.abs()),
            other => return Err(err(format!("abs: expected number, got {}", other.type_name()))),
        })
    }));

    b.insert("sign".into(), Arc::new(|args: &[Value]| {
        expect_args("sign", args, 1)?;
        Ok(match &args[0] {
            Value::Int(n) => Value::Int(n.signum()),
            Value::Float(f) => Value::Float(f.signum()),
            other => return Err(err(format!("sign: expected number, got {}", other.type_name()))),
        })
    }));

    b.insert("min".into(), Arc::new(|args: &[Value]| {
        expect_args("min", args, 2)?;
        let a = as_float(&args[0], "min")?;
        let b = as_float(&args[1], "min")?;
        if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) {
            Ok(Value::Int(a.min(b) as i64))
        } else {
            Ok(Value::Float(a.min(b)))
        }
    }));

    b.insert("max".into(), Arc::new(|args: &[Value]| {
        expect_args("max", args, 2)?;
        let a = as_float(&args[0], "max")?;
        let b = as_float(&args[1], "max")?;
        if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) {
            Ok(Value::Int(a.max(b) as i64))
        } else {
            Ok(Value::Float(a.max(b)))
        }
    }));

    b.insert("clamp".into(), Arc::new(|args: &[Value]| {
        expect_args("clamp", args, 3)?;
        let x = as_float(&args[0], "clamp")?;
        let lo = as_float(&args[1], "clamp")?;
        let hi = as_float(&args[2], "clamp")?;
        if matches!(&args[0], Value::Int(_)) {
            Ok(Value::Int(x.clamp(lo, hi) as i64))
        } else {
            Ok(Value::Float(x.clamp(lo, hi)))
        }
    }));

    b.insert("pow".into(), Arc::new(|args: &[Value]| {
        expect_args("pow", args, 2)?;
        Ok(Value::Float(as_float(&args[0], "pow")?.powf(as_float(&args[1], "pow")?)))
    }));
}

// ============================================================
// STRINGS
// ============================================================

fn install_strings(b: &mut HashMap<String, BuiltinFn>) {
    b.insert("len".into(), Arc::new(|args: &[Value]| {
        expect_args("len", args, 1)?;
        Ok(match &args[0] {
            Value::Str(s) => Value::Int(s.len() as i64),
            Value::Bytes(x) => Value::Int(x.len() as i64),
            Value::Vec(v) => Value::Int(v.len() as i64),
            other => return Err(err(format!("len: unsupported type {}", other.type_name()))),
        })
    }));

    b.insert("upper".into(), Arc::new(|args: &[Value]| {
        expect_args("upper", args, 1)?;
        Ok(Value::str(as_str(&args[0], "upper")?.to_uppercase()))
    }));
    b.insert("lower".into(), Arc::new(|args: &[Value]| {
        expect_args("lower", args, 1)?;
        Ok(Value::str(as_str(&args[0], "lower")?.to_lowercase()))
    }));
    b.insert("trim".into(), Arc::new(|args: &[Value]| {
        expect_args("trim", args, 1)?;
        Ok(Value::str(as_str(&args[0], "trim")?.trim().to_string()))
    }));
    b.insert("contains".into(), Arc::new(|args: &[Value]| {
        expect_args("contains", args, 2)?;
        Ok(Value::Bool(as_str(&args[0], "contains")?.contains(as_str(&args[1], "contains")?)))
    }));
    b.insert("starts_with".into(), Arc::new(|args: &[Value]| {
        expect_args("starts_with", args, 2)?;
        Ok(Value::Bool(as_str(&args[0], "starts_with")?.starts_with(as_str(&args[1], "starts_with")?)))
    }));
    b.insert("ends_with".into(), Arc::new(|args: &[Value]| {
        expect_args("ends_with", args, 2)?;
        Ok(Value::Bool(as_str(&args[0], "ends_with")?.ends_with(as_str(&args[1], "ends_with")?)))
    }));
    b.insert("split".into(), Arc::new(|args: &[Value]| {
        expect_args("split", args, 2)?;
        let parts: Vec<Value> = as_str(&args[0], "split")?
            .split(as_str(&args[1], "split")?)
            .map(Value::str)
            .collect();
        Ok(Value::vec(parts))
    }));
    b.insert("join".into(), Arc::new(|args: &[Value]| {
        expect_args("join", args, 2)?;
        let vec = match &args[0] {
            Value::Vec(v) => v.clone(),
            other => return Err(err(format!("join: expected vec, got {}", other.type_name()))),
        };
        let parts: Vec<String> = vec.iter().map(|v| format!("{}", v)).collect();
        Ok(Value::str(parts.join(as_str(&args[1], "join")?)))
    }));
    b.insert("replace".into(), Arc::new(|args: &[Value]| {
        expect_args("replace", args, 3)?;
        Ok(Value::str(
            as_str(&args[0], "replace")?
                .replace(as_str(&args[1], "replace")?, as_str(&args[2], "replace")?),
        ))
    }));
    b.insert("hex".into(), Arc::new(|args: &[Value]| {
        expect_args("hex", args, 1)?;
        Ok(match &args[0] {
            Value::Bytes(b) => Value::str(b.iter().map(|x| format!("{:02x}", x)).collect::<String>()),
            other => Value::str(format!("{:#x}", as_int(other, "hex")?)),
        })
    }));
    b.insert("unhex".into(), Arc::new(|args: &[Value]| {
        expect_args("unhex", args, 1)?;
        let s = as_str(&args[0], "unhex")?;
        let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.len() % 2 != 0 {
            return Err(err("unhex: odd number of hex digits"));
        }
        let mut out = Vec::with_capacity(cleaned.len() / 2);
        let bytes = cleaned.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let hi = (bytes[i] as char).to_digit(16).ok_or_else(|| err("unhex: invalid hex"))?;
            let lo = (bytes[i + 1] as char).to_digit(16).ok_or_else(|| err("unhex: invalid hex"))?;
            out.push(((hi << 4) | lo) as u8);
            i += 2;
        }
        Ok(Value::bytes(out))
    }));
    b.insert("to_string".into(), Arc::new(|args: &[Value]| {
        expect_args("to_string", args, 1)?;
        Ok(Value::str(format!("{}", args[0])))
    }));
    b.insert("to_int".into(), Arc::new(|args: &[Value]| {
        expect_args("to_int", args, 1)?;
        Ok(match &args[0] {
            Value::Str(s) => {
                let t = s.trim();
                let n = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    i64::from_str_radix(hex, 16)
                        .map_err(|_| err(format!("to_int: '{}' is not a hex int", t)))?
                } else {
                    t.parse::<i64>().map_err(|_| err(format!("to_int: '{}' is not an int", t)))?
                };
                Value::Int(n)
            }
            other => Value::Int(as_int(other, "to_int")?),
        })
    }));
    b.insert("to_float".into(), Arc::new(|args: &[Value]| {
        expect_args("to_float", args, 1)?;
        let v = match &args[0] {
            Value::Str(s) => s.trim().parse::<f64>().map_err(|_| err("to_float: invalid"))?,
            other => as_float(other, "to_float")?,
        };
        Ok(Value::Float(v))
    }));
    b.insert("to_bytes".into(), Arc::new(|args: &[Value]| {
        expect_args("to_bytes", args, 1)?;
        Ok(Value::bytes(as_str(&args[0], "to_bytes")?.as_bytes().to_vec()))
    }));
}

// ============================================================
// MEMORY I/O
// ============================================================

fn install_memory(b: &mut HashMap<String, BuiltinFn>, reader: Arc<Mutex<Option<MemoryReader>>>) {
    {
        let reader = reader.clone();
        b.insert("read".into(), Arc::new(move |args: &[Value]| {
            expect_args("read", args, 1)?;
            let addr = as_u64(&args[0], "read")?;
            let bytes = with_reader(&reader, |r| r.read(addr, 4).map_err(err))?;
            value::bytes_to_value(&bytes, &TypeKind::Int32).map_err(|e| err(format!("read: {}", e)))
        }));
    }

    macro_rules! typed_read {
        ($name:expr, $ty:expr, $size:expr) => {{
            let reader = reader.clone();
            b.insert($name.into(), Arc::new(move |args: &[Value]| {
                expect_args($name, args, 1)?;
                let addr = as_u64(&args[0], $name)?;
                let bytes = with_reader(&reader, |r| r.read(addr, $size).map_err(err))?;
                value::bytes_to_value(&bytes, &$ty).map_err(|e| err(format!("{}: {}", $name, e)))
            }));
        }};
    }
    typed_read!("read_int8", TypeKind::Int8, 1);
    typed_read!("read_int16", TypeKind::Int16, 2);
    typed_read!("read_int32", TypeKind::Int32, 4);
    typed_read!("read_int64", TypeKind::Int64, 8);
    typed_read!("read_uint8", TypeKind::Uint8, 1);
    typed_read!("read_uint16", TypeKind::Uint16, 2);
    typed_read!("read_uint32", TypeKind::Uint32, 4);
    typed_read!("read_uint64", TypeKind::Uint64, 8);
    typed_read!("read_float32", TypeKind::Float32, 4);
    typed_read!("read_float64", TypeKind::Float64, 8);
    typed_read!("read_bool", TypeKind::Bool, 1);
    typed_read!("read_ptr", TypeKind::Ptr, 8);

    {
        let reader = reader.clone();
        b.insert("read_string".into(), Arc::new(move |args: &[Value]| {
            expect_args("read_string", args, 2)?;
            let addr = as_u64(&args[0], "read_string")?;
            let max = as_int(&args[1], "read_string")?.max(1) as usize;
            let bytes = with_reader(&reader, |r| r.read(addr, max).map_err(err))?;
            let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Ok(Value::str(String::from_utf8_lossy(&bytes[..nul]).to_string()))
        }));
    }
    {
        let reader = reader.clone();
        b.insert("read_wstring".into(), Arc::new(move |args: &[Value]| {
            expect_args("read_wstring", args, 2)?;
            let addr = as_u64(&args[0], "read_wstring")?;
            let max = as_int(&args[1], "read_wstring")?.max(1) as usize;
            let bytes = with_reader(&reader, |r| r.read(addr, max * 2).map_err(err))?;
            let mut u16s = Vec::new();
            for chunk in bytes.chunks_exact(2) {
                let u = u16::from_le_bytes([chunk[0], chunk[1]]);
                if u == 0 { break; }
                u16s.push(u);
            }
            Ok(Value::str(String::from_utf16_lossy(&u16s)))
        }));
    }
    {
        let reader = reader.clone();
        b.insert("read_bytes".into(), Arc::new(move |args: &[Value]| {
            expect_args("read_bytes", args, 2)?;
            let addr = as_u64(&args[0], "read_bytes")?;
            let len = as_int(&args[1], "read_bytes")?.max(0) as usize;
            let bytes = with_reader(&reader, |r| r.read(addr, len).map_err(err))?;
            Ok(Value::bytes(bytes))
        }));
    }
    {
        let reader = reader.clone();
        b.insert("read_mem".into(), Arc::new(move |args: &[Value]| {
            expect_args("read_mem", args, 2)?;
            let addr = as_u64(&args[0], "read_mem")?;
            let len = as_int(&args[1], "read_mem")?.max(0) as usize;
            let bytes = with_reader(&reader, |r| r.read(addr, len).map_err(err))?;
            Ok(Value::bytes(bytes))
        }));
    }

    {
        let reader = reader.clone();
        b.insert("write".into(), Arc::new(move |args: &[Value]| {
            expect_args("write", args, 2)?;
            let addr = as_u64(&args[0], "write")?;
            let ty = match &args[1] {
                Value::Int(_) => TypeKind::Int32,
                Value::Float(_) => TypeKind::Float64,
                Value::Bool(_) => TypeKind::Bool,
                Value::Bytes(_) => TypeKind::Bytes,
                Value::Ptr(_) => TypeKind::Ptr,
                other => return Err(err(format!("write: unsupported type {}", other.type_name()))),
            };
            let bytes = value::value_to_bytes(&args[1], &ty)
                .map_err(|e| err(format!("write: {}", e)))?;
            with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;
            Ok(Value::Void)
        }));
    }

    macro_rules! typed_write {
        ($name:expr, $ty:expr) => {{
            let reader = reader.clone();
            b.insert($name.into(), Arc::new(move |args: &[Value]| {
                expect_args($name, args, 2)?;
                let addr = as_u64(&args[0], $name)?;
                let bytes = value::value_to_bytes(&args[1], &$ty)
                    .map_err(|e| err(format!("{}: {}", $name, e)))?;
                with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;
                Ok(Value::Void)
            }));
        }};
    }
    typed_write!("write_int8", TypeKind::Int8);
    typed_write!("write_int16", TypeKind::Int16);
    typed_write!("write_int32", TypeKind::Int32);
    typed_write!("write_int64", TypeKind::Int64);
    typed_write!("write_uint8", TypeKind::Uint8);
    typed_write!("write_uint16", TypeKind::Uint16);
    typed_write!("write_uint32", TypeKind::Uint32);
    typed_write!("write_uint64", TypeKind::Uint64);
    typed_write!("write_float32", TypeKind::Float32);
    typed_write!("write_float64", TypeKind::Float64);
    typed_write!("write_bool", TypeKind::Bool);
    typed_write!("write_ptr", TypeKind::Ptr);

    {
        let reader = reader.clone();
        b.insert("write_string".into(), Arc::new(move |args: &[Value]| {
            expect_args("write_string", args, 2)?;
            let addr = as_u64(&args[0], "write_string")?;
            let mut bytes = as_str(&args[1], "write_string")?.as_bytes().to_vec();
            bytes.push(0);
            with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;
            Ok(Value::Void)
        }));
    }
    {
        let reader = reader.clone();
        b.insert("write_bytes".into(), Arc::new(move |args: &[Value]| {
            expect_args("write_bytes", args, 2)?;
            let addr = as_u64(&args[0], "write_bytes")?;
            let bytes = match &args[1] {
                Value::Bytes(b) => b.to_vec(),
                Value::Vec(v) => v.iter().map(|x| as_int(x, "write_bytes").map(|n| n as u8)).collect::<RResult<Vec<u8>>>()?,
                other => return Err(err(format!("write_bytes: expected bytes/vec, got {}", other.type_name()))),
            };
            with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;
            Ok(Value::Void)
        }));
    }
    {
        let reader = reader.clone();
        b.insert("write_mem".into(), Arc::new(move |args: &[Value]| {
            expect_args("write_mem", args, 2)?;
            let addr = as_u64(&args[0], "write_mem")?;
            let hexstr = as_str(&args[1], "write_mem")?;
            let cleaned: String = hexstr.chars().filter(|c| !c.is_whitespace()).collect();
            if cleaned.len() % 2 != 0 {
                return Err(err("write_mem: odd number of hex digits"));
            }
            let mut bytes = Vec::with_capacity(cleaned.len() / 2);
            let bs = cleaned.as_bytes();
            let mut i = 0;
            while i + 1 < bs.len() {
                let hi = (bs[i] as char).to_digit(16).ok_or_else(|| err("write_mem: invalid hex"))?;
                let lo = (bs[i + 1] as char).to_digit(16).ok_or_else(|| err("write_mem: invalid hex"))?;
                bytes.push(((hi << 4) | lo) as u8);
                i += 2;
            }
            with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;
            Ok(Value::Void)
        }));
    }
    {
        let reader = reader.clone();
        b.insert("memcpy".into(), Arc::new(move |args: &[Value]| {
            expect_args("memcpy", args, 3)?;
            let dst = as_u64(&args[0], "memcpy")?;
            let src = as_u64(&args[1], "memcpy")?;
            let len = as_int(&args[2], "memcpy")?.max(0) as usize;
            with_reader(&reader, |r| {
                let data = r.read(src, len).map_err(err)?;
                r.write(dst, &data).map_err(err)
            })?;
            Ok(Value::Void)
        }));
    }
    {
        let reader = reader.clone();
        b.insert("memset".into(), Arc::new(move |args: &[Value]| {
            expect_args("memset", args, 3)?;
            let addr = as_u64(&args[0], "memset")?;
            let byte = as_int(&args[1], "memset")? as u8;
            let len = as_int(&args[2], "memset")?.max(0) as usize;
            let data = vec![byte; len];
            with_reader(&reader, |r| r.write(addr, &data).map_err(err))?;
            Ok(Value::Void)
        }));
    }
}

// ============================================================
// FREEZE (persistent)
// ============================================================

fn install_freeze(
    b: &mut HashMap<String, BuiltinFn>,
    reader: Arc<Mutex<Option<MemoryReader>>>,
    frozen: Arc<Mutex<Vec<FrozenEntry>>>,
    freeze_active: Arc<AtomicBool>,
) {
    // freeze(addr, value) — pins addr to value; writes once and adds
    // to the persistent freeze list.
    {
        let reader = reader.clone();
        let frozen = frozen.clone();
        let active = freeze_active.clone();
        b.insert("freeze".into(), Arc::new(move |args: &[Value]| {
            expect_args("freeze", args, 2)?;
            let addr = as_u64(&args[0], "freeze")?;
            let ty = match &args[1] {
                Value::Int(_) => TypeKind::Int32,
                Value::Float(_) => TypeKind::Float64,
                Value::Bool(_) => TypeKind::Bool,
                Value::Bytes(_) => TypeKind::Bytes,
                _ => TypeKind::Int32,
            };
            let bytes = value::value_to_bytes(&args[1], &ty)
                .map_err(|e| err(format!("freeze: {}", e)))?;

            // Write once immediately.
            with_reader(&reader, |r| r.write(addr, &bytes).map_err(err))?;

            // Register in the persistent list. Value type used for
            // display; the raw bytes are what get re-written.
            let vt = match ty {
                TypeKind::Int8 => ValueType::Int32,
                TypeKind::Int16 => ValueType::Int32,
                TypeKind::Int32 => ValueType::Int32,
                TypeKind::Int64 => ValueType::Int64,
                TypeKind::Uint8 | TypeKind::Uint16 | TypeKind::Uint32 => ValueType::Int32,
                TypeKind::Uint64 => ValueType::Int64,
                TypeKind::Float32 => ValueType::Float,
                TypeKind::Float64 => ValueType::Double,
                TypeKind::Bool => ValueType::Int32,
                TypeKind::Bytes => ValueType::Bytes,
                TypeKind::Ptr => ValueType::Int64,
                _ => ValueType::Int32,
            };

            if let Ok(mut l) = frozen.lock() {
                l.retain(|e| e.address != addr);
                l.push(FrozenEntry {
                    address: addr,
                    value_type: vt,
                    bytes: bytes.clone(),
                });
            }
            active.store(true, Ordering::Relaxed);
            Ok(Value::Void)
        }));
    }

    // unfreeze(addr)
    {
        let frozen = frozen.clone();
        b.insert("unfreeze".into(), Arc::new(move |args: &[Value]| {
            expect_args("unfreeze", args, 1)?;
            let addr = as_u64(&args[0], "unfreeze")?;
            if let Ok(mut l) = frozen.lock() {
                l.retain(|e| e.address != addr);
            }
            Ok(Value::Void)
        }));
    }

    // is_frozen(addr)
    {
        let frozen = frozen.clone();
        b.insert("is_frozen".into(), Arc::new(move |args: &[Value]| {
            expect_args("is_frozen", args, 1)?;
            let addr = as_u64(&args[0], "is_frozen")?;
            let found = frozen.lock().map(|l| l.iter().any(|e| e.address == addr)).unwrap_or(false);
            Ok(Value::Bool(found))
        }));
    }

    // freeze_list() → vec of addresses
    {
        let frozen = frozen.clone();
        b.insert("freeze_list".into(), Arc::new(move |_args: &[Value]| {
            let list = frozen.lock().map(|l| l.clone()).unwrap_or_default();
            let addrs: Vec<Value> = list.iter().map(|e| Value::Ptr(e.address)).collect();
            Ok(Value::vec(addrs))
        }));
    }
}

// ============================================================
// SCANNING
// ============================================================

fn install_scanning(b: &mut HashMap<String, BuiltinFn>, reader: Arc<Mutex<Option<MemoryReader>>>) {
    // scan(pattern_bytes, max_hits=10000) → vec of addresses
    {
        let reader = reader.clone();
        b.insert("scan".into(), Arc::new(move |args: &[Value]| {
            expect_min_args("scan", args, 1)?;
            let pattern = bytes_arg(&args[0], "scan")?;
            let max = if args.len() >= 2 {
                as_int(&args[1], "scan")?.max(1) as usize
            } else {
                10_000
            };
            let hits = with_reader(&reader, |r| {
                Ok(memory::scan_all_regions(r, &pattern, max))
            })?;
            let addrs: Vec<Value> = hits.iter().map(|h| Value::Ptr(h.address)).collect();
            Ok(Value::vec(addrs))
        }));
    }

    // scan_first(pattern) → ptr or null
    {
        let reader = reader.clone();
        b.insert("scan_first".into(), Arc::new(move |args: &[Value]| {
            expect_args("scan_first", args, 1)?;
            let pattern = bytes_arg(&args[0], "scan_first")?;
            let hits = with_reader(&reader, |r| {
                Ok(memory::scan_all_regions(r, &pattern, 1))
            })?;
            Ok(match hits.first() {
                Some(h) => Value::Ptr(h.address),
                None => Value::Null,
            })
        }));
    }

    // find_string(s) → first hit address (ASCII)
    {
        let reader = reader.clone();
        b.insert("find_string".into(), Arc::new(move |args: &[Value]| {
            expect_args("find_string", args, 1)?;
            let s = as_str(&args[0], "find_string")?;
            let bytes = s.as_bytes().to_vec();
            let hits = with_reader(&reader, |r| {
                Ok(memory::scan_all_regions(r, &bytes, 1))
            })?;
            Ok(match hits.first() {
                Some(h) => Value::Ptr(h.address),
                None => Value::Null,
            })
        }));
    }

    // find_cstring(s) — same as find_string for v1
    {
        let reader = reader.clone();
        b.insert("find_cstring".into(), Arc::new(move |args: &[Value]| {
            expect_args("find_cstring", args, 1)?;
            let s = as_str(&args[0], "find_cstring")?;
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            let hits = with_reader(&reader, |r| {
                Ok(memory::scan_all_regions(r, &bytes, 1))
            })?;
            Ok(match hits.first() {
                Some(h) => Value::Ptr(h.address),
                None => Value::Null,
            })
        }));
    }

    // scan_module("game.exe", pattern) — stub for now; returns empty vec
    {
        let reader = reader.clone();
        b.insert("scan_module".into(), Arc::new(move |args: &[Value]| {
            expect_args("scan_module", args, 2)?;
            let _name = as_str(&args[0], "scan_module")?;
            let _pattern = bytes_arg(&args[1], "scan_module")?;
            // Without module enumeration, fall back to whole-process scan.
            // Real per-module limiting lands in a later drop.
            let pattern = bytes_arg(&args[1], "scan_module")?;
            let hits = with_reader(&reader, |r| {
                Ok(memory::scan_all_regions(r, &pattern, 10_000))
            })?;
            let addrs: Vec<Value> = hits.iter().map(|h| Value::Ptr(h.address)).collect();
            Ok(Value::vec(addrs))
        }));
    }

    // scan_range(start, end, pattern)
    {
        let reader = reader.clone();
        b.insert("scan_range".into(), Arc::new(move |args: &[Value]| {
            expect_args("scan_range", args, 3)?;
            let start = as_u64(&args[0], "scan_range")?;
            let end = as_u64(&args[1], "scan_range")?;
            let pattern = bytes_arg(&args[2], "scan_range")?;
            if end <= start || pattern.is_empty() {
                return Ok(Value::vec(Vec::new()));
            }
            let len = (end - start).min(16 * 1024 * 1024) as usize;
            let data = with_reader(&reader, |r| r.read(start, len).map_err(err))?;
            let mut out = Vec::new();
            if data.len() >= pattern.len() {
                let mut i = 0;
                while i + pattern.len() <= data.len() {
                    if &data[i..i + pattern.len()] == pattern.as_slice() {
                        out.push(Value::Ptr(start + i as u64));
                        if out.len() >= 10_000 {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            Ok(Value::vec(out))
        }));
    }
}

/// Coerce an argument to a byte pattern. Accepts `bytes` or a hex string.
fn bytes_arg(v: &Value, name: &str) -> RResult<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.to_vec()),
        Value::Str(s) => {
            let cleaned: String = s.chars().filter(|c| !c.is_whitespace() && *c != '?').collect();
            if cleaned.len() % 2 != 0 {
                return Err(err(format!("{}: odd number of hex digits", name)));
            }
            let mut out = Vec::with_capacity(cleaned.len() / 2);
            let bs = cleaned.as_bytes();
            let mut i = 0;
            while i + 1 < bs.len() {
                let hi = (bs[i] as char).to_digit(16)
                    .ok_or_else(|| err(format!("{}: invalid hex", name)))?;
                let lo = (bs[i + 1] as char).to_digit(16)
                    .ok_or_else(|| err(format!("{}: invalid hex", name)))?;
                out.push(((hi << 4) | lo) as u8);
                i += 2;
            }
            Ok(out)
        }
        other => Err(err(format!("{}: expected bytes or hex string, got {}", name, other.type_name()))),
    }
}

// ============================================================
// POINTERS
// ============================================================

fn install_pointers(b: &mut HashMap<String, BuiltinFn>, reader: Arc<Mutex<Option<MemoryReader>>>) {
    // deref(addr) → read 8 bytes as a pointer
    {
        let reader = reader.clone();
        b.insert("deref".into(), Arc::new(move |args: &[Value]| {
            expect_args("deref", args, 1)?;
            let addr = as_u64(&args[0], "deref")?;
            let bytes = with_reader(&reader, |r| r.read(addr, 8).map_err(err))?;
            if bytes.len() < 8 {
                return Err(err("deref: short read"));
            }
            let p = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(Value::Ptr(p))
        }));
    }

    // offset(addr, n) → addr + n
    b.insert("offset".into(), Arc::new(|args: &[Value]| {
        expect_args("offset", args, 2)?;
        let addr = as_u64(&args[0], "offset")?;
        let n = as_int(&args[1], "offset")? as i64;
        Ok(Value::Ptr((addr as i64).wrapping_add(n) as u64))
    }));

    // chain(base, [off1, off2, ...]) → walk the chain
    {
        let reader = reader.clone();
        b.insert("chain".into(), Arc::new(move |args: &[Value]| {
            expect_args("chain", args, 2)?;
            let base = as_u64(&args[0], "chain")?;
            let offsets = match &args[1] {
                Value::Vec(v) => v.clone(),
                other => return Err(err(format!("chain: expected vec, got {}", other.type_name()))),
            };
            let mut cur = base;
            for (i, off) in offsets.iter().enumerate() {
                let o = as_int(off, "chain")? as i64;
                cur = (cur as i64).wrapping_add(o) as u64;
                if i + 1 < offsets.len() {
                    // Read pointer at cur before adding the next offset.
                    let bytes = with_reader(&reader, |r| r.read(cur, 8).map_err(err))?;
                    if bytes.len() < 8 {
                        return Err(err("chain: short read"));
                    }
                    cur = u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                }
            }
            Ok(Value::Ptr(cur))
        }));
    }

    // resolve(base, [off1, off2, ...]) — same as chain
    {
        let reader = reader.clone();
        b.insert("resolve".into(), Arc::new(move |args: &[Value]| {
            expect_args("resolve", args, 2)?;
            let base = as_u64(&args[0], "resolve")?;
            let offsets = match &args[1] {
                Value::Vec(v) => v.clone(),
                other => return Err(err(format!("resolve: expected vec, got {}", other.type_name()))),
            };
            let mut cur = base;
            for (i, off) in offsets.iter().enumerate() {
                let o = as_int(off, "resolve")? as i64;
                cur = (cur as i64).wrapping_add(o) as u64;
                if i + 1 < offsets.len() {
                    let bytes = with_reader(&reader, |r| r.read(cur, 8).map_err(err))?;
                    if bytes.len() < 8 {
                        return Err(err("resolve: short read"));
                    }
                    cur = u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                }
            }
            Ok(Value::Ptr(cur))
        }));
    }
}

// ============================================================
// MODULES AND REGIONS
// ============================================================

fn install_modules_and_regions(
    b: &mut HashMap<String, BuiltinFn>,
    reader: Arc<Mutex<Option<MemoryReader>>>,
) {
    // regions() → vec of addresses (base of each committed region)
    {
        let reader = reader.clone();
        b.insert("regions".into(), Arc::new(move |_args: &[Value]| {
            let list = with_reader(&reader, |r| Ok(r.regions()))?;
            let addrs: Vec<Value> = list.iter().map(|r| Value::Ptr(r.base)).collect();
            Ok(Value::vec(addrs))
        }));
    }

    // region_at(addr) → address of the containing region base, or null
    {
        let reader = reader.clone();
        b.insert("region_at".into(), Arc::new(move |args: &[Value]| {
            expect_args("region_at", args, 1)?;
            let addr = as_u64(&args[0], "region_at")?;
            let r = with_reader(&reader, |r| Ok(r.query_region(addr)))?;
            Ok(match r {
                Some(reg) => Value::Ptr(reg.base),
                None => Value::Null,
            })
        }));
    }

    // modules() → empty vec for now (module enumeration deferred)
    b.insert("modules".into(), Arc::new(|_args: &[Value]| {
        Ok(Value::vec(Vec::new()))
    }));

    // module_base(name) — stub, returns null
    b.insert("module_base".into(), Arc::new(|args: &[Value]| {
        expect_args("module_base", args, 1)?;
        let _ = as_str(&args[0], "module_base")?;
        Ok(Value::Null)
    }));

    // module_size(name) — stub, returns 0
    b.insert("module_size".into(), Arc::new(|args: &[Value]| {
        expect_args("module_size", args, 1)?;
        let _ = as_str(&args[0], "module_size")?;
        Ok(Value::Int(0))
    }));

    // main_module() — stub, returns empty string
    b.insert("main_module".into(), Arc::new(|_args: &[Value]| {
        Ok(Value::str(""))
    }));

    // pid() → target PID, or 0 if not attached
    {
        let reader = reader.clone();
        b.insert("pid".into(), Arc::new(move |_args: &[Value]| {
            let pid = reader.lock().ok()
                .and_then(|g| g.as_ref().map(|r| r.pid as i64))
                .unwrap_or(0);
            Ok(Value::Int(pid))
        }));
    }

    // is_alive() → whether we're still attached (best we can do)
    {
        let reader = reader.clone();
        b.insert("is_alive".into(), Arc::new(move |_args: &[Value]| {
            let alive = reader.lock().map(|g| g.is_some()).unwrap_or(false);
            Ok(Value::Bool(alive))
        }));
    }

    // exe_path() — stub, returns empty string
    b.insert("exe_path".into(), Arc::new(|_args: &[Value]| {
        Ok(Value::str(""))
    }));
}

// ============================================================
// UTILITY
// ============================================================

fn install_utility(b: &mut HashMap<String, BuiltinFn>, start: Instant, stop: Arc<AtomicBool>) {
    {
        let stop = stop.clone();
        b.insert("sleep".into(), Arc::new(move |args: &[Value]| {
            expect_args("sleep", args, 1)?;
            let ms = as_int(&args[0], "sleep")?.max(0) as u64;
            let deadline = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < deadline {
                if stop.load(Ordering::Relaxed) {
                    return Err(err("script stopped"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(Value::Void)
        }));
    }

    {
        b.insert("now".into(), Arc::new(move |_args: &[Value]| {
            Ok(Value::Float(start.elapsed().as_secs_f64()))
        }));
    }

    b.insert("rand".into(), Arc::new(|args: &[Value]| {
        expect_args("rand", args, 1)?;
        let max = as_int(&args[0], "rand")?.max(1) as u64;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        Ok(Value::Int((x % max) as i64))
    }));

    b.insert("type_of".into(), Arc::new(|args: &[Value]| {
        expect_args("type_of", args, 1)?;
        Ok(Value::str(args[0].type_name()))
    }));

    b.insert("sizeof_int32".into(), Arc::new(|_| Ok(Value::Int(4))));
    b.insert("sizeof_int64".into(), Arc::new(|_| Ok(Value::Int(8))));
    b.insert("sizeof_ptr".into(), Arc::new(|_| Ok(Value::Int(8))));

    b.insert("assert".into(), Arc::new(|args: &[Value]| {
        expect_min_args("assert", args, 1)?;
        if args[0].is_truthy() {
            Ok(Value::Void)
        } else {
            let msg = if args.len() >= 2 { format!("{}", args[1]) } else { "assertion failed".into() };
            Err(err(msg))
        }
    }));

    b.insert("panic".into(), Arc::new(|args: &[Value]| {
        let msg = if args.is_empty() { "panic".into() } else { format!("{}", args[0]) };
        Err(err(msg))
    }));
}