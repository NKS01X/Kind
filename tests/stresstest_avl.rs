use kind::avl::Node;
use std::time::Instant;

#[test]
fn stress_test_insertion_and_inorder() {
    let mut root: Option<Box<Node>> = None;

    let start_insert = Instant::now();
    for i in 1..10_000_001 {
        root = Node::insert(root, i);
    }
    let duration_insert = start_insert.elapsed().as_millis();
    println!("Inserted 10,000,000 nodes in {} ms", duration_insert);

    let start_inorder = Instant::now();
    let _i = Node::inorder(&root);
    let duration_inorder = start_inorder.elapsed().as_millis();
    println!("Inorder traversal of 10,000,000 nodes done in {} ms", duration_inorder);
}
