//! SigilScript tree-walking interpreter.
//!
//! Core evaluator: handles every AST node type. The built-in library
//! (read/write/freeze/scan/etc.) lives in `builtins.rs` and is
//! dispatched from here.
//!
//! The interpreter owns:
//!   - a scope stack (variable environments)
//!   - a log buffer for `log()` output
//!   - a handle to the target process for memory operations
//!   - a stop flag so the host can interrupt long-running scripts

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::ast::*;
use super::value::{FuncValue, Value};

// ============================================================
// RUNTIME ERRORS
// ============================================================

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for RuntimeError {}

type RResult<T> = Result<T, RuntimeError>;

// ============================================================
// CONTROL FLOW SIGNALS
// ============================================================

/// Returned by statement execution to signal non-local control flow.
enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
    Throw(Value),
}

// ============================================================
// SCOPE
// ============================================================

/// A scope chain. Each block pushes a new scope; lookups walk up.
struct ScopeStack {
    scopes: Vec<HashMap<String, Value>>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn define(&mut self, name: &str, value: Value) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), value);
        }
    }

    fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn set(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}

// ============================================================
// INTERPRETER
// ============================================================

/// Signature of a host-provided built-in function. The interpreter
/// dispatches to the host via this callback, which knows how to talk
/// to the target process.
pub type BuiltinFn = Arc<dyn Fn(&[Value]) -> RResult<Value> + Send + Sync>;

pub struct Interpreter {
    scope: ScopeStack,
    /// Named addresses declared with `addr`.
    named: HashMap<String, (u64, TypeKind)>,
    /// User-defined functions, keyed by name.
    functions: HashMap<String, Arc<FuncValue>>,
    /// User-defined struct definitions (for later `StructLiteral` construction).
    struct_defs: HashMap<String, StructDecl>,
    /// Built-in library, provided by the host.
    builtins: HashMap<String, BuiltinFn>,
    /// Log buffer; UI reads from this.
    pub log: Arc<Mutex<Vec<String>>>,
    /// Set to true to stop the interpreter on the next statement.
    pub stop: Arc<AtomicBool>,
    /// Start time, for `now()`.
    start_time: Instant,
    /// Depth guard against infinite recursion in function calls.
    call_depth: usize,
}

const MAX_CALL_DEPTH: usize = 512;

impl Interpreter {
    pub fn new(
        builtins: HashMap<String, BuiltinFn>,
        log: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            scope: ScopeStack::new(),
            named: HashMap::new(),
            functions: HashMap::new(),
            struct_defs: HashMap::new(),
            builtins,
            log,
            stop,
            start_time: Instant::now(),
            call_depth: 0,
        }
    }

    /// Run a program. This is the top-level entry point.
    pub fn run(&mut self, program: &Program) -> RResult<Value> {
        // Two-pass: register all top-level declarations (functions,
        // structs, enums, addr declarations) before executing any
        // statements. This lets functions call each other regardless
        // of source order.
        self.collect_declarations(&program.statements)?;

        // Then execute the statements in order.
        for stmt in &program.statements {
            match self.exec_stmt(stmt)? {
                Flow::Normal => {}
                Flow::Return(v) => return Ok(v),
                Flow::Throw(v) => {
                    return Err(RuntimeError {
                        message: format!("uncaught: {}", v),
                        line: stmt.span.line,
                        col: stmt.span.col,
                    });
                }
                Flow::Break | Flow::Continue => {
                    return Err(RuntimeError {
                        message: "break/continue outside loop".into(),
                        line: stmt.span.line,
                        col: stmt.span.col,
                    });
                }
            }
        }
        Ok(Value::Void)
    }

    // ------------------------------------------------------------
    // Pre-pass: register declarations
    // ------------------------------------------------------------

    fn collect_declarations(&mut self, stmts: &[Stmt]) -> RResult<()> {
        for stmt in stmts {
            match &stmt.node {
                StmtKind::FuncDecl(f) => {
                    let fv = Arc::new(FuncValue {
                        name: f.name.clone(),
                        params: f.params.clone(),
                        body: f.body.clone(),
                        captures: HashMap::new(),
                    });
                    self.functions.insert(f.name.clone(), fv);
                }
                StmtKind::StructDecl(s) => {
                    self.struct_defs.insert(s.name.clone(), s.clone());
                }
                StmtKind::ImplDecl(_) => {
                    // Methods are attached to struct defs; for v1 we
                    // store them under `TypeName::method` as functions.
                    // Handled later; kept as no-op here.
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------

    fn exec_stmt(&mut self, stmt: &Stmt) -> RResult<Flow> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(RuntimeError {
                message: "script stopped".into(),
                line: stmt.span.line,
                col: stmt.span.col,
            });
        }

        match &stmt.node {
            StmtKind::Let { name, mutable, ty, value } => {
                let _ = mutable;
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Null,
                };
                let _ = ty;
                self.scope.define(name, v);
                Ok(Flow::Normal)
            }

            StmtKind::Const { name, ty, value } => {
                let _ = ty;
                let v = self.eval(value)?;
                self.scope.define(name, v);
                Ok(Flow::Normal)
            }

            StmtKind::AddrDecl { name, address, ty } => {
                let addr_val = self.eval(address)?;
                let addr = addr_val.as_ptr().ok_or_else(|| self.err_stmt(
                    stmt,
                    format!("address must be a pointer or int, got {}", addr_val.type_name()),
                ))?;
                self.named.insert(name.clone(), (addr, ty.node.clone()));
                Ok(Flow::Normal)
            }

            StmtKind::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::Normal)
            }

            StmtKind::Assign { target, op, value } => {
                self.exec_assign(target, *op, value)?;
                Ok(Flow::Normal)
            }

            StmtKind::If { cond, then_block, elifs, else_block } => {
                if self.eval(cond)?.is_truthy() {
                    return self.exec_block(then_block);
                }
                for (c, b) in elifs {
                    if self.eval(c)?.is_truthy() {
                        return self.exec_block(b);
                    }
                }
                if let Some(b) = else_block {
                    return self.exec_block(b);
                }
                Ok(Flow::Normal)
            }

            StmtKind::While { cond, body } => {
                while self.eval(cond)?.is_truthy() {
                    match self.exec_block(body)? {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => continue,
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }

            StmtKind::Loop { interval_ms, body } => {
                let sleep_ms: u64 = match interval_ms {
                    Some(e) => {
                        let v = self.eval(e)?;
                        v.as_int().unwrap_or(0).max(0) as u64
                    }
                    None => 0,
                };
                loop {
                    if self.stop.load(Ordering::Relaxed) {
                        return Ok(Flow::Normal);
                    }
                    match self.exec_block(body)? {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => {}
                        other => return Ok(other),
                    }
                    if sleep_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                    }
                }
                Ok(Flow::Normal)
            }

            StmtKind::For { name, iterable, body } => {
                let iter_val = self.eval(iterable)?;
                let items: Vec<Value> = match iter_val {
                    Value::Vec(v) => v.as_ref().clone(),
                    Value::Set(v) => v.as_ref().clone(),
                    Value::Tuple(v) => v.as_ref().clone(),
                    Value::Bytes(b) => b.iter().map(|x| Value::Int(*x as i64)).collect(),
                    Value::Str(s) => s.chars().map(|c| Value::str(c.to_string())).collect(),
                    _ => {
                        return Err(self.err_stmt(
                            stmt,
                            format!("value of type {} is not iterable", iter_val.type_name()),
                        ))
                    }
                };
                for item in items {
                    self.scope.push();
                    self.scope.define(name, item);
                    let flow = self.exec_block(body)?;
                    self.scope.pop();
                    match flow {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => continue,
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }

            StmtKind::Match { subject, arms } => {
                let val = self.eval(subject)?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &val)? {
                        self.scope.push();
                        for (k, v) in bindings {
                            self.scope.define(&k, v);
                        }
                        // Guard?
                        if let Some(g) = &arm.guard {
                            let guard_val = self.eval(g)?;
                            if !guard_val.is_truthy() {
                                self.scope.pop();
                                continue;
                            }
                        }
                        let result = self.eval(&arm.body);
                        self.scope.pop();
                        result?;
                        return Ok(Flow::Normal);
                    }
                }
                Ok(Flow::Normal)
            }

            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
            StmtKind::Return(e) => {
                let v = match e {
                    Some(expr) => self.eval(expr)?,
                    None => Value::Void,
                };
                Ok(Flow::Return(v))
            }
            StmtKind::Throw(e) => {
                let v = self.eval(e)?;
                Ok(Flow::Throw(v))
            }

            StmtKind::Defer(_) => {
                // Defer is not yet wired through the interpreter.
                // For v1, we accept the syntax but do nothing.
                Ok(Flow::Normal)
            }

            StmtKind::Try { body, catch, finally } => {
                let body_result = self.exec_block(body);
                match body_result {
                    Ok(Flow::Throw(err)) => {
                        if let Some((name, handler)) = catch {
                            self.scope.push();
                            self.scope.define(name, err);
                            let r = self.exec_block(handler)?;
                            self.scope.pop();
                            if let Some(fin) = finally {
                                self.exec_block(fin)?;
                            }
                            return Ok(r);
                        }
                        if let Some(fin) = finally {
                            self.exec_block(fin)?;
                        }
                        Ok(Flow::Throw(err))
                    }
                    other => {
                        if let Some(fin) = finally {
                            self.exec_block(fin)?;
                        }
                        other
                    }
                }
            }

            StmtKind::FuncDecl(_) => Ok(Flow::Normal), // handled in pre-pass
            StmtKind::StructDecl(_) => Ok(Flow::Normal),
            StmtKind::EnumDecl(_) => Ok(Flow::Normal),
            StmtKind::ImplDecl(_) => Ok(Flow::Normal),

            StmtKind::TypeAlias { name, generics: _, ty } => {
                self.named.insert(
                    format!("__type_{}", name),
                    (0, ty.node.clone()),
                );
                Ok(Flow::Normal)
            }

            StmtKind::ScriptDecl { body, .. } => self.exec_block(body),

            StmtKind::Import(_) => {
                // Imports are resolved before interpretation by the host.
                Ok(Flow::Normal)
            }

            StmtKind::Export(_) => Ok(Flow::Normal),

            StmtKind::Breakpoint(label) => {
                self.push_log(format!("[breakpoint] {}", label));
                Ok(Flow::Normal)
            }

            StmtKind::Assert { cond, message } => {
                let c = self.eval(cond)?;
                if !c.is_truthy() {
                    let msg = match message {
                        Some(m) => format!("assertion failed: {}", self.eval(m)?),
                        None => "assertion failed".into(),
                    };
                    return Err(self.err_stmt(stmt, msg));
                }
                Ok(Flow::Normal)
            }
        }
    }

    fn exec_block(&mut self, block: &Block) -> RResult<Flow> {
        self.scope.push();
        for stmt in &block.statements {
            match self.exec_stmt(stmt)? {
                Flow::Normal => {}
                other => {
                    self.scope.pop();
                    return Ok(other);
                }
            }
        }
        self.scope.pop();
        Ok(Flow::Normal)
    }

    // ------------------------------------------------------------
    // Assignment
    // ------------------------------------------------------------

    fn exec_assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) -> RResult<()> {
        // Compute the new value.
        let new_value = if op == AssignOp::Assign {
            self.eval(value)?
        } else {
            // Compound assignment: read current, apply op, write back.
            let current = self.eval(target)?;
            let rhs = self.eval(value)?;
            let binop = compound_to_binop(op);
            self.eval_binary(binop, current, rhs, target)?
        };

        // Assign to the target.
        match &target.node {
            ExprKind::Ident(name) => {
                if !self.scope.set(name, new_value.clone()) {
                    // Not found in scopes — define at the top level.
                    self.scope.define(name, new_value);
                }
                Ok(())
            }
            ExprKind::Field { receiver, name } => {
                let obj = self.eval(receiver)?;
                if let Value::Struct(s) = obj {
                    let mut cloned = (*s).clone();
                    cloned.set(name, new_value);
                    if let ExprKind::Ident(rname) = &receiver.node {
                        self.scope.set(rname, Value::Struct(Arc::new(cloned)));
                        Ok(())
                    } else {
                        Err(self.err_at(target, "cannot assign to a temporary struct"))
                    }
                } else {
                    Err(self.err_at(target, "field assignment on non-struct"))
                }
            }
            ExprKind::Index { .. } => {
                Err(self.err_at(target, "indexed assignment not yet supported"))
            }
            _ => Err(self.err_at(target, "invalid assignment target")),
        }
    }

    // ------------------------------------------------------------
    // Expression evaluation
    // ------------------------------------------------------------

    pub fn eval(&mut self, expr: &Expr) -> RResult<Value> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(self.err_at(expr, "script stopped"));
        }

        match &expr.node {
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Float(f) => Ok(Value::Float(*f)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Str(s) => Ok(Value::str(s.clone())),
            ExprKind::RawStr(s) => Ok(Value::str(s.clone())),
            ExprKind::ByteStr(b) => Ok(Value::bytes(b.clone())),
            ExprKind::Char(c) => Ok(Value::str(c.to_string())),
            ExprKind::Null | ExprKind::NoneLit => Ok(Value::Null),

            ExprKind::Ident(name) => {
                if let Some(v) = self.scope.get(name) {
                    return Ok(v);
                }
                if let Some((addr, _)) = self.named.get(name) {
                    return Ok(Value::Ptr(*addr));
                }
                if self.functions.contains_key(name) {
                    let f = self.functions.get(name).unwrap().clone();
                    return Ok(Value::Func(f));
                }
                // Enum variant with no payload: treat as opaque string.
                if name.contains("::") {
                    return Ok(Value::str(name.clone()));
                }
                Err(self.err_at(expr, format!("undefined name `{}`", name)))
            }

            ExprKind::Unary { op, operand } => {
                let v = self.eval(operand)?;
                self.eval_unary(*op, v, expr)
            }

            ExprKind::Binary { op, left, right } => {
                // Short-circuit for `and` / `or`.
                if *op == BinaryOp::And {
                    let l = self.eval(left)?;
                    if !l.is_truthy() {
                        return Ok(Value::Bool(false));
                    }
                    let r = self.eval(right)?;
                    return Ok(Value::Bool(r.is_truthy()));
                }
                if *op == BinaryOp::Or {
                    let l = self.eval(left)?;
                    if l.is_truthy() {
                        return Ok(Value::Bool(true));
                    }
                    let r = self.eval(right)?;
                    return Ok(Value::Bool(r.is_truthy()));
                }
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                self.eval_binary(*op, l, r, expr)
            }

            ExprKind::IfExpr { cond, then_branch, else_branch } => {
                if self.eval(cond)?.is_truthy() {
                    self.eval(then_branch)
                } else {
                    self.eval(else_branch)
                }
            }

            ExprKind::Call { callee, args } => {
                // Evaluate args.
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval(&a.value))
                    .collect::<RResult<_>>()?;

                // If callee is an Ident, prefer built-in, then user func.
                if let ExprKind::Ident(name) = &callee.node {
                    return self.call_named(name, arg_values, expr);
                }

                // Otherwise evaluate the callee to get a Value::Func.
                let fv = self.eval(callee)?;
                match fv {
                    Value::Func(f) => self.call_func(&f, arg_values, expr),
                    other => Err(self.err_at(
                        expr,
                        format!("value of type {} is not callable", other.type_name()),
                    )),
                }
            }

            ExprKind::MethodCall { receiver, method, args } => {
                let recv = self.eval(receiver)?;
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval(&a.value))
                    .collect::<RResult<_>>()?;
                self.call_method(&recv, method, arg_values, expr)
            }

            ExprKind::GenericMethodCall { receiver, method, args, .. } => {
                let recv = self.eval(receiver)?;
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval(&a.value))
                    .collect::<RResult<_>>()?;
                self.call_method(&recv, method, arg_values, expr)
            }

            ExprKind::Field { receiver, name } => {
                let recv = self.eval(receiver)?;
                self.field_access(&recv, name, expr)
            }

            ExprKind::TupleField { receiver, index } => {
                let recv = self.eval(receiver)?;
                if let Value::Tuple(t) = recv {
                    t.get(*index).cloned().ok_or_else(|| {
                        self.err_at(expr, format!("tuple has no element {}", index))
                    })
                } else {
                    Err(self.err_at(expr, "tuple field on non-tuple"))
                }
            }

            ExprKind::Index { receiver, index } => {
                let r = self.eval(receiver)?;
                let i = self.eval(index)?;
                self.index_access(&r, &i, expr)
            }

            ExprKind::Slice { receiver, range } => {
                let r = self.eval(receiver)?;
                let start = match &range.start {
                    Some(e) => self.eval(e)?.as_int().unwrap_or(0).max(0) as usize,
                    None => 0,
                };
                let end = match &range.end {
                    Some(e) => self.eval(e)?.as_int().unwrap_or(0).max(0) as usize,
                    None => usize::MAX,
                };
                self.slice_value(&r, start, end, expr)
            }

            ExprKind::VecLiteral(items) => {
                let v: Vec<Value> = items
                    .iter()
                    .map(|e| self.eval(e))
                    .collect::<RResult<_>>()?;
                Ok(Value::vec(v))
            }

            ExprKind::TupleLiteral(items) => {
                let v: Vec<Value> = items
                    .iter()
                    .map(|e| self.eval(e))
                    .collect::<RResult<_>>()?;
                Ok(Value::tuple(v))
            }

            ExprKind::MapLiteral(_) | ExprKind::SetLiteral(_) => {
                Err(self.err_at(expr, "map/set literals not yet implemented"))
            }

            ExprKind::StructLiteral { name, fields, .. } => {
                let mut field_values = Vec::new();
                for (fname, fexpr) in fields {
                    field_values.push((fname.clone(), self.eval(fexpr)?));
                }
                let s = super::value::StructInstance {
                    type_name: name.clone(),
                    fields: field_values,
                };
                Ok(Value::Struct(Arc::new(s)))
            }

            ExprKind::Range { .. } => {
                Err(self.err_at(expr, "range expressions not yet supported"))
            }

            ExprKind::Cast { expr: inner, ty } => {
                let v = self.eval(inner)?;
                self.cast_value(v, &ty.node, expr)
            }

            ExprKind::IsType { .. } => Ok(Value::Bool(false)),
            ExprKind::SizeOf(ty) => Ok(Value::Int(type_size(&ty.node) as i64)),
            ExprKind::OffsetOf { .. } => Ok(Value::Int(0)),
            ExprKind::TypeOf(inner) => {
                let v = self.eval(inner)?;
                Ok(Value::str(v.type_name().to_string()))
            }

            ExprKind::Coalesce { left, right } => {
                let l = self.eval(left)?;
                if l.is_null() {
                    self.eval(right)
                } else {
                    Ok(l)
                }
            }

            ExprKind::TryPropagate(inner) => {
                // Unwrap Ok/Some; propagate Err/None as a throw.
                let v = self.eval(inner)?;
                match v {
                    Value::Ok(x) => Ok((*x).clone()),
                    Value::Some(x) => Ok((*x).clone()),
                    Value::Err(e) => Ok(Flow::Throw((*e).clone()).into_value()),
                    Value::None => Ok(Value::Null),
                    other => Ok(other),
                }
            }

            ExprKind::Lambda { params, body } => {
                // Capture current scope.
                let mut captures = HashMap::new();
                for scope in &self.scope.scopes {
                    for (k, v) in scope {
                        captures.insert(k.clone(), v.clone());
                    }
                }
                let f = FuncValue {
                    name: "<lambda>".into(),
                    params: params.clone(),
                    body: (**body).clone(),
                    captures,
                };
                Ok(Value::Func(Arc::new(f)))
            }

            ExprKind::BlockExpr(b) => {
                self.exec_block(b)?;
                Ok(Value::Void)
            }

            ExprKind::Match { subject, arms } => {
                let v = self.eval(subject)?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &v)? {
                        self.scope.push();
                        for (k, bv) in bindings {
                            self.scope.define(&k, bv);
                        }
                        let result = self.eval(&arm.body);
                        self.scope.pop();
                        return result;
                    }
                }
                Ok(Value::Void)
            }

            ExprKind::Some(inner) => Ok(Value::some(self.eval(inner)?)),
            ExprKind::Ok(inner) => Ok(Value::ok(self.eval(inner)?)),
            ExprKind::Err(inner) => Ok(Value::err(self.eval(inner)?)),

            ExprKind::TryExpr(b) => {
                self.exec_block(b)?;
                Ok(Value::Void)
            }

            ExprKind::OptionalField { receiver, name } => {
                let recv = self.eval(receiver)?;
                if recv.is_null() {
                    Ok(Value::Null)
                } else {
                    self.field_access(&recv, name, expr)
                }
            }
        }
    }

    // ------------------------------------------------------------
    // Operators
    // ------------------------------------------------------------

    fn eval_unary(&self, op: UnaryOp, v: Value, span: &Expr) -> RResult<Value> {
        Ok(match op {
            UnaryOp::Neg => match v {
                Value::Int(n) => Value::Int(-n),
                Value::Float(f) => Value::Float(-f),
                other => {
                    return Err(self.err_at(
                        span,
                        format!("cannot negate {}", other.type_name()),
                    ))
                }
            },
            UnaryOp::Not => Value::Bool(!v.is_truthy()),
            UnaryOp::BitNot => match v {
                Value::Int(n) => Value::Int(!n),
                other => {
                    return Err(self.err_at(
                        span,
                        format!("cannot bitwise-not {}", other.type_name()),
                    ))
                }
            },
            UnaryOp::Deref => v,
            UnaryOp::Ref | UnaryOp::RefMut => v,
        })
    }

    fn eval_binary(
        &self,
        op: BinaryOp,
        l: Value,
        r: Value,
        span: &Expr,
    ) -> RResult<Value> {
        use BinaryOp::*;

        // Comparison first — these work on any pair of values and must
        // be checked before the numeric fast-path below.
        match op {
            Eq => return Ok(Value::Bool(l == r)),
            Ne => return Ok(Value::Bool(l != r)),
            Lt => return Ok(Value::Bool(cmp_values(&l, &r)? < 0)),
            Le => return Ok(Value::Bool(cmp_values(&l, &r)? <= 0)),
            Gt => return Ok(Value::Bool(cmp_values(&l, &r)? > 0)),
            Ge => return Ok(Value::Bool(cmp_values(&l, &r)? >= 0)),
            _ => {}
        }

        // String concat via `+`.
        if op == Add {
            if let (Value::Str(a), b) = (&l, &r) {
                return Ok(Value::str(format!("{}{}", a, b)));
            }
        }

        // Numeric ops.
        if l.is_numeric() && r.is_numeric() {
            let both_int = matches!((&l, &r), (Value::Int(_), Value::Int(_)));
            if both_int {
                let a = l.as_int().unwrap();
                let b = r.as_int().unwrap();
                return Ok(match op {
                    Add => Value::Int(a + b),
                    Sub => Value::Int(a - b),
                    Mul => Value::Int(a.wrapping_mul(b)),
                    Div => {
                        if b == 0 {
                            return Err(self.err_at(span, "division by zero"));
                        }
                        Value::Int(a / b)
                    }
                    Mod => {
                        if b == 0 {
                            return Err(self.err_at(span, "modulo by zero"));
                        }
                        Value::Int(a % b)
                    }
                    Pow => Value::Int((a as f64).powf(b as f64) as i64),
                    BitAnd => Value::Int(a & b),
                    BitOr => Value::Int(a | b),
                    BitXor => Value::Int(a ^ b),
                    Shl => Value::Int(a << b),
                    Shr => Value::Int(a >> b),
                    Ushr => Value::Int(((a as u64) >> b) as i64),
                    _ => {
                        return Err(self.err_at(
                            span,
                            format!("unhandled int operator {:?}", op),
                        ));
                    }
                });
            }
            // At least one float.
            let a = l.as_float().unwrap();
            let b = r.as_float().unwrap();
            return Ok(match op {
                Add => Value::Float(a + b),
                Sub => Value::Float(a - b),
                Mul => Value::Float(a * b),
                Div => Value::Float(a / b),
                Mod => Value::Float(a % b),
                Pow => Value::Float(a.powf(b)),
                _ => {
                    return Err(self.err_at(
                        span,
                        format!("unhandled float operator {:?}", op),
                    ));
                }
            });
        }

        // Any operator that reaches here isn't supported for these types.
        Err(self.err_at(
            span,
            format!(
                "operator not supported for {} and {}",
                l.type_name(),
                r.type_name()
            ),
        ))
    }

    // ------------------------------------------------------------
    // Calls
    // ------------------------------------------------------------

    fn call_named(&mut self, name: &str, args: Vec<Value>, span: &Expr) -> RResult<Value> {
        // Built-in first.
        if let Some(f) = self.builtins.get(name).cloned() {
            return f(&args).map_err(|e| RuntimeError {
                message: e.message,
                line: span.span.line,
                col: span.span.col,
            });
        }
        // User-defined function.
        if let Some(f) = self.functions.get(name).cloned() {
            return self.call_func(&f, args, span);
        }
        Err(self.err_at(span, format!("undefined function `{}`", name)))
    }

    fn call_func(&mut self, f: &FuncValue, args: Vec<Value>, span: &Expr) -> RResult<Value> {
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(self.err_at(span, "stack overflow: too many nested calls"));
        }

        self.scope.push();

        // Bind captures (for closures).
        for (k, v) in &f.captures {
            self.scope.define(k, v.clone());
        }

        // Bind parameters.
        for (i, param) in f.params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or_else(|| {
                param.default.as_ref().map(|_| Value::Null).unwrap_or(Value::Null)
            });
            self.scope.define(&param.name, v);
        }

        self.call_depth += 1;
        let result = self.exec_block(&f.body);
        self.call_depth -= 1;

        let flow = result?;
        self.scope.pop();

        Ok(match flow {
            Flow::Normal => Value::Void,
            Flow::Return(v) => v,
            Flow::Throw(v) => {
                return Err(RuntimeError {
                    message: format!("uncaught: {}", v),
                    line: span.span.line,
                    col: span.span.col,
                });
            }
            Flow::Break | Flow::Continue => Value::Void,
        })
    }

    fn call_method(
        &self,
        recv: &Value,
        method: &str,
        args: Vec<Value>,
        span: &Expr,
    ) -> RResult<Value> {
        // Small set of methods for common value types.
        match (recv, method) {
            // String methods
            (Value::Str(s), "len") => Ok(Value::Int(s.len() as i64)),
            (Value::Str(s), "upper") => Ok(Value::str(s.to_uppercase())),
            (Value::Str(s), "lower") => Ok(Value::str(s.to_lowercase())),
            (Value::Str(s), "trim") => Ok(Value::str(s.trim().to_string())),
            (Value::Str(s), "contains") => {
                let needle = args.first().and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::Bool(s.contains(needle)))
            }
            (Value::Str(s), "starts_with") => {
                let needle = args.first().and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::Bool(s.starts_with(needle)))
            }
            (Value::Str(s), "ends_with") => {
                let needle = args.first().and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::Bool(s.ends_with(needle)))
            }

            // Vec methods
            (Value::Vec(v), "len") => Ok(Value::Int(v.len() as i64)),
            (Value::Vec(v), "first") => Ok(v.first().cloned().unwrap_or(Value::Null)),
            (Value::Vec(v), "last") => Ok(v.last().cloned().unwrap_or(Value::Null)),
            (Value::Vec(v), "is_empty") => Ok(Value::Bool(v.is_empty())),

            // Bytes methods
            (Value::Bytes(b), "len") => Ok(Value::Int(b.len() as i64)),
            (Value::Bytes(b), "hex") => Ok(Value::str(
                b.iter().map(|x| format!("{:02x}", x)).collect::<String>(),
            )),

            // Struct method dispatch would go here. For v1, any method
            // called on a struct returns Void unless a builtin matches.
            (Value::Struct(_), _) => Ok(Value::Void),

            _ => Err(self.err_at(
                span,
                format!(
                    "no method `{}` on {}",
                    method,
                    recv.type_name()
                ),
            )),
        }
    }

    // ------------------------------------------------------------
    // Field and index access
    // ------------------------------------------------------------

    fn field_access(&self, recv: &Value, name: &str, span: &Expr) -> RResult<Value> {
        match recv {
            Value::Struct(s) => s.get(name).cloned().ok_or_else(|| {
                self.err_at(span, format!("struct has no field `{}`", name))
            }),
            _ => Err(self.err_at(
                span,
                format!("no field `{}` on {}", name, recv.type_name()),
            )),
        }
    }

    fn index_access(&self, recv: &Value, idx: &Value, span: &Expr) -> RResult<Value> {
        let i = idx.as_int().unwrap_or(0).max(0) as usize;
        match recv {
            Value::Vec(v) => v.get(i).cloned().ok_or_else(|| {
                self.err_at(span, format!("index {} out of bounds", i))
            }),
            Value::Tuple(v) => v.get(i).cloned().ok_or_else(|| {
                self.err_at(span, format!("tuple index {} out of bounds", i))
            }),
            Value::Bytes(b) => b.get(i).map(|x| Value::Int(*x as i64)).ok_or_else(|| {
                self.err_at(span, format!("index {} out of bounds", i))
            }),
            Value::Str(s) => s
                .chars()
                .nth(i)
                .map(|c| Value::str(c.to_string()))
                .ok_or_else(|| self.err_at(span, format!("index {} out of bounds", i))),
            _ => Err(self.err_at(
                span,
                format!("cannot index into {}", recv.type_name()),
            )),
        }
    }

    fn slice_value(
        &self,
        recv: &Value,
        start: usize,
        end: usize,
        span: &Expr,
    ) -> RResult<Value> {
        let e = end.min(usize::MAX);
        match recv {
            Value::Vec(v) => {
                let s = start.min(v.len());
                let t = e.min(v.len()).max(s);
                Ok(Value::vec(v[s..t].to_vec()))
            }
            Value::Bytes(b) => {
                let s = start.min(b.len());
                let t = e.min(b.len()).max(s);
                Ok(Value::bytes(b[s..t].to_vec()))
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let st = start.min(chars.len());
                let en = e.min(chars.len()).max(st);
                Ok(Value::str(chars[st..en].iter().collect::<String>()))
            }
            _ => Err(self.err_at(
                span,
                format!("cannot slice {}", recv.type_name()),
            )),
        }
    }

    // ------------------------------------------------------------
    // Casts
    // ------------------------------------------------------------

    fn cast_value(&self, v: Value, ty: &TypeKind, span: &Expr) -> RResult<Value> {
        match ty {
            TypeKind::Int
            | TypeKind::Int8
            | TypeKind::Int16
            | TypeKind::Int32
            | TypeKind::Int64 => Ok(Value::Int(v.as_int().unwrap_or(0))),
            TypeKind::Uint
            | TypeKind::Uint8
            | TypeKind::Uint16
            | TypeKind::Uint32
            | TypeKind::Uint64 => Ok(Value::Int(v.as_int().unwrap_or(0))),
            TypeKind::Float
            | TypeKind::Float32
            | TypeKind::Float64 => Ok(Value::Float(v.as_float().unwrap_or(0.0))),
            TypeKind::Bool => Ok(Value::Bool(v.is_truthy())),
            TypeKind::String | TypeKind::CString => Ok(Value::str(format!("{}", v))),
            _ => Err(self.err_at(
                span,
                format!("cast to {:?} not supported", ty),
            )),
        }
    }

    // ------------------------------------------------------------
    // Pattern matching
    // ------------------------------------------------------------

    fn match_pattern(
        &self,
        pat: &Pattern,
        value: &Value,
    ) -> RResult<Option<HashMap<String, Value>>> {
        let mut binds = HashMap::new();
        if self.match_pattern_into(pat, value, &mut binds)? {
            Ok(Some(binds))
        } else {
            Ok(None)
        }
    }

    fn match_pattern_into(
        &self,
        pat: &Pattern,
        value: &Value,
        binds: &mut HashMap<String, Value>,
    ) -> RResult<bool> {
        Ok(match pat {
            Pattern::Wildcard => true,
            Pattern::Binding(name) => {
                binds.insert(name.clone(), value.clone());
                true
            }
            Pattern::Literal(lit) => {
                let lit_val = match lit {
                    Literal::Int(n) => Value::Int(*n),
                    Literal::Float(f) => Value::Float(*f),
                    Literal::Bool(b) => Value::Bool(*b),
                    Literal::Str(s) => Value::str(s.clone()),
                    Literal::Char(c) => Value::str(c.to_string()),
                    Literal::Null => Value::Null,
                };
                lit_val == *value
            }
            Pattern::Range { start, end, inclusive } => {
                let mut start_binds = HashMap::new();
                let mut end_binds = HashMap::new();
                if !self.match_pattern_into(start, value, &mut start_binds)? {
                    return Ok(false);
                }
                let mut end_clone = end.as_ref().clone();
                if !self.match_pattern_into(&mut end_clone, value, &mut end_binds)? {
                    return Ok(false);
                }
                // Range: we can only compare if value has as_int.
                if let (Some(v), Some(s), Some(e)) = (
                    value.as_int(),
                    pattern_as_int(start),
                    pattern_as_int(end),
                ) {
                    if *inclusive {
                        v >= s && v <= e
                    } else {
                        v >= s && v < e
                    }
                } else {
                    false
                }
            }
            Pattern::Or(pats) => {
                for p in pats {
                    let mut sub = HashMap::new();
                    if self.match_pattern_into(p, value, &mut sub)? {
                        for (k, v) in sub {
                            binds.insert(k, v);
                        }
                        return Ok(true);
                    }
                }
                false
            }
            Pattern::Tuple(pats) => {
                if let Value::Tuple(t) = value {
                    if t.len() != pats.len() {
                        return Ok(false);
                    }
                    for (p, v) in pats.iter().zip(t.iter()) {
                        if !self.match_pattern_into(p, v, binds)? {
                            return Ok(false);
                        }
                    }
                    true
                } else {
                    false
                }
            }
            Pattern::Slice { .. } => false,
            Pattern::Struct { .. } => false,
            Pattern::TupleVariant { .. } => false,
            Pattern::StructVariant { .. } => false,
        })
    }

    // ------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------

    fn err_at(&self, span: &Expr, message: impl Into<String>) -> RuntimeError {
        RuntimeError {
            message: message.into(),
            line: span.span.line,
            col: span.span.col,
        }
    }

    fn err_stmt(&self, span: &Stmt, message: impl Into<String>) -> RuntimeError {
        RuntimeError {
            message: message.into(),
            line: span.span.line,
            col: span.span.col,
        }
    }

    pub fn push_log(&self, msg: String) {
        if let Ok(mut log) = self.log.lock() {
            log.push(msg);
            if log.len() > 5000 {
                let overflow = log.len() - 5000;
                log.drain(0..overflow);
            }
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }
}

// ============================================================
// FREE HELPERS
// ============================================================

fn compound_to_binop(op: AssignOp) -> BinaryOp {
    match op {
        AssignOp::Assign => BinaryOp::Add, // never used
        AssignOp::AddAssign => BinaryOp::Add,
        AssignOp::SubAssign => BinaryOp::Sub,
        AssignOp::MulAssign => BinaryOp::Mul,
        AssignOp::DivAssign => BinaryOp::Div,
        AssignOp::ModAssign => BinaryOp::Mod,
        AssignOp::PowAssign => BinaryOp::Pow,
        AssignOp::BitAndAssign => BinaryOp::BitAnd,
        AssignOp::BitOrAssign => BinaryOp::BitOr,
        AssignOp::BitXorAssign => BinaryOp::BitXor,
        AssignOp::ShlAssign => BinaryOp::Shl,
        AssignOp::ShrAssign => BinaryOp::Shr,
        AssignOp::UshrAssign => BinaryOp::Ushr,
    }
}

fn cmp_values(a: &Value, b: &Value) -> RResult<i32> {
    if let (Some(x), Some(y)) = (a.as_float(), b.as_float()) {
        return Ok(if x < y { -1 } else if x > y { 1 } else { 0 });
    }
    if let (Some(x), Some(y)) = (a.as_str(), b.as_str()) {
        return Ok(match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
    }
    Err(RuntimeError {
        message: format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        ),
        line: 0,
        col: 0,
    })
}

fn pattern_as_int(p: &Pattern) -> Option<i64> {
    if let Pattern::Literal(Literal::Int(n)) = p {
        Some(*n)
    } else {
        None
    }
}

fn type_size(ty: &TypeKind) -> usize {
    match ty {
        TypeKind::Int8 | TypeKind::Uint8 | TypeKind::Bool | TypeKind::Bool8 => 1,
        TypeKind::Int16 | TypeKind::Uint16 => 2,
        TypeKind::Int32 | TypeKind::Uint32 | TypeKind::Float32 => 4,
        TypeKind::Int | TypeKind::Int64 | TypeKind::Uint | TypeKind::Uint64
        | TypeKind::Float | TypeKind::Float64 | TypeKind::Ptr => 8,
        TypeKind::String | TypeKind::CString | TypeKind::WString | TypeKind::Bytes => 8,
        _ => 8,
    }
}

// ============================================================
// Small extension so we can convert Flow to Value in try-propagation.
// ============================================================

impl Flow {
    fn into_value(self) -> Value {
        match self {
            Flow::Return(v) | Flow::Throw(v) => v,
            _ => Value::Void,
        }
    }
}

// ============================================================
// TESTS
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::script::parser::parse;

    fn run(src: &str) -> RResult<()> {
        let program = match parse(src) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("PARSE ERROR at {}:{}: {}", e.line, e.col, e.message);
                return Err(RuntimeError {
                    message: e.message,
                    line: e.line,
                    col: e.col,
                });
            }
        };
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut interp = Interpreter::new(HashMap::new(), log, stop);
        match interp.run(&program) {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("RUNTIME ERROR at {}:{}: {}", e.line, e.col, e.message);
                Err(e)
            }
        }
    }

    #[test]
    fn let_binding_and_arithmetic() {
        run("let x = 1 + 2 * 3").unwrap();
    }

    #[test]
    fn division_by_zero_errors() {
        assert!(run("let x = 1 / 0").is_err());
    }

    #[test]
    fn if_else() {
        let src = "if 1 < 2:\n    let x = 1\nelse:\n    let x = 2";
        if let Err(e) = run(src) {
            panic!("if_else failed: {}", e);
        }
    }

}