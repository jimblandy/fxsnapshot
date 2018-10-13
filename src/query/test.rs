#![cfg(test)]

use super::QueryParser;

#[test]
fn parse_query() {
    QueryParser::new().parse("root")
        .expect("parse failed");
    QueryParser::new().parse("nodes { id: 0x0123456789abcdef }")
        .expect("parse failed");
}
