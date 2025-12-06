mod avl;
mod debug;

use avl::Node;
use debug::debug_tree;

fn main() {
    let mut root: Option<Box<Node>> = None;
    root = Node::insert(root, 10);
    root = Node::insert(root, 20);
    root = Node::insert(root, 5);
    root = Node::insert(root, 4);
    root = Node::insert(root, 15);

    println!("Inorder traversal:");
    Node::inorder(&root);
    println!("\nDebug tree:");
    debug_tree(&root, String::from(""), true);
}
