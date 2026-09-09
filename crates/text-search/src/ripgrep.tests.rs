use super::*;
use crate::Position;
use std::cell::Cell;
use std::sync::Arc;

fn request(query: &str) -> Request {
    Request {
        id: 42,
        query: query.to_owned(),
        documents: vec![
            Arc::new(Document::from_rows([
                "removed: needle NEEDLE needle".to_owned(),
                "a+b.c[0] -needle".to_owned(),
                String::new(),
                "prefix\nneedle inside one presented row".to_owned(),
            ])),
            Arc::new(Document::default()),
            Arc::new(Document::from_rows([
                "é needle naïve NAÏVE σ ς Σ".to_owned(),
                "embedded\0needle".to_owned(),
            ])),
        ],
    }
}

#[test]
fn inline_and_background_search_share_literal_smart_case_rules() {
    let mut engine = Engine::default();
    for (query, count) in [
        ("needle", 7),
        ("NEEDLE", 1),
        ("a+b.c[0]", 1),
        ("-needle", 1),
        ("naïve", 2),
        ("NAÏVE", 1),
        ("σ", 3),
        ("absent", 0),
        ("", 0),
    ] {
        let request = request(query);
        let actual = engine.search(&request, || false).unwrap();
        assert_eq!(actual.matches.len(), count, "query {query}");
        assert_eq!(actual, request.search(), "query {query}");
    }
}

#[test]
fn positions_follow_presented_rows_and_utf8_byte_columns() {
    let actual = Engine::default()
        .search(&request("needle"), || false)
        .unwrap();
    let expected = [
        (0, 0, 9),
        (0, 0, 16),
        (0, 0, 23),
        (0, 1, 10),
        (0, 3, 7),
        (2, 0, 3),
        (2, 1, 9),
    ]
    .map(|(document_index, row, column)| Match {
        document_index,
        position: Position { row, column },
    });
    assert_eq!(actual.matches, expected);
}

#[test]
fn bom_and_nul_bytes_are_preserved() {
    let mut request = request("needle");
    request.documents = vec![Arc::new(Document::from_rows([
        "\u{feff}needle".to_owned(),
        "\0needle\r".to_owned(),
    ]))];
    let actual = Engine::default().search(&request, || false).unwrap();
    assert_eq!(actual.matches.len(), 2);
    assert_eq!(actual.matches[0].position, Position { row: 0, column: 3 });
    assert_eq!(actual.matches[1].position, Position { row: 1, column: 1 });
    assert_eq!(actual, request.search());
    request.query = "\u{feff}".to_owned();
    assert_eq!(
        Engine::default()
            .search(&request, || false)
            .unwrap()
            .matches[0]
            .position,
        Position { row: 0, column: 0 }
    );
}

#[test]
fn newline_queries_match_within_a_presented_row_only() {
    let mut request = request("prefix\nneedle");
    request.documents = vec![Arc::new(Document::from_rows([
        "prefix".to_owned(),
        "needle".to_owned(),
        "prefix\nneedle".to_owned(),
    ]))];
    let actual = Engine::default().search(&request, || false).unwrap();
    assert_eq!(actual.matches.len(), 1);
    assert_eq!(actual.matches[0].position, Position { row: 2, column: 0 });
    assert_eq!(
        Query::new(&request.query)
            .ranges("prefix\nneedle")
            .collect::<Vec<_>>(),
        vec![0..13]
    );
}

#[test]
fn cancellation_discards_partial_matches() {
    let mut request = request("needle");
    request.documents = vec![Arc::new(Document::from_rows(["needle ".repeat(20_000)]))];
    let mut engine = Engine::default();
    assert!(engine.search(&request, || true).is_none());
    let checks = Cell::new(0);
    assert!(
        engine
            .search(&request, || {
                checks.set(checks.get() + 1);
                checks.get() >= 8
            })
            .is_none()
    );
    assert!(checks.get() < 20_000);
    assert_eq!(
        engine.search(&request, || false).unwrap().matches.len(),
        20_000
    );
}

#[test]
fn dense_search_collects_every_occurrence() {
    let mut request = request("needle");
    request.documents = vec![Arc::new(Document::from_rows(
        (0..20_000).map(|_| "needle needle".to_owned()),
    ))];
    let actual = Engine::default().search(&request, || false).unwrap();
    assert_eq!(actual.matches.len(), 40_000);
    assert_eq!(actual, request.search());
}
