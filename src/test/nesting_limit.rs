use crate::{config::DEFAULT_NESTING_LIMIT, from_str, Error, SerdeXml};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct XmlNode {
    #[allow(dead_code)]
    a: Option<Box<XmlNode>>,
}

/// Build `<a>...</a>` nested `depth` levels deep.
fn nested(depth: usize) -> String {
    "<a>".repeat(depth) + &"</a>".repeat(depth)
}

#[test]
fn nesting_at_default_limit_is_ok() {
    // The innermost node has no child, so `depth` elements means
    // `depth - 1` descents past the root. Pick the deepest input that
    // stays within the default limit.
    let input = nested(DEFAULT_NESTING_LIMIT);
    let result: Result<XmlNode, _> = from_str(&input);
    assert!(
        result.is_ok(),
        "input nested to the default limit should deserialize, got {result:?}"
    );
}

#[test]
fn deeply_nested_input_errors_instead_of_overflowing_the_stack() {
    // Without the depth guard this input overflows the stack and aborts the
    // process. With the guard it must return a clean `Error::TooDeep`.
    let input = nested(50_000);
    let result: Result<XmlNode, _> = from_str(&input);
    match result {
        Err(Error::TooDeep(limit)) => assert_eq!(limit, DEFAULT_NESTING_LIMIT),
        other => panic!("expected Error::TooDeep, got {other:?}"),
    }
}

#[test]
fn custom_nesting_limit_is_respected() {
    // A tiny limit rejects input that the default would accept.
    let input = nested(10);
    let result: Result<XmlNode, _> = SerdeXml::new().nesting_limit(3).from_str(&input);
    match result {
        Err(Error::TooDeep(limit)) => assert_eq!(limit, 3),
        other => panic!("expected Error::TooDeep(3), got {other:?}"),
    }
}

#[test]
fn raised_nesting_limit_allows_deeper_input() {
    // Input that exceeds the default limit succeeds once the limit is raised.
    let depth = DEFAULT_NESTING_LIMIT + 50;
    let input = nested(depth);
    let result: Result<XmlNode, _> = SerdeXml::new().nesting_limit(depth + 1).from_str(&input);
    assert!(
        result.is_ok(),
        "raising the limit should accept deeper input, got {result:?}"
    );
}
