pub mod store;
use kind::avl::Node;

fn create_tree(){
    let inorder: Vec<i32> = store::get_val();
    let root : Option<Box<Node>> = None;
    for (i,&val) in inorder.iter().enumerate() {
        root = avl::insert(root,val);
    }
}