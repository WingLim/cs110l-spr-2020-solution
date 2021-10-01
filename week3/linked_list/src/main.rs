use linked_list::LinkedList;
pub mod linked_list;

fn main() {
    let mut list: LinkedList<String> = LinkedList::new();
    assert!(list.is_empty());
    assert_eq!(list.get_size(), 0);
    for i in vec!["a", "b", "c", "d"] {
        list.push_front(i.to_string());
    }

    // test generics
    println!("===== test generics =====");
    println!("{}", list);
    println!("list size: {}", list.get_size());
    println!("top element: {}", list.pop_front().unwrap());
    println!("{}", list);
    println!("size: {}", list.get_size());
    println!("");

    // test Clone trait
    println!("===== test clone trait =====");
    let mut list_clone = list.clone();
    list_clone.push_front("e".to_string());
    println!("original: {}", list);
    println!("cloned: {}", list_clone);
    println!("");

    // test Iterator
    println!("===== test iterator trait =====");
    for val in &list {
       println!("{}", val);
    }
    println!("");

    // test PartialEq trait
    println!("===== test clone trait =====");
    list.push_front("e".to_string());
    println!("orginal == cloned: {}", list == list_clone);
    
}
