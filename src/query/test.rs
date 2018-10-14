#![cfg(test)]

use super::test_utils::*;
use super::QueryParser;

#[test]
fn parse_query() {
    assert_eq!(QueryParser::new().parse("root").expect("parse failed"),
               root());
    assert_eq!(QueryParser::new().parse("nodes { id: 0x0123456789abcdef }")
               .expect("parse failed"),
               filter(nodes(), and1(field("id", expr_pred(number(0x0123456789abcdef))))));
}
