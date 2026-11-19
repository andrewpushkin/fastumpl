//! QA-11: layout time does not grow with how far apart boxes are.

use std::time::Instant;

use umpl_draw::Diagram;

#[test]
fn boxes_far_apart_lay_out_quickly() {
    // The farthest positions a file may hold.
    let doc = umpl_text::parse(
        "class A\nclass B\nA --> B\nB --> A : back\nlayout {\n  A at -1000000, 0\n  B at 1000000, 1000000\n}\n",
    )
    .doc;
    let start = Instant::now();
    let d = Diagram::arrange(&doc);
    let took = start.elapsed();
    assert_eq!(d.layout.routes.len(), 2);
    assert!(took.as_millis() < 500, "took {took:?}");
}
