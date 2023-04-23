// reimplement stackful coroutine in https://mthli.xyz/stackful-stackless with stable rust
// original code: https://github.com/mthli/blog/blob/master/content/blog/stackful-stackless

use rand::Rng;
use std::{arch::global_asm, ptr};

// source: https://github.com/mthli/blog/blob/master/content/blog/stackful-stackless/stackful.s
global_asm!(include_str!("stackful.s"), sym swap_ctx, options(att_syntax));

const CTX_SIZE: usize = 1024;

type Ctx = *mut *mut u8;

// *(ctx + CTX_SIZE - 1) 存储 return address
// *(ctx + CTX_SIZE - 2) 存储 ebx
// *(ctx + CTX_SIZE - 3) 存储 edi
// *(ctx + CTX_SIZE - 4) 存储 esi
// *(ctx + CTX_SIZE - 5) 存储 ebp
// *(ctx + CTX_SIZE - 6) 存储 esp
static mut MAIN_CTX: Ctx = ptr::null_mut();
static mut NEST_CTX: Ctx = ptr::null_mut();
static mut FUNC_CTX_1: Ctx = ptr::null_mut();
static mut FUNC_CTX_2: Ctx = ptr::null_mut();

// 用于模拟切换协程的上下文
static mut YIELD_COUNT: usize = 0;

// 切换上下文，具体参见 stackful.s 的注释
extern "cdecl" {
    fn swap_ctx(current: Ctx, next: Ctx);
}

const CTX_LAYOUT: std::alloc::Layout = std::alloc::Layout::new::<[*mut u8; CTX_SIZE]>();

// 注意 x86 的栈增长方向是从高位向低位增长的，所以寻址是向下偏移的
unsafe fn init_ctx(func: fn()) -> Ctx {
    // 动态申请 CTX_SIZE 内存用于存储协程上下文
    let ctx = std::alloc::alloc_zeroed(CTX_LAYOUT) as Ctx;

    // 将 func 的地址作为其栈帧 return address 的初始值，
    // 当 func 第一次被调度时，将从其入口处开始执行
    *ctx.add(CTX_SIZE - 1) = func as _;

    // https://github.com/mthli/blog/pull/12
    // 需要预留 6 个寄存器内容的存储空间，
    // 余下的内存空间均可以作为 func 的栈帧空间
    *ctx.add(CTX_SIZE - 6) = ctx.add(CTX_SIZE - 7) as _;
    return ctx.add(CTX_SIZE);
}

// 因为我们只有 4 个协程（其中一个是主协程），
// 所以这里简单用 switch 来模拟调度器切换上下文了
unsafe fn r#yield() {
    let current_yiled_count = YIELD_COUNT;
    YIELD_COUNT += 1;
    match current_yiled_count % 4 {
        0 => swap_ctx(MAIN_CTX, NEST_CTX),

        1 => swap_ctx(NEST_CTX, FUNC_CTX_1),
        2 => swap_ctx(FUNC_CTX_1, FUNC_CTX_2),
        3 => swap_ctx(FUNC_CTX_2, MAIN_CTX),
        _ => unreachable!(),
    };
}

fn nest_yield() {
    unsafe {
        r#yield();
    }
}

fn nest() {
    // 随机生成一个整数作为 tag
    let tag = rand::thread_rng().gen_range(0..100);
    for i in 0..3 {
        println!("nest, tag: {}, index: {}", tag, i);
        nest_yield();
    }
}

fn func() {
    // 随机生成一个整数作为 tag
    let tag = rand::thread_rng().gen_range(0..100);
    for i in 0..3 {
        println!("func, tag: {}, index: {}", tag, i);
        unsafe {
            r#yield();
        }
    }
}

fn main() {
    if !cfg!(target_arch = "x86") {
        eprintln!("this example only support 32bit x86 target!");
        return;
    }
    unsafe {
        MAIN_CTX = init_ctx(main);

        // 证明 nest() 可以在其嵌套函数中被挂起
        NEST_CTX = init_ctx(nest);

        // 证明同一个函数在不同的栈帧空间上运行
        FUNC_CTX_1 = init_ctx(func);
        FUNC_CTX_2 = init_ctx(func);

        let tag = rand::thread_rng().gen_range(0..100);
        for i in 0..3 {
            println!("main, tag: {}, index: {}", tag, i);
            r#yield();
        }

        std::alloc::dealloc(MAIN_CTX.sub(CTX_SIZE) as _, CTX_LAYOUT);
        std::alloc::dealloc(NEST_CTX.sub(CTX_SIZE) as _, CTX_LAYOUT);
        std::alloc::dealloc(FUNC_CTX_1.sub(CTX_SIZE) as _, CTX_LAYOUT);
        std::alloc::dealloc(FUNC_CTX_2.sub(CTX_SIZE) as _, CTX_LAYOUT);
    }
}
