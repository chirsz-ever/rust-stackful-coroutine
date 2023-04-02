use rand::Rng;

mod coroutine;

use coroutine::Coroutine;

fn func(index: i32, tag: i32) {
    for i in 0..4 {
        println!("thread {} in func, tag: {}, count: {}", index, tag, i);
        coroutine::yield_now();
    }
}

fn main() {
    let mut threads = Vec::new();

    for index in 0..3 {
        threads.push(Coroutine::new(move || {
            let tag = rand::thread_rng().gen_range(0..100);
            for i in 0..3 {
                println!("thread {}, tag: {}, count: {}", index, tag, i);
                coroutine::yield_now();
            }
        }));
    }
    threads.push(Coroutine::new(|| {
        let tag = rand::thread_rng().gen_range(0..100);
        func(3, tag);
    }));

    threads.push(Coroutine::new(|| {
        for _ in 0..4 {
            println!("-----");
            coroutine::yield_now();
        }
    }));

    coroutine::schedule(&mut threads);
}
