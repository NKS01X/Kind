use kind::avl::Node; 

//preorder mil jayga 


//save it in the file 
fn save() {
    let mut inorder: Vec<i32> = vec![1,3,5,7,4];
    
    const bytes:u32 = 1024;
    
    // let mut page:u8 = vec![0u8,4*bytes]; //created 4 bytes of (u8) 0's

    for (i,&val) in inorder.enumerate() {
        let mut page:Vec<u8> = vec![0u8,4*bytes];
        let offset = i * 4;
        page[offset..offset+4].copy_from_slice(&val.to_le_bytes());
    }


}