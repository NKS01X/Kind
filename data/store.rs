use std::fs::File;
use std::io::{Read, Write};
use kind::avl::Node;

fn save(inorder: Vec<i32>) {
    const PAGE_SIZE: usize = 1024;
    let page_bytes = PAGE_SIZE * std::mem::size_of::<i32>();
    let mut page: Vec<u8> = vec![0u8; page_bytes];

    for (i, &val) in inorder.iter().enumerate() {
        let offset = i * std::mem::size_of::<i32>();
        page[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
    }

    let mut file = File::create("data/page1.bin").unwrap();
    file.write_all(&page).unwrap();
}

fn get_val() -> Vec<i32> {
    const PAGE_SIZE: usize = 1024;
    let page_bytes = PAGE_SIZE * std::mem::size_of::<i32>();

    let mut page = vec![0u8; page_bytes];
    let mut file = File::open("data/page1.bin").unwrap();
    file.read_exact(&mut page).unwrap();

    let mut result = Vec::new();

    for chunk in page.chunks(4) {
        let val = i32::from_le_bytes(chunk.try_into().unwrap());
        if val == 0 {
            break;
        }
        result.push(val);
    }

    result
}
