// Test harness. Pulls in modules that have #[cfg(test)] tests.
pub use crate::core::script::lexer;

#[test]
fn lexer_basics() {
    use crate::core::script::lexer::{tokenize, TokenKind};
    let toks = tokenize("1 2 3").unwrap();
    assert_eq!(toks[0].kind, TokenKind::Int(1));
    assert_eq!(toks[1].kind, TokenKind::Int(2));
    assert_eq!(toks[2].kind, TokenKind::Int(3));
}