use std::cmp::max;

#[derive(Clone, Debug)]
pub struct Node {
    pub id: i32,
    pub height: i32,
    pub left: Option<Box<Node>>,
    pub right: Option<Box<Node>>,
}

impl Node {
    pub fn new(val: i32) -> Box<Node> {
        Box::new(Node {
            id: val,
            height: 1,
            left: None,
            right: None,
        })
    }

    pub fn height(node: &Option<Box<Node>>) -> i32 {
        match node {
            Some(n) => n.height,
            None => 0,
        }
    }

    fn update_height(n: &mut Box<Node>) {
        n.height = 1 + max(Self::height(&n.left), Self::height(&n.right));
    }

    fn balance_factor(n: &Box<Node>) -> i32 {
        Self::height(&n.left) - Self::height(&n.right)
    }

    pub fn rotate_right(mut y: Box<Node>) -> Box<Node> {
        let mut x = y.left.take().unwrap();
        y.left = x.right.take();
        Self::update_height(&mut y);
        Self::update_height(&mut x);
        x.right = Some(y);
        x
    }

    pub fn rotate_left(mut x: Box<Node>) -> Box<Node> {
        let mut y = x.right.take().unwrap();
        x.right = y.left.take();
        Self::update_height(&mut x);
        Self::update_height(&mut y);
        y.left = Some(x);
        y
    }

    pub fn insert(node: Option<Box<Node>>, id: i32) -> Option<Box<Node>> {
        if node.is_none() {
            return Some(Self::new(id));
        }

        let mut n = node.unwrap();

        if id < n.id {
            n.left = Self::insert(n.left.take(), id);
        } else if id > n.id {
            n.right = Self::insert(n.right.take(), id);
        } else {
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

    pub fn inorder(node: &Option<Box<Node>>)->Vec<i32> {

        let mut v: Vec<i32> = Vec::new(); 
        if let Some(n) = node {
            Self::inorder(&n.left);
            //print!("{} ", n.id);
            v.push(n.id);
            Self::inorder(&n.right);
        }
        v
    }
}
