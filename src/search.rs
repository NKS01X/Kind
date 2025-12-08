use crate::avl::Node;

pub fn search_by_id<'a>(node: &'a Option<Box<Node>>, target_id: i32) -> Option<&'a Box<Node>> {
    let mut curr = node.as_ref();

    while let Some(root) = curr {
        if root.id == target_id {
            return Some(root);
        } else if target_id < root.id {
            curr = root.left.as_ref();
        } else {
            curr = root.right.as_ref();
        }
    }

    None
}

