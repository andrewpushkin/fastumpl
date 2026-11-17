mod common;

use common::random_document;
use umpl_text::{parse, print};

/// The example file from the design document (§5.2).
const DESIGN_EXAMPLE: &str = r#"diagram "Shop"

package billing {
  abstract class Payment {
    +amount: Money
    +pay(): Receipt {abstract}
  }
  class CardPayment
}

class Order {
  +id: OrderId
  -lines: List<OrderLine>
  +total(): Money
}

interface Priced {
  +price(): Money
}

enum Status { New, Paid, Shipped }

# an order owns its lines
Order "1" *-- "1..*" OrderLine : contains
billing.CardPayment --|> billing.Payment
Order ..|> Priced
Order --> billing.Payment : paid by

layout {
  Order               at 24, 8
  OrderLine           at 24, 22
  billing.Payment     at 48, 8
  billing.CardPayment at 48, 22
}
"#;

const SHOP_EXAMPLE: &str = include_str!("../../../examples/shop.umpl");

fn assert_stable(src: &str) {
    let first = print(&parse(src).doc);
    let second = print(&parse(&first).doc);
    assert_eq!(first, second, "printing is not stable for:\n{src}");
}

#[test]
fn print_parse_print_is_stable_for_design_example() {
    assert_stable(DESIGN_EXAMPLE);
}

#[test]
fn shop_example_is_formatted_and_clean() {
    let parsed = parse(SHOP_EXAMPLE);
    assert_eq!(parsed.problems, vec![]);
    assert_eq!(print(&parsed.doc), SHOP_EXAMPLE);
}

#[test]
fn print_parse_print_is_stable_for_random_documents() {
    for seed in 0..300 {
        let doc = random_document(seed);
        let text = print(&doc);
        let parsed = parse(&text);
        assert_eq!(parsed.problems, vec![], "seed {seed}:\n{text}");
        assert_eq!(print(&parsed.doc), text, "seed {seed}");
        assert_eq!(parsed.doc.elements.len(), doc.elements.len(), "seed {seed}");
        assert_eq!(
            parsed.doc.relations.len(),
            doc.relations.len(),
            "seed {seed}"
        );
        let mut names: Vec<_> = doc.elements.keys().map(|id| doc.full_name(*id)).collect();
        let mut parsed_names: Vec<_> = parsed
            .doc
            .elements
            .keys()
            .map(|id| parsed.doc.full_name(*id))
            .collect();
        names.sort();
        parsed_names.sort();
        assert_eq!(names, parsed_names, "seed {seed}");
    }
}

#[test]
fn canonical_output_of_design_example() {
    let out = print(&parse(DESIGN_EXAMPLE).doc);
    assert!(out.starts_with("diagram \"Shop\"\n\npackage billing {\n  abstract class Payment {\n"));
    assert!(out.contains("enum Status { New, Paid, Shipped }\n"));
    assert!(out.contains("# an order owns its lines\nOrder \"1\" *-- \"1..*\" OrderLine : contains\nbilling.CardPayment --|> billing.Payment\n"));
    assert!(out.ends_with("layout {\n  Order               at 24, 8\n  OrderLine           at 24, 22\n  billing.CardPayment at 48, 22\n  billing.Payment     at 48, 8\n}\n"));
}

#[test]
fn raw_lines_survive() {
    let src = "class A\n\n# about the junk\n!!! junk line\nclass B\n";
    let out = print(&parse(src).doc);
    assert!(out.contains("# about the junk\n!!! junk line\n"), "{out}");
    assert_stable(src);
}

#[test]
fn self_relation_round_trip() {
    assert_stable("class Node\nNode \"1\" --> \"0..1 next\" Node : links\n");
}

#[test]
fn reversed_and_spaced_input_is_normalised() {
    let out = print(&parse("class A\nclass B\n  A   <|--   B\n").doc);
    assert!(out.contains("B --|> A\n"), "{out}");
}
