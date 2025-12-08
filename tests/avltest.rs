use std::time::Instant;
use kind::avl::Node; 

pub fn run_test() {
    test1();
    test2();
    test3();
    test4();
    test5();
    test6();
    test7();
    test8();
}

fn check_inorders(v: &Vec<i32>, u: &Vec<i32>) -> bool {
    if v.len() != u.len() {
        return false;
    }
    for i in 0..v.len() {
        if v[i] != u[i] {
            return false;
        }
    }
    true
}

pub fn print_result(testnum: i8, passed: bool, duration_ms: u128, output: &Vec<i32>) {
    if passed {
        println!("\x1b[32m[✓] {} passed in {} ms\x1b[0m", testnum, duration_ms);
    } else {
        println!("\x1b[31m[✗] {} failed in {} ms\x1b[0m", testnum, duration_ms);
        println!("Output: {:?}", output);
        panic!("Test {} failed", testnum);
    }
}

#[test]
fn test1() { // ll check 
    let mut root: Option<Box<Node>> = None;
    let start = Instant::now();
    root = Node::insert(root, 30);
    root = Node::insert(root, 20);
    root = Node::insert(root, 10);
    let duration = start.elapsed().as_millis();
    
    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30];
    print_result(1, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test2() { // rr check
    let mut root: Option<Box<Node>> = None;
    let start = Instant::now();
    root = Node::insert(root, 10);
    root = Node::insert(root, 20);
    root = Node::insert(root, 30); 
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30];
    print_result(2, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test3() { // lr check
    let mut root: Option<Box<Node>> = None;
    let start = Instant::now();
    root = Node::insert(root, 30);
    root = Node::insert(root, 10);
    root = Node::insert(root, 20); 
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30];
    print_result(3, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test4() { // rl check
    let mut root: Option<Box<Node>> = None;
    let start = Instant::now();
    root = Node::insert(root, 10);
    root = Node::insert(root, 30);
    root = Node::insert(root, 20);
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30];
    print_result(4, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test5() {
    let mut root: Option<Box<Node>> = None;
    let start = Instant::now();
    root = Node::insert(root, 10);
    root = Node::insert(root, 20);
    root = Node::insert(root, 10); // duplicate
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20];
    print_result(5, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test6() { // deeper LL/RR rotations
    let mut root: Option<Box<Node>> = None;
    let values = vec![50, 30, 70, 20, 40, 60, 80, 10, 25, 35, 45, 55, 65, 75, 90];
    let start = Instant::now();
    for v in values {
        root = Node::insert(root, v);
    }
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 90];
    print_result(6, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test7() { // left-heavy deep tree
    let mut root: Option<Box<Node>> = None;
    let values = vec![100, 90, 80, 70, 60, 50, 40, 30, 20, 10];
    let start = Instant::now();
    for v in values {
        root = Node::insert(root, v);
    }
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    print_result(7, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}

#[test]
fn test8() { // right-heavy deep tree
    let mut root: Option<Box<Node>> = None;
    let values = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    let start = Instant::now();
    for v in values {
        root = Node::insert(root, v);
    }
    let duration = start.elapsed().as_millis();

    let output: Vec<i32> = Node::inorder(&root); 
    let expected = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    print_result(8, check_inorders(output.as_ref(), expected.as_ref()), duration, &output);
}
