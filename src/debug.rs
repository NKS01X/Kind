use crate::avl::Node;
use std::option::Option;
use std::boxed::Box;

#[allow(dead_code)]
pub fn debug_tree<T: std::fmt::Display>(node: &Option<Box<Node<T>>>, prefix: String, is_left: bool) {
    if let Some(n) = node {
        let new_prefix = if is_left { format!("{}│   ", prefix) } else { format!("{}    ", prefix) };
        debug_tree(&n.right, new_prefix.clone(), false);
        println!("{}{}── {}(h={})", prefix, if is_left { "└" } else { "┌" }, n.id, n.height);
        debug_tree(&n.left, new_prefix, true);
    }
}
