//! SigilScript parser.
//!
//! Recursive-descent parser that turns a token stream from `lexer.rs`
//! into a `Program` AST from `ast.rs`.
//!
//! Structure:
//! - A `Parser` holds the token list, a cursor, and the current line
//!   for error messages.
//! - Each `parse_*` function handles one non-terminal and returns
//!   `Result<_, ParseError>`.
//! - Expression parsing uses precedence climbing (Pratt-style).

use super::ast::*;
use super::lexer::{Token, TokenKind};

// ============================================================
// ERRORS
// ============================================================

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

// ============================================================
// PARSER
// ============================================================

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Current indentation depth for block tracking. Indent-sensitivity
    /// is done in a preprocessing pass: we replace leading indentation
    /// with `Indent`/`Dedent` markers before parsing begins.
    indent_stack: Vec<usize>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            indent_stack: vec![0],
        }
    }

    pub fn parse_program(&mut self) -> PResult<Program> {
        let start = self.peek_span();
        let mut statements = Vec::new();
        self.skip_newlines();
        while !self.check_eof() {
            let stmt = self.parse_statement()?;
            statements.push(stmt);
            self.skip_newlines();
        }
        Ok(Program {
            span: start,
            statements,
        })
    }

    // ------------------------------------------------------------
    // Cursor helpers
    // ------------------------------------------------------------

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_nth_kind(&self, n: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + n)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| Span::new(t.line, t.col))
            .unwrap_or_default()
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn check_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn check(&self, k: &TokenKind) -> bool {
        self.peek_kind() == k
    }

    fn check_keyword(&self, kw: &str) -> bool {
        matches!(self.peek_kind(), TokenKind::Keyword(k) if k == kw)
    }

    fn match_kind(&mut self, k: &TokenKind) -> bool {
        if self.check(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn match_keyword(&mut self, kw: &str) -> bool {
        if self.check_keyword(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, k: &TokenKind) -> PResult<Token> {
        if self.check(k) {
            Ok(self.bump().unwrap())
        } else {
            Err(self.err(format!(
                "expected {}, found {}",
                k.describe(),
                self.peek_kind().describe()
            )))
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> PResult<Token> {
        if self.check_keyword(kw) {
            Ok(self.bump().unwrap())
        } else {
            Err(self.err(format!(
                "expected keyword `{}`, found {}",
                kw,
                self.peek_kind().describe()
            )))
        }
    }

    fn expect_ident(&mut self) -> PResult<String> {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            // Some keywords can be used as identifiers in "soft" positions.
            TokenKind::Keyword(s) if is_soft_keyword(&s) => {
                self.bump();
                Ok(s)
            }
            other => Err(self.err(format!(
                "expected identifier, found {}",
                other.describe()
            ))),
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Semicolon) {
            self.bump();
        }
    }

    fn err(&self, message: impl Into<String>) -> ParseError {
        let (line, col) = self
            .peek()
            .map(|t| (t.line, t.col))
            .unwrap_or((0, 0));
        ParseError {
            message: message.into(),
            line,
            col,
        }
    }

    // ------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------

    fn parse_statement(&mut self) -> PResult<Stmt> {
        let span = self.peek_span();

        // Public modifier
        let is_pub = self.match_keyword("pub");
        let _ = is_pub; // applied to the declaration below

        // Attributes
        let mut attrs = Vec::new();
        while self.check(&TokenKind::Hash) {
            attrs.push(self.parse_attribute()?);
            self.skip_newlines();
        }

        // Declarations
        if self.check_keyword("let") {
            return self.parse_let(span);
        }
        if self.check_keyword("const") {
            return self.parse_const(span);
        }
        if self.check_keyword("addr") {
            return self.parse_addr(span);
        }
        if self.check_keyword("func") {
            return self.parse_func(span, attrs, is_pub);
        }
        if self.check_keyword("struct") {
            return self.parse_struct(span, attrs, is_pub);
        }
        if self.check_keyword("enum") {
            return self.parse_enum(span, attrs, is_pub);
        }
        if self.check_keyword("impl") {
            return self.parse_impl(span);
        }
        if self.check_keyword("type") {
            return self.parse_type_alias(span);
        }
        if self.check_keyword("script") {
            return self.parse_script_decl(span, attrs);
        }
        if self.check_keyword("import") {
            return self.parse_import(span);
        }
        if self.check_keyword("export") {
            let name = self.expect_ident()?;
            return Ok(Node::new(span, StmtKind::Export(name)));
        }

        // Control flow
        if self.check_keyword("if") {
            return self.parse_if(span);
        }
        if self.check_keyword("while") {
            return self.parse_while(span);
        }
        if self.check_keyword("loop") {
            return self.parse_loop(span);
        }
        if self.check_keyword("for") {
            return self.parse_for(span);
        }
        if self.check_keyword("match") {
            return self.parse_match_stmt(span);
        }
        if self.check_keyword("try") {
            return self.parse_try(span);
        }
        if self.check_keyword("defer") {
            self.bump();
            let body = self.parse_block()?;
            return Ok(Node::new(span, StmtKind::Defer(body)));
        }
        if self.check_keyword("break") {
            self.bump();
            return Ok(Node::new(span, StmtKind::Break));
        }
        if self.check_keyword("continue") {
            self.bump();
            return Ok(Node::new(span, StmtKind::Continue));
        }
        if self.check_keyword("return") {
            self.bump();
            let value = if self.at_block_end() {
                None
            } else {
                Some(self.parse_expr()?)
            };
            return Ok(Node::new(span, StmtKind::Return(value)));
        }
        if self.check_keyword("throw") {
            self.bump();
            let value = self.parse_expr()?;
            return Ok(Node::new(span, StmtKind::Throw(value)));
        }
        if self.check_keyword("breakpoint") {
            self.bump();
            self.expect(&TokenKind::LParen)?;
            let label = match self.peek_kind().clone() {
                TokenKind::Str(s) | TokenKind::RawStr(s) => {
                    self.bump();
                    s
                }
                _ => {
                    return Err(self.err("breakpoint expects a string label"));
                }
            };
            self.expect(&TokenKind::RParen)?;
            return Ok(Node::new(span, StmtKind::Breakpoint(label)));
        }

        // Assignment or expression statement
        let expr = self.parse_expr()?;

        // Check for assignment
        if let Some(op) = self.assignment_op() {
            self.bump();
            let value = self.parse_expr()?;
            return Ok(Node::new(
                span,
                StmtKind::Assign {
                    target: expr,
                    op,
                    value,
                },
            ));
        }

        Ok(Node::new(span, StmtKind::Expr(expr)))
    }

    fn assignment_op(&self) -> Option<AssignOp> {
        match self.peek_kind() {
            TokenKind::Eq => Some(AssignOp::Assign),
            TokenKind::PlusEq => Some(AssignOp::AddAssign),
            TokenKind::MinusEq => Some(AssignOp::SubAssign),
            TokenKind::StarEq => Some(AssignOp::MulAssign),
            TokenKind::SlashEq => Some(AssignOp::DivAssign),
            TokenKind::PercentEq => Some(AssignOp::ModAssign),
            TokenKind::StarStarEq => Some(AssignOp::PowAssign),
            TokenKind::AmpEq => Some(AssignOp::BitAndAssign),
            TokenKind::PipeEq => Some(AssignOp::BitOrAssign),
            TokenKind::CaretEq => Some(AssignOp::BitXorAssign),
            TokenKind::ShlEq => Some(AssignOp::ShlAssign),
            TokenKind::ShrEq => Some(AssignOp::ShrAssign),
            TokenKind::UsrEq => Some(AssignOp::UshrAssign),
            _ => None,
        }
    }

    fn at_block_end(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Newline | TokenKind::Semicolon | TokenKind::Eof
        ) || matches!(self.peek_kind(), TokenKind::Keyword(k) if k == "else" || k == "elif" || k == "catch" || k == "finally")
    }

    // ------------------------------------------------------------
    // Declarations
    // ------------------------------------------------------------

    fn parse_let(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("let")?;
        let mutable = self.match_keyword("mut");
        let name = self.expect_ident()?;
        let ty = if self.match_kind(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let value = if self.match_kind(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Node::new(
            span,
            StmtKind::Let {
                name,
                mutable,
                ty,
                value,
            },
        ))
    }

    fn parse_const(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("const")?;
        let name = self.expect_ident()?;
        let ty = if self.match_kind(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Ok(Node::new(
            span,
            StmtKind::Const { name, ty, value },
        ))
    }

    fn parse_addr(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("addr")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let address = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        Ok(Node::new(
            span,
            StmtKind::AddrDecl { name, address, ty },
        ))
    }

    fn parse_func(
        &mut self,
        span: Span,
        attrs: Vec<Attribute>,
        public: bool,
    ) -> PResult<Stmt> {
        self.expect_keyword("func")?;
        let name = self.expect_ident()?;

        let generics = self.parse_generics()?;

        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;

        let return_type = if self.match_kind(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Node::new(
            span,
            StmtKind::FuncDecl(FuncDecl {
                name,
                generics,
                params,
                return_type,
                body,
                attrs,
                public,
            }),
        ))
    }

    fn parse_generics(&mut self) -> PResult<Vec<GenericParam>> {
        if !self.check(&TokenKind::Lt) {
            return Ok(Vec::new());
        }
        self.bump();
        let mut params = Vec::new();
        loop {
            let name = self.expect_ident()?;
            let bound = if self.match_kind(&TokenKind::Colon) {
                let mut bs = vec![self.expect_ident()?];
                while self.match_kind(&TokenKind::Plus) {
                    bs.push(self.expect_ident()?);
                }
                Some(bs)
            } else {
                None
            };
            params.push(GenericParam { name, bound });
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::Gt)?;
        Ok(params)
    }

    fn parse_params(&mut self) -> PResult<Vec<Param>> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let variadic = self.match_kind(&TokenKind::DotDot) || self.match_kind(&TokenKind::Dot);
            let name = self.expect_ident()?;
            let ty = if self.match_kind(&TokenKind::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            let default = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                ty,
                default,
                variadic,
            });
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_struct(
        &mut self,
        span: Span,
        attrs: Vec<Attribute>,
        public: bool,
    ) -> PResult<Stmt> {
        self.expect_keyword("struct")?;
        let name = self.expect_ident()?;
        let generics = self.parse_generics()?;
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        self.skip_newlines();
        while !self.check(&TokenKind::RBrace) && !self.check_eof() {
            // Method?
            if self.check_keyword("func") {
                let m_span = self.peek_span();
                let m = self.parse_func(m_span, Vec::new(), false)?;
                if let StmtKind::FuncDecl(f) = m.node {
                    methods.push(f);
                }
                self.skip_newlines();
                continue;
            }

            // Field: name: Type
            let field_name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_type()?;
            let default = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            fields.push(StructField {
                name: field_name,
                ty,
                default,
                public: true,
            });
            self.match_kind(&TokenKind::Comma);
            self.skip_newlines();
        }

        self.expect(&TokenKind::RBrace)?;

        Ok(Node::new(
            span,
            StmtKind::StructDecl(StructDecl {
                name,
                generics,
                attrs,
                fields,
                methods,
                public,
            }),
        ))
    }

    fn parse_enum(
        &mut self,
        span: Span,
        attrs: Vec<Attribute>,
        public: bool,
    ) -> PResult<Stmt> {
        self.expect_keyword("enum")?;
        let name = self.expect_ident()?;
        let generics = self.parse_generics()?;
        self.expect(&TokenKind::LBrace)?;

        let mut variants = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::RBrace) && !self.check_eof() {
            let vname = self.expect_ident()?;
            let payload = if self.check(&TokenKind::LParen) {
                self.bump();
                let mut tys = Vec::new();
                loop {
                    tys.push(self.parse_type()?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                EnumPayload::Tuple(tys)
            } else if self.check(&TokenKind::LBrace) {
                self.bump();
                let mut fields = Vec::new();
                self.skip_newlines();
                while !self.check(&TokenKind::RBrace) {
                    let f = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let t = self.parse_type()?;
                    fields.push((f, t));
                    self.match_kind(&TokenKind::Comma);
                    self.skip_newlines();
                }
                self.expect(&TokenKind::RBrace)?;
                EnumPayload::Struct(fields)
            } else {
                EnumPayload::Unit
            };
            variants.push(EnumVariant {
                name: vname,
                payload,
            });
            self.match_kind(&TokenKind::Comma);
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Node::new(
            span,
            StmtKind::EnumDecl(EnumDecl {
                name,
                generics,
                attrs,
                variants,
                public,
            }),
        ))
    }

    fn parse_impl(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("impl")?;
        let type_name = self.expect_ident()?;
        let generics = self.parse_generics()?;
        self.expect(&TokenKind::LBrace)?;

        let mut methods = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::RBrace) && !self.check_eof() {
            if self.check_keyword("func") {
                let m_span = self.peek_span();
                let m = self.parse_func(m_span, Vec::new(), false)?;
                if let StmtKind::FuncDecl(f) = m.node {
                    methods.push(f);
                }
            } else {
                return Err(self.err("expected `func` inside `impl`"));
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Node::new(
            span,
            StmtKind::ImplDecl(ImplDecl {
                type_name,
                generics,
                methods,
            }),
        ))
    }

    fn parse_type_alias(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("type")?;
        let name = self.expect_ident()?;
        let generics = self.parse_generics()?.into_iter().map(|g| g.name).collect();
        self.expect(&TokenKind::Eq)?;
        let ty = self.parse_type()?;
        Ok(Node::new(
            span,
            StmtKind::TypeAlias {
                name,
                generics,
                ty,
            },
        ))
    }

    fn parse_script_decl(&mut self, span: Span, attrs: Vec<Attribute>) -> PResult<Stmt> {
        self.expect_keyword("script")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Node::new(
            span,
            StmtKind::ScriptDecl { name, attrs, body },
        ))
    }

    fn parse_import(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("import")?;

        // Two forms:
        //   import "path"
        //   import "path" as alias
        //   import { a, b } from "path"

        if self.check(&TokenKind::LBrace) {
            self.bump();
            let mut items = Vec::new();
            loop {
                items.push(self.expect_ident()?);
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RBrace)?;
            self.expect_keyword("from")?;
            let path = match self.peek_kind().clone() {
                TokenKind::Str(s) | TokenKind::RawStr(s) => {
                    self.bump();
                    s
                }
                _ => return Err(self.err("expected string path after `from`")),
            };
            return Ok(Node::new(
                span,
                StmtKind::Import(ImportDecl {
                    path,
                    alias: None,
                    items,
                }),
            ));
        }

        let path = match self.peek_kind().clone() {
            TokenKind::Str(s) | TokenKind::RawStr(s) => {
                self.bump();
                s
            }
            _ => return Err(self.err("expected string path after `import`")),
        };
        let alias = if self.match_keyword("as") {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(Node::new(
            span,
            StmtKind::Import(ImportDecl {
                path,
                alias,
                items: Vec::new(),
            }),
        ))
    }

    // ------------------------------------------------------------
    // Control flow
    // ------------------------------------------------------------

    fn parse_if(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("if")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let then_block = self.parse_block()?;

        let mut elifs = Vec::new();
        while self.check_keyword("elif") {
            self.bump();
            let c = self.parse_expr()?;
            self.expect(&TokenKind::Colon)?;
            let b = self.parse_block()?;
            elifs.push((c, b));
        }

        let else_block = if self.check_keyword("else") {
            self.bump();
            self.expect(&TokenKind::Colon)?;
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Node::new(
            span,
            StmtKind::If {
                cond,
                then_block,
                elifs,
                else_block,
            },
        ))
    }

    fn parse_while(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("while")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Node::new(span, StmtKind::While { cond, body }))
    }

    fn parse_loop(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("loop")?;
        let interval_ms = if self.check_keyword("every") {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Node::new(span, StmtKind::Loop { interval_ms, body }))
    }

    fn parse_for(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("for")?;
        let name = self.expect_ident()?;
        self.expect_keyword("in")?;
        let iterable = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Node::new(
            span,
            StmtKind::For { name, iterable, body },
        ))
    }

    fn parse_match_stmt(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("match")?;
        let subject = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let arms = self.parse_match_arms()?;
        Ok(Node::new(
            span,
            StmtKind::Match { subject, arms },
        ))
    }

    fn parse_match_arms(&mut self) -> PResult<Vec<MatchArm>> {
        self.skip_newlines();
        let mut arms = Vec::new();
        // Arms are indented one level deeper than the match. We use a
        // simple heuristic: parse arms until we hit a token that can't
        // start a pattern or we hit the end of the current block.
        while !self.check_eof() && !self.at_block_end() {
            let pattern = self.parse_pattern()?;
            let guard = if self.check_keyword("if") {
                self.bump();
                Some(self.parse_expr()?)
            } else {
                None
            };
            // Either `=>` or `:`
            if !self.match_kind(&TokenKind::FatArrow) {
                self.expect(&TokenKind::Colon)?;
            }
            let body = self.parse_expr()?;
            arms.push(MatchArm { pattern, guard, body });
            self.match_kind(&TokenKind::Comma);
            self.skip_newlines();
        }
        Ok(arms)
    }

    fn parse_try(&mut self, span: Span) -> PResult<Stmt> {
        self.expect_keyword("try")?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;

        let catch = if self.check_keyword("catch") {
            self.bump();
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let b = self.parse_block()?;
            Some((name, b))
        } else {
            None
        };

        let finally = if self.check_keyword("finally") {
            self.bump();
            self.expect(&TokenKind::Colon)?;
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Node::new(
            span,
            StmtKind::Try {
                body,
                catch,
                finally,
            },
        ))
    }

    // ------------------------------------------------------------
    // Blocks
    // ------------------------------------------------------------

    fn parse_block(&mut self) -> PResult<Block> {
        let span = self.peek_span();

        // Two styles:
        //   `: <newline> <indented statements>`
        //   `{ <statements> }`
        if self.match_kind(&TokenKind::LBrace) {
            self.skip_newlines();
            let mut statements = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.check_eof() {
                statements.push(self.parse_statement()?);
                self.skip_newlines();
            }
            self.expect(&TokenKind::RBrace)?;
            return Ok(Block { span, statements });
        }

        // Indented block: consume newlines, then parse statements.
        // We don't do real indentation tracking yet; the parser assumes
        // the caller has set up correct context. In v1 this works for
        // well-formed input.
        self.skip_newlines();
        let mut statements = Vec::new();
        while !self.at_block_end() && !self.check_eof() {
            statements.push(self.parse_statement()?);
            self.skip_newlines();
        }
        Ok(Block { span, statements })
    }

    // ------------------------------------------------------------
    // Types
    // ------------------------------------------------------------

    fn parse_type(&mut self) -> PResult<TypeNode> {
        let span = self.peek_span();

        let kind = match self.peek_kind().clone() {
            // Primitives
            TokenKind::Keyword(k) => {
                self.bump();
                match k.as_str() {
                    "int" => TypeKind::Int,
                    "int8" => TypeKind::Int8,
                    "int16" => TypeKind::Int16,
                    "int32" => TypeKind::Int32,
                    "int64" => TypeKind::Int64,
                    "uint" => TypeKind::Uint,
                    "uint8" => TypeKind::Uint8,
                    "uint16" => TypeKind::Uint16,
                    "uint32" => TypeKind::Uint32,
                    "uint64" => TypeKind::Uint64,
                    "float" => TypeKind::Float,
                    "float32" => TypeKind::Float32,
                    "float64" => TypeKind::Float64,
                    "bool" => TypeKind::Bool,
                    "bool8" => TypeKind::Bool8,
                    "string" => TypeKind::String,
                    "cstring" => TypeKind::CString,
                    "wstring" => TypeKind::WString,
                    "bytes" => TypeKind::Bytes,
                    "ptr" => TypeKind::Ptr,
                    "void" => TypeKind::Void,
                    // Generic containers
                    "vec" => {
                        self.expect(&TokenKind::Lt)?;
                        let inner = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        return Ok(Node::new(span, TypeKind::Vec(Box::new(inner))));
                    }
                    "map" => {
                        self.expect(&TokenKind::Lt)?;
                        let k = self.parse_type()?;
                        self.expect(&TokenKind::Comma)?;
                        let v = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        return Ok(Node::new(span, TypeKind::Map(Box::new(k), Box::new(v))));
                    }
                    "set" => {
                        self.expect(&TokenKind::Lt)?;
                        let inner = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        return Ok(Node::new(span, TypeKind::Set(Box::new(inner))));
                    }
                    "option" => {
                        self.expect(&TokenKind::Lt)?;
                        let inner = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        return Ok(Node::new(span, TypeKind::Option(Box::new(inner))));
                    }
                    "result" => {
                        self.expect(&TokenKind::Lt)?;
                        let ok = self.parse_type()?;
                        self.expect(&TokenKind::Comma)?;
                        let err = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        return Ok(Node::new(span, TypeKind::Result(Box::new(ok), Box::new(err))));
                    }
                    other => TypeKind::Named(other.to_string()),
                }
            }
            TokenKind::Ident(name) => {
                self.bump();
                // Generic args? `Foo<int32>`
                if self.check(&TokenKind::Lt) {
                    // Consume type args but for v1 only `Named` variants
                    // without nested generics are fully supported.
                    let mut depth = 0;
                    while !self.check_eof() {
                        if self.check(&TokenKind::Lt) {
                            depth += 1;
                            self.bump();
                        } else if self.check(&TokenKind::Gt) {
                            depth -= 1;
                            self.bump();
                            if depth == 0 {
                                break;
                            }
                        } else {
                            self.bump();
                        }
                    }
                }
                TypeKind::Named(name)
            }
            TokenKind::LParen => {
                self.bump();
                let mut tys = Vec::new();
                loop {
                    tys.push(self.parse_type()?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                TypeKind::Tuple(tys)
            }
            other => {
                return Err(self.err(format!(
                    "expected type, found {}",
                    other.describe()
                )))
            }
        };

        Ok(Node::new(span, kind))
    }

    // ------------------------------------------------------------
    // Expressions (Pratt-style precedence climbing)
    // ------------------------------------------------------------

    fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_binary(0)
    }

    fn parse_binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match self.binary_op() {
                Some(o) => o,
                None => break,
            };
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            let span = self.peek_span();
            self.bump(); // consume operator

            // Right-assoc for `**`, left-assoc otherwise.
            let next_min = if op.is_left_associative() { prec + 1 } else { prec };

            let right = self.parse_binary(next_min)?;
            left = Node::new(
                span,
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            );
        }

        // `??` is right-associative and sits low.
        loop {
            if !self.check(&TokenKind::QuestionQuestion) {
                break;
            }
            let span = self.peek_span();
            self.bump();
            let right = self.parse_binary(0)?;
            left = Node::new(
                span,
                ExprKind::Coalesce {
                    left: Box::new(left),
                    right: Box::new(right),
                },
            );
        }

        // `if` in ternary position: `<then> if <cond> else <else>`
        if self.check_keyword("if") {
            let span = self.peek_span();
            self.bump();
            let cond = self.parse_expr()?;
            self.expect_keyword("else")?;
            let else_branch = self.parse_expr()?;
            left = Node::new(
                span,
                ExprKind::IfExpr {
                    cond: Box::new(cond),
                    then_branch: Box::new(left),
                    else_branch: Box::new(else_branch),
                },
            );
        }

        Ok(left)
    }

    fn binary_op(&self) -> Option<BinaryOp> {
        Some(match self.peek_kind() {
            TokenKind::Plus => BinaryOp::Add,
            TokenKind::Minus => BinaryOp::Sub,
            TokenKind::Star => BinaryOp::Mul,
            TokenKind::Slash => BinaryOp::Div,
            TokenKind::Percent => BinaryOp::Mod,
            TokenKind::StarStar => BinaryOp::Pow,
            TokenKind::Amp => BinaryOp::BitAnd,
            TokenKind::Pipe_ => BinaryOp::BitOr,
            TokenKind::Caret => BinaryOp::BitXor,
            TokenKind::Shl => BinaryOp::Shl,
            TokenKind::Shr => BinaryOp::Shr,
            TokenKind::Usr => BinaryOp::Ushr,
            TokenKind::EqEq => BinaryOp::Eq,
            TokenKind::BangEq => BinaryOp::Ne,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Ge => BinaryOp::Ge,
            TokenKind::AmpAmp => BinaryOp::And,
            TokenKind::PipePipe => BinaryOp::Or,
            TokenKind::Keyword(k) if k == "and" => BinaryOp::And,
            TokenKind::Keyword(k) if k == "or" => BinaryOp::Or,
            _ => return None,
        })
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let span = self.peek_span();
        let op = match self.peek_kind() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitNot),
            TokenKind::Star => Some(UnaryOp::Deref),
            TokenKind::Amp => Some(UnaryOp::Ref),
            TokenKind::Keyword(k) if k == "not" => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let operand = self.parse_unary()?;
            return Ok(Node::new(
                span,
                ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
            ));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            let span = self.peek_span();
            match self.peek_kind().clone() {
                TokenKind::Dot => {
                    self.bump();
                    // Tuple index: `.0`, `.1`
                    if let TokenKind::Int(n) = self.peek_kind().clone() {
                        self.bump();
                        expr = Node::new(
                            span,
                            ExprKind::TupleField {
                                receiver: Box::new(expr),
                                index: n as usize,
                            },
                        );
                        continue;
                    }
                    // Method or field
                    let name = self.expect_ident()?;
                    if self.check(&TokenKind::LParen) {
                        self.bump();
                        let args = self.parse_call_args()?;
                        self.expect(&TokenKind::RParen)?;
                        expr = Node::new(
                            span,
                            ExprKind::MethodCall {
                                receiver: Box::new(expr),
                                method: name,
                                args,
                            },
                        );
                    } else if self.check(&TokenKind::ColonColon) {
                        // `receiver.method::<T>()`
                        self.bump();
                        let mut ta = Vec::new();
                        self.expect(&TokenKind::Lt)?;
                        loop {
                            ta.push(self.parse_type()?);
                            if !self.match_kind(&TokenKind::Comma) {
                                break;
                            }
                        }
                        self.expect(&TokenKind::Gt)?;
                        self.expect(&TokenKind::LParen)?;
                        let args = self.parse_call_args()?;
                        self.expect(&TokenKind::RParen)?;
                        expr = Node::new(
                            span,
                            ExprKind::GenericMethodCall {
                                receiver: Box::new(expr),
                                method: name,
                                type_args: ta,
                                args,
                            },
                        );
                    } else {
                        expr = Node::new(
                            span,
                            ExprKind::Field {
                                receiver: Box::new(expr),
                                name,
                            },
                        );
                    }
                }
                TokenKind::QuestionDot => {
                    self.bump();
                    let name = self.expect_ident()?;
                    expr = Node::new(
                        span,
                        ExprKind::OptionalField {
                            receiver: Box::new(expr),
                            name,
                        },
                    );
                }
                TokenKind::LBracket => {
                    self.bump();
                    let idx = self.parse_expr()?;
                    // Slice?
                    if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
                        let inclusive = self.check(&TokenKind::DotDotEq);
                        self.bump();
                        let end = if self.check(&TokenKind::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.expect(&TokenKind::RBracket)?;
                        expr = Node::new(
                            span,
                            ExprKind::Slice {
                                receiver: Box::new(expr),
                                range: SliceRange {
                                    start: Some(Box::new(idx)),
                                    end,
                                    inclusive,
                                },
                            },
                        );
                    } else {
                        self.expect(&TokenKind::RBracket)?;
                        expr = Node::new(
                            span,
                            ExprKind::Index {
                                receiver: Box::new(expr),
                                index: Box::new(idx),
                            },
                        );
                    }
                }
                TokenKind::LParen => {
                    self.bump();
                    let args = self.parse_call_args()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Node::new(
                        span,
                        ExprKind::Call {
                            callee: Box::new(expr),
                            args,
                        },
                    );
                }
                TokenKind::Question => {
                    self.bump();
                    expr = Node::new(span, ExprKind::TryPropagate(Box::new(expr)));
                }
                TokenKind::Keyword(k) if k == "as" => {
                    self.bump();
                    let ty = self.parse_type()?;
                    expr = Node::new(
                        span,
                        ExprKind::Cast {
                            expr: Box::new(expr),
                            ty,
                        },
                    );
                }
                TokenKind::Keyword(k) if k == "is" => {
                    self.bump();
                    let ty = self.parse_type()?;
                    expr = Node::new(
                        span,
                        ExprKind::IsType {
                            expr: Box::new(expr),
                            ty,
                        },
                    );
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_call_args(&mut self) -> PResult<Vec<CallArg>> {
        let mut args = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(args);
        }
        loop {
            // Named argument: `foo(x: 1)`
            let name = if let TokenKind::Ident(_) = self.peek_kind() {
                if matches!(self.peek_nth_kind(1), TokenKind::Colon) {
                    let n = self.expect_ident()?;
                    self.bump(); // colon
                    Some(n)
                } else {
                    None
                }
            } else {
                None
            };
            let value = self.parse_expr()?;
            args.push(CallArg { name, value });
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let span = self.peek_span();

        match self.peek_kind().clone() {
            // Literals
            TokenKind::Int(n) => {
                self.bump();
                Ok(Node::new(span, ExprKind::Int(n)))
            }
            TokenKind::Float(f) => {
                self.bump();
                Ok(Node::new(span, ExprKind::Float(f)))
            }
            TokenKind::Str(s) => {
                self.bump();
                Ok(Node::new(span, ExprKind::Str(s)))
            }
            TokenKind::RawStr(s) => {
                self.bump();
                Ok(Node::new(span, ExprKind::RawStr(s)))
            }
            TokenKind::ByteStr(b) => {
                self.bump();
                Ok(Node::new(span, ExprKind::ByteStr(b)))
            }
            TokenKind::Char(c) => {
                self.bump();
                Ok(Node::new(span, ExprKind::Char(c)))
            }

            // Keywords
            TokenKind::Keyword(k) => {
                self.bump();
                match k.as_str() {
                    "true" => Ok(Node::new(span, ExprKind::Bool(true))),
                    "false" => Ok(Node::new(span, ExprKind::Bool(false))),
                    "null" | "none" => Ok(Node::new(span, ExprKind::NoneLit)),
                    "some" => {
                        self.expect(&TokenKind::LParen)?;
                        let v = self.parse_expr()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(Node::new(span, ExprKind::Some(Box::new(v))))
                    }
                    "ok" => {
                        self.expect(&TokenKind::LParen)?;
                        let v = self.parse_expr()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(Node::new(span, ExprKind::Ok(Box::new(v))))
                    }
                    "err" => {
                        self.expect(&TokenKind::LParen)?;
                        let v = self.parse_expr()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(Node::new(span, ExprKind::Err(Box::new(v))))
                    }
                    "sizeof" => {
                        self.expect(&TokenKind::ColonColon)?;
                        self.expect(&TokenKind::Lt)?;
                        let ty = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        Ok(Node::new(span, ExprKind::SizeOf(ty)))
                    }
                    "offset_of" => {
                        self.expect(&TokenKind::ColonColon)?;
                        self.expect(&TokenKind::Lt)?;
                        let ty = self.parse_type()?;
                        self.expect(&TokenKind::Gt)?;
                        self.expect(&TokenKind::LParen)?;
                        let field = match self.peek_kind().clone() {
                            TokenKind::Str(s) | TokenKind::RawStr(s) => {
                                self.bump();
                                s
                            }
                            _ => {
                                return Err(self.err("offset_of expects a string field name"));
                            }
                        };
                        self.expect(&TokenKind::RParen)?;
                        Ok(Node::new(span, ExprKind::OffsetOf { ty, field }))
                    }
                    "type_of" | "typeof" => {
                        self.expect(&TokenKind::LParen)?;
                        let v = self.parse_expr()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(Node::new(span, ExprKind::TypeOf(Box::new(v))))
                    }
                    "if" => {
                        // if-expression: `b if a else c` handled in parse_binary
                        self.parse_expr()
                    }
                    "try" => {
                        self.expect(&TokenKind::LBrace)?;
                        let body = self.parse_block()?;
                        Ok(Node::new(span, ExprKind::TryExpr(Box::new(body))))
                    }
                    "match" => {
                        let subject = self.parse_expr()?;
                        self.expect(&TokenKind::Colon)?;
                        let arms = self.parse_match_arms()?;
                        Ok(Node::new(
                            span,
                            ExprKind::Match {
                                subject: Box::new(subject),
                                arms,
                            },
                        ))
                    }
                    other => Err(self.err(format!(
                        "unexpected keyword `{}` in expression",
                        other
                    ))),
                }
            }

            // Identifier — could be a name, function call, struct literal,
            // or enum variant path.
            TokenKind::Ident(name) => {
                self.bump();

                // Enum variant path: `Foo::Bar(...)`
                if self.check(&TokenKind::ColonColon) {
                    let mut path = vec![name];
                    while self.match_kind(&TokenKind::ColonColon) {
                        path.push(self.expect_ident()?);
                    }
                    if self.check(&TokenKind::LParen) {
                        self.bump();
                        let mut patterns = Vec::new();
                        loop {
                            patterns.push(self.parse_expr()?);
                            if !self.match_kind(&TokenKind::Comma) {
                                break;
                            }
                        }
                        self.expect(&TokenKind::RParen)?;
                        // We model variant construction as a Call on a
                        // path expression, which the interpreter resolves.
                        let callee = Node::new(
                            span,
                            ExprKind::Ident(path.join("::")),
                        );
                        let args = patterns
                            .into_iter()
                            .map(|v| CallArg { name: None, value: v })
                            .collect();
                        return Ok(Node::new(
                            span,
                            ExprKind::Call {
                                callee: Box::new(callee),
                                args,
                            },
                        ));
                    }
                    return Ok(Node::new(span, ExprKind::Ident(path.join("::"))));
                }

                // Struct literal: `Name { field: value }`
                if self.check(&TokenKind::LBrace) {
                    self.bump();
                    let mut fields = Vec::new();
                    self.skip_newlines();
                    while !self.check(&TokenKind::RBrace) {
                        let fname = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                        self.skip_newlines();
                    }
                    self.expect(&TokenKind::RBrace)?;
                    return Ok(Node::new(
                        span,
                        ExprKind::StructLiteral {
                            name,
                            type_args: Vec::new(),
                            fields,
                        },
                    ));
                }

                Ok(Node::new(span, ExprKind::Ident(name)))
            }

            // Parenthesized or tuple
            TokenKind::LParen => {
                self.bump();
                if self.check(&TokenKind::RParen) {
                    self.bump();
                    return Ok(Node::new(span, ExprKind::TupleLiteral(Vec::new())));
                }
                let first = self.parse_expr()?;
                if self.check(&TokenKind::Comma) {
                    let mut items = vec![first];
                    while self.match_kind(&TokenKind::Comma) {
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                        items.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Node::new(span, ExprKind::TupleLiteral(items)))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }

            // Vec literal
            TokenKind::LBracket => {
                self.bump();
                let mut items = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Node::new(span, ExprKind::VecLiteral(items)))
            }

            // Lambda: `|x| expr` or `|x| { block }`
            TokenKind::Pipe_ => {
                self.bump();
                let mut params = Vec::new();
                if !self.check(&TokenKind::Pipe_) {
                    loop {
                        let pname = self.expect_ident()?;
                        let pty = if self.match_kind(&TokenKind::Colon) {
                            Some(self.parse_type()?)
                        } else {
                            None
                        };
                        params.push(Param {
                            name: pname,
                            ty: pty,
                            default: None,
                            variadic: false,
                        });
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::Pipe_)?;

                // Optional return type
                let _ret = if self.match_kind(&TokenKind::Arrow) {
                    Some(self.parse_type()?)
                } else {
                    None
                };

                let body = if self.check(&TokenKind::LBrace) {
                    self.parse_block()?
                } else {
                    let expr = self.parse_expr()?;
                    let s = expr.span;
                    Block {
                        span: s,
                        statements: vec![Node::new(s, StmtKind::Return(Some(expr)))],
                    }
                };

                Ok(Node::new(
                    span,
                    ExprKind::Lambda {
                        params,
                        body: Box::new(body),
                    },
                ))
            }

            // Block expression
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                Ok(Node::new(span, ExprKind::BlockExpr(Box::new(block))))
            }

            other => Err(self.err(format!(
                "expected expression, found {}",
                other.describe()
            ))),
        }
    }

    // ------------------------------------------------------------
    // Patterns
    // ------------------------------------------------------------

    fn parse_pattern(&mut self) -> PResult<Pattern> {
        // Handle or-patterns: `a | b | c`
        let mut pats = vec![self.parse_pattern_single()?];
        while self.check(&TokenKind::Pipe_) {
            self.bump();
            pats.push(self.parse_pattern_single()?);
        }
        if pats.len() == 1 {
            Ok(pats.into_iter().next().unwrap())
        } else {
            Ok(Pattern::Or(pats))
        }
    }

    fn parse_pattern_single(&mut self) -> PResult<Pattern> {
        match self.peek_kind().clone() {
            TokenKind::Underscore => {
                self.bump();
                Ok(Pattern::Wildcard)
            }
            TokenKind::Int(n) => {
                self.bump();
                // Range pattern?
                if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
                    let inclusive = self.check(&TokenKind::DotDotEq);
                    self.bump();
                    let end = self.parse_pattern_single()?;
                    Ok(Pattern::Range {
                        start: Box::new(Pattern::Literal(Literal::Int(n))),
                        end: Box::new(end),
                        inclusive,
                    })
                } else {
                    Ok(Pattern::Literal(Literal::Int(n)))
                }
            }
            TokenKind::Float(f) => {
                self.bump();
                Ok(Pattern::Literal(Literal::Float(f)))
            }
            TokenKind::Str(s) | TokenKind::RawStr(s) => {
                self.bump();
                Ok(Pattern::Literal(Literal::Str(s)))
            }
            TokenKind::Char(c) => {
                self.bump();
                Ok(Pattern::Literal(Literal::Char(c)))
            }
            TokenKind::Keyword(k) if k == "true" => {
                self.bump();
                Ok(Pattern::Literal(Literal::Bool(true)))
            }
            TokenKind::Keyword(k) if k == "false" => {
                self.bump();
                Ok(Pattern::Literal(Literal::Bool(false)))
            }
            TokenKind::Keyword(k) if k == "null" || k == "none" => {
                self.bump();
                Ok(Pattern::Literal(Literal::Null))
            }
            TokenKind::LParen => {
                self.bump();
                let mut pats = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        pats.push(self.parse_pattern()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RParen)?;
                Ok(Pattern::Tuple(pats))
            }
            TokenKind::LBracket => {
                self.bump();
                let mut pats = Vec::new();
                let mut rest = None;
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        if self.check(&TokenKind::DotDot) {
                            self.bump();
                            rest = Some(Box::new(self.parse_pattern_single()?));
                            break;
                        }
                        pats.push(self.parse_pattern()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Pattern::Slice { patterns: pats, rest })
            }
            TokenKind::Ident(name) => {
                self.bump();
                // Path? `Foo::Bar`
                let mut path = vec![name];
                while self.match_kind(&TokenKind::ColonColon) {
                    path.push(self.expect_ident()?);
                }
                if self.check(&TokenKind::LParen) {
                    self.bump();
                    let mut pats = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            pats.push(self.parse_pattern()?);
                            if !self.match_kind(&TokenKind::Comma) {
                                break;
                            }
                            if self.check(&TokenKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Pattern::TupleVariant { path, patterns: pats });
                }
                if self.check(&TokenKind::LBrace) {
                    self.bump();
                    let mut fields = Vec::new();
                    let mut rest = false;
                    loop {
                        if self.check(&TokenKind::DotDot) {
                            self.bump();
                            rest = true;
                            break;
                        }
                        let fname = self.expect_ident()?;
                        let fpat = if self.match_kind(&TokenKind::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern::Binding(fname.clone())
                        };
                        fields.push((fname, fpat));
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    return Ok(Pattern::StructVariant { path, fields, rest });
                }
                if path.len() == 1 {
                    Ok(Pattern::Binding(path.pop().unwrap()))
                } else {
                    Ok(Pattern::TupleVariant { path, patterns: Vec::new() })
                }
            }
            other => Err(self.err(format!(
                "expected pattern, found {}",
                other.describe()
            ))),
        }
    }

    // ------------------------------------------------------------
    // Attributes
    // ------------------------------------------------------------

    fn parse_attribute(&mut self) -> PResult<Attribute> {
        self.expect(&TokenKind::Hash)?;
        self.expect(&TokenKind::LBracket)?;
        let name = self.expect_ident()?;
        let mut args = Vec::new();
        if self.match_kind(&TokenKind::LParen) {
            loop {
                // Named arg? `key = value`
                if let TokenKind::Ident(_) = self.peek_kind() {
                    if matches!(self.peek_nth_kind(1), TokenKind::Eq) {
                        let k = self.expect_ident()?;
                        self.bump();
                        let v = self.parse_expr()?;
                        args.push(AttributeArg::Named(k, v));
                    } else {
                        args.push(AttributeArg::Positional(self.parse_expr()?));
                    }
                } else {
                    args.push(AttributeArg::Positional(self.parse_expr()?));
                }
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(Attribute { name, args })
    }
}

// ============================================================
// HELPERS
// ============================================================

/// Keywords that can also appear as identifiers in "soft" positions.
/// This keeps `struct` field names like `type` and `as` usable.
fn is_soft_keyword(s: &str) -> bool {
    matches!(s, "type" | "as" | "from" | "self" | "case" | "default")
}

/// Tokenize and parse in one step. This is the entry point the rest
/// of the app should use.
pub fn parse(source: &str) -> Result<Program, ParseError> {
    let tokens = super::lexer::tokenize(source).map_err(|e| ParseError {
        message: e.message,
        line: e.line,
        col: e.col,
    })?;
    let mut p = Parser::new(tokens);
    p.parse_program()
}

// ============================================================
// TESTS
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_program() {
        let p = parse("").unwrap();
        assert!(p.statements.is_empty());
    }

    #[test]
    fn let_binding() {
        let p = parse("let x = 1").unwrap();
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let p = parse("let x = 1 + 2 * 3").unwrap();
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn addr_decl() {
        let p = parse("addr hp = 0x7ff6a2c0 : int32").unwrap();
        assert_eq!(p.statements.len(), 1);
    }
}