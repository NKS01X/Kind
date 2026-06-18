#[cfg(test)]
mod tests {
    use crate::avl::AVLTree;
    use crate::server::DbRecord;

    #[test]
    fn test_avl() {
        let mut tree = AVLTree::new();
        tree.insert(DbRecord { key: "b".to_string(), value: vec![1] });
        tree.insert(DbRecord { key: "a".to_string(), value: vec![2] });
        tree.insert(DbRecord { key: "c".to_string(), value: vec![3] });

        assert_eq!(tree.get(&DbRecord { key: "b".to_string(), value: vec![] }).unwrap().value, vec![1]);
        
        tree.delete(&DbRecord { key: "b".to_string(), value: vec![] });
        assert!(tree.get(&DbRecord { key: "b".to_string(), value: vec![] }).is_none());

        let range = tree.range(&DbRecord { key: "a".to_string(), value: vec![] }, &DbRecord { key: "c".to_string(), value: vec![] });
        assert_eq!(range.len(), 2);
    }
}
