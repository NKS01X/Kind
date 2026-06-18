use kind::avl::AVLTree; 

pub fn run_test() {
    test1();
    test2();
    test3();
    test4();
    test5();
}

fn check_inorders(v: Vec<i32>, u: Vec<i32>) -> bool {
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

pub fn print_result(testnum: i8, passed: bool) {
    if passed {
        println!("\x1b[32m[✓] {} passed\x1b[0m", testnum);
    } else {
        println!("\x1b[31m[✗] {} failed\x1b[0m", testnum);
    }
}

#[test]
fn test1() { // ll check 
    let mut tree = AVLTree::new();
    tree.insert(30);
    tree.insert(20);
    tree.insert(10);
    
    let output = tree.get_inorder(); 
    let expected = vec![10, 20, 30];
    print_result(1, check_inorders(output.clone(), expected.clone()));
    assert_eq!(output, expected);
}

#[test]
fn test2() { // rr check
    let mut tree = AVLTree::new();
    tree.insert(10);
    tree.insert(20);
    tree.insert(30); 

    let output = tree.get_inorder(); 
    let expected = vec![10, 20, 30];
    print_result(2, check_inorders(output.clone(), expected.clone()));
    assert_eq!(output, expected);
}

#[test]
fn test3() { //lr check
    let mut tree = AVLTree::new();
    tree.insert(30);
    tree.insert(10);
    tree.insert(20); 

    let output = tree.get_inorder(); 
    let expected = vec![10, 20, 30];
    print_result(3, check_inorders(output.clone(), expected.clone()));
    assert_eq!(output, expected);
}

#[test]
fn test4() { //rl check
    let mut tree = AVLTree::new();
    tree.insert(10);
    tree.insert(30);
    tree.insert(20);

    let output = tree.get_inorder(); 
    let expected = vec![10, 20, 30];
    print_result(4, check_inorders(output.clone(), expected.clone()));
    assert_eq!(output, expected);
}

#[test]
fn test5() {
    let mut tree = AVLTree::new();
    tree.insert(10);
    tree.insert(20);
    tree.insert(10); // duplicate

    let output = tree.get_inorder(); 
    let expected = vec![10, 20];
    print_result(5, check_inorders(output.clone(), expected.clone()));
    assert_eq!(output, expected);
}
