use cs492_concur_homework::{Bst, SequentialMap};

mod map_test;

#[test]
fn bst_smoke() {
    let mut bst = map_test::Sequentialize::<_, _, Bst<String, _>>::default();
    assert!(bst.insert(&String::from("aa"), 42).is_ok());
    assert!(bst.insert(&String::from("bb"), 37).is_ok());
    assert_eq!(bst.lookup(&String::from("bb")), Some(&37));
    assert_eq!(bst.delete(&String::from("aa")), Ok(42));
    assert_eq!(bst.delete(&String::from("aa")), Err(()));
}

#[test]
fn bst_custom() {
    let mut art = map_test::Sequentialize::<_, _, Bst<String, _>>::default();
    assert!(art.insert(&String::from("ABCDE"), 1).is_ok());
    assert!(art.insert(&String::from("ABCD"), 2).is_ok());
    assert!(art.insert(&String::from("ABC"), 3).is_ok());
    assert!(art.insert(&String::from("AB"), 4).is_ok());
    assert!(art.insert(&String::from("A"), 5).is_ok());
    assert!(art.delete(&String::from("A")).is_ok());
    assert_eq!(art.lookup(&String::from("A")), None);
    
    assert_eq!(art.lookup(&String::from("AB")), Some(&4));
    assert!(art.delete(&String::from("AB")).is_ok());
    assert_eq!(art.lookup(&String::from("AB")), None);

    assert_eq!(art.lookup(&String::from("ABC")), Some(&3));
    assert!(art.delete(&String::from("ABC")).is_ok());
    assert_eq!(art.lookup(&String::from("ABC")), None);
    
    assert_eq!(art.lookup(&String::from("ABCD")), Some(&2));
    assert!(art.delete(&String::from("ABCD")).is_ok());
    assert_eq!(art.lookup(&String::from("ABCD")), None);

    assert_eq!(art.lookup(&String::from("ABCDE")), Some(&1));
    assert!(art.delete(&String::from("ABCDE")).is_ok());
    assert_eq!(art.lookup(&String::from("ABCDE")), None);
}

#[test]
fn bst_stress() {
    map_test::stress_concurrent_sequential::<String, Bst<String, usize>>();
}

#[test]
fn bst_stress_concurrent() {
    map_test::stress_concurrent::<String, Bst<String, usize>>();
}
