use crossbeam_channel as channel;
use std::{thread, time};

fn parallel_map<T, U, F>(mut input_vec: Vec<T>, num_threads: usize, f: F) -> Vec<U>
where
    F: FnOnce(T) -> U + Send + Copy + 'static,
    T: Send + 'static,
    U: Send + 'static + Default,
{
    let mut output_vec: Vec<U> = Vec::with_capacity(input_vec.len());
    output_vec.resize_with(input_vec.len(), Default::default);
    let (in_sender, in_receiver) = channel::unbounded();
    let (out_sender, out_receiver) = channel::unbounded();
    let mut threads = Vec::new();
    
    for _ in 0..num_threads {
        let in_receiver = in_receiver.clone();
        let out_sender = out_sender.clone();
        threads.push(thread::spawn(move || {
            while let Ok(pair) = in_receiver.recv() {
                let (idx, val) = pair;
                out_sender.send((idx, f(val))).expect("Tried writing to channel, but there are no receivers");
            }
        }))
    }

    let len = input_vec.len();
    for i in 0..len {
        let idx = len - i - 1;
        let val = input_vec.pop().unwrap();
        in_sender.send((idx, val)).expect("Tried writing to channel, but there are no receivers");
    }

    drop(in_sender);
    drop(out_sender);

    while let Ok(pair) = out_receiver.recv() {
        let (idx, val) = pair;
        output_vec[idx] = val;
    }

    for handle in threads {
        handle.join().expect("Panic occurred in thread!");
    }

    output_vec
}

fn main() {
    let v = vec![6, 7, 8, 9, 10, 1, 2, 3, 4, 5, 12, 18, 11, 5, 20];
    let squares = parallel_map(v, 10, |num| {
        println!("{} squared is {}", num, num * num);
        thread::sleep(time::Duration::from_millis(500));
        num * num
    });
    println!("squares: {:?}", squares);
}
