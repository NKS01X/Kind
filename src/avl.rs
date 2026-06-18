use std::cmp::max;

#[derive(Clone, Debug)]
pub struct Node<T> {
    pub id: T,
    pub height: i32,
    pub left: Option<Box<Node<T>>>,
    pub right: Option<Box<Node<T>>>,
}

pub struct AVLTree<T> {
    pub root: Option<Box<Node<T>>>,
}

impl<T: Ord + Clone> AVLTree<T> {
    pub fn new() -> Self {
        AVLTree { root: None }
    }

    fn create_node(val: T) -> Box<Node<T>> {
        Box::new(Node {
            id: val,
            height: 1,
            left: None,
            right: None,
        })
    }

    pub fn insert(&mut self, id: T) {
        self.root = Self::recursive_insert(self.root.take(), id);
    }

    fn recursive_insert(node: Option<Box<Node<T>>>, id: T) -> Option<Box<Node<T>>> {
        let mut n = match node {
            None => return Some(Self::create_node(id)),
            Some(n) => n,
        };

        if id < n.id {
            n.left = Self::recursive_insert(n.left.take(), id.clone());
        } else if id > n.id {
            n.right = Self::recursive_insert(n.right.take(), id.clone());
        } else {
            n.id = id;
            return Some(n);
        }

        Self::update_height(&mut n);
        let balance = Self::balance_factor(&n);

        if balance > 1 && id < n.left.as_ref().unwrap().id {
            return Some(Self::rotate_right(n));
        }
        if balance < -1 && id > n.right.as_ref().unwrap().id {
            return Some(Self::rotate_left(n));
        }
        if balance > 1 && id > n.left.as_ref().unwrap().id {
            n.left = Some(Self::rotate_left(n.left.take().unwrap()));
            return Some(Self::rotate_right(n));
        }
        if balance < -1 && id < n.right.as_ref().unwrap().id {
            n.right = Some(Self::rotate_right(n.right.take().unwrap()));
            return Some(Self::rotate_left(n));
        }

        Some(n)
    }

    // --- Helper Methods ---

    fn get_height(node: &Option<Box<Node<T>>>) -> i32 {
        node.as_ref().map_or(0, |n| n.height)
    }

    fn update_height(n: &mut Box<Node<T>>) {
        n.height = 1 + max(Self::get_height(&n.left), Self::get_height(&n.right));
    }

    fn balance_factor(n: &Box<Node<T>>) -> i32 {
        Self::get_height(&n.left) - Self::get_height(&n.right)
    }

    fn rotate_right(mut y: Box<Node<T>>) -> Box<Node<T>> {
        let mut x = y.left.take().expect("Left child missing");
        y.left = x.right.take();
        Self::update_height(&mut y);
        Self::update_height(&mut x);
        x.right = Some(y);
        x
    }

    fn rotate_left(mut x: Box<Node<T>>) -> Box<Node<T>> {
        let mut y = x.right.take().expect("Right child missing");
        x.right = y.left.take();
        Self::update_height(&mut x);
        Self::update_height(&mut y);
        y.left = Some(x);
        y
    }

    pub fn get(&self, target: &T) -> Option<&T> {
        let mut current = &self.root;
        while let Some(node) = current {
            if target < &node.id {
                current = &node.left;
            } else if target > &node.id {
                current = &node.right;
            } else {
                return Some(&node.id);
            }
        }
        None
    }

    pub fn delete(&mut self, target: &T) {
        self.root = Self::recursive_delete(self.root.take(), target);
    }

    fn recursive_delete(node: Option<Box<Node<T>>>, target: &T) -> Option<Box<Node<T>>> {
        let mut n = match node {
            None => return None,
            Some(n) => n,
        };

        if target < &n.id {
            n.left = Self::recursive_delete(n.left.take(), target);
        } else if target > &n.id {
            n.right = Self::recursive_delete(n.right.take(), target);
        } else {
            if n.left.is_none() {
                return n.right;
            } else if n.right.is_none() {
                return n.left;
            } else {
                let mut min_node = n.right.as_ref().unwrap();
                while let Some(left) = &min_node.left {
                    min_node = left;
                }
                n.id = min_node.id.clone();
                n.right = Self::recursive_delete(n.right.take(), &n.id);
            }
        }

        Self::update_height(&mut n);
        let balance = Self::balance_factor(&n);

        if balance > 1 {
            let left_balance = Self::balance_factor(n.left.as_ref().unwrap());
            if left_balance >= 0 {
                return Some(Self::rotate_right(n));
            } else {
                n.left = Some(Self::rotate_left(n.left.take().unwrap()));
                return Some(Self::rotate_right(n));
            }
        }
        if balance < -1 {
            let right_balance = Self::balance_factor(n.right.as_ref().unwrap());
            if right_balance <= 0 {
                return Some(Self::rotate_left(n));
            } else {
                n.right = Some(Self::rotate_right(n.right.take().unwrap()));
                return Some(Self::rotate_left(n));
            }
        }

        Some(n)
    }

    pub fn range(&self, lo: &T, hi: &T) -> Vec<T> {
        let mut result = Vec::new();
        Self::recursive_range(&self.root, lo, hi, &mut result);
        result
    }

    fn recursive_range(node: &Option<Box<Node<T>>>, lo: &T, hi: &T, result: &mut Vec<T>) {
        if let Some(n) = node {
            if &n.id >= lo {
                Self::recursive_range(&n.left, lo, hi, result);
            }
            if &n.id >= lo && &n.id <= hi {
                result.push(n.id.clone());
            }
            if &n.id <= hi {
                Self::recursive_range(&n.right, lo, hi, result);
            }
        }
    }

    pub fn get_inorder(&self) -> Vec<T> {
        let mut result = Vec::new();
        Self::recursive_inorder(&self.root, &mut result);
        result
    }

    fn recursive_inorder(node: &Option<Box<Node<T>>>, v: &mut Vec<T>) {
        if let Some(n) = node {
            Self::recursive_inorder(&n.left, v);
            v.push(n.id.clone());
            Self::recursive_inorder(&n.right, v);
        }
    }
}
