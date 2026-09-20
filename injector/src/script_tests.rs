// Test harness for the SigilScript interpreter.
// Runs via `cargo test` from the binary.

#[test]
fn value_int_roundtrip() {
    use crate::core::script::value::{value_to_bytes, bytes_to_value, Value};
    use crate::core::script::ast::TypeKind;

    let v = Value::Int(12345);
    let bytes = value_to_bytes(&v, &TypeKind::Int32).unwrap();
    assert_eq!(bytes, vec![0x39, 0x30, 0, 0]);
    let back = bytes_to_value(&bytes, &TypeKind::Int32).unwrap();
    assert_eq!(back, Value::Int(12345));
}

#[test]
fn lexer_basics() {
    use crate::core::script::lexer::{tokenize, TokenKind};
    let toks = tokenize("let x = 1").unwrap();
    assert!(matches!(toks[0].kind, TokenKind::Keyword(_)));
    assert!(matches!(toks[1].kind, TokenKind::Ident(_)));
}

#[test]
fn parser_smoke() {
    use crate::core::script::parser::parse;
    let p = parse("let x = 1 + 2 * 3").unwrap();
    assert_eq!(p.statements.len(), 1);
}