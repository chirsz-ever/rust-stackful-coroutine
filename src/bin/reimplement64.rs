// reimplement stackful coroutine in https://mthli.xyz/stackful-stackless with stable rust in sysv64
// original code: https://github.com/mthli/blog/blob/master/content/blog/stackful-stackless

use rand::Rng;
use std::ptr;

const CTX_SIZE: usize = 1024;

type Ctx = *mut *mut u8;

#[cfg(all(target_arch = "x86_64", not(windows)))]
mod platform {
    use super::*;

    // sysv64:
    //      callee-saved: RBX, RSP, RBP, and R12–R15
    //      *(ctx + CTX_SIZE - 1) 存储 return address
    //      *(ctx + CTX_SIZE - 2) 存储 rbx
    //      *(ctx + CTX_SIZE - 3) 存储 rbp
    //      *(ctx + CTX_SIZE - 4) ~ *(ctx + CTX_SIZE - 7) 存储 r12 ~ r15
    //      *(ctx + CTX_SIZE - 8) 存储 rsp
    pub const SAVED_REG_COUNT: usize = 7;

    extern "sysv64" {
        pub fn swap_ctx(current: Ctx, next: Ctx);
    }

    core::arch::global_asm!(
    "
.global {0}
{0}:
    // 获取 swap_ctx 的第一个参数 char **current
    mov %rdi, %rax

    // 依次将各个寄存器的值存储到 current
    mov %rbx, -16(%rax)
    mov %rbp, -24(%rax)
    mov %r12, -32(%rax)
    mov %r13, -40(%rax)
    mov %r14, -48(%rax)
    mov %r15, -56(%rax)
    mov %rsp, -64(%rax)

    mov (%rsp), %rcx
    mov %rcx,  -8(%rax) // save return address

    // 获取 swap_ctx 的第二个参数 char **next
    mov %rsi, %rax

    // 依次将 next 存储的值写入各个寄存器
    mov -16(%rax), %rbx
    mov -24(%rax), %rbp
    mov -32(%rax), %r12
    mov -40(%rax), %r13
    mov -48(%rax), %r14
    mov -56(%rax), %r15
    mov -64(%rax), %rsp

    mov -8(%rax), %rcx
    mov %rcx,    (%rsp) // restore return address

    ret",
        sym swap_ctx,
        options(att_syntax)
    );
}

#[cfg(all(target_arch = "x86_64", windows))]
mod platform {
    use super::*;

    // win64:
    //      The registers RBX, RBP, RDI, RSI, RSP, R12, R13, R14, and R15 are considered nonvolatile (callee-saved)
    //      *(ctx + CTX_SIZE - 1) 存储 return address
    //      *(ctx + CTX_SIZE - 2) 存储 rbx
    //      *(ctx + CTX_SIZE - 3) 存储 rbp
    //      *(ctx + CTX_SIZE - 4) 存储 rdi
    //      *(ctx + CTX_SIZE - 5) 存储 rsi
    //      *(ctx + CTX_SIZE - 6) ~ *(ctx + CTX_SIZE - 9) 存储 r12 ~ r15
    //      *(ctx + CTX_SIZE - 10) 存储 rsp
    pub const SAVED_REG_COUNT: usize = 9;

    extern "win64" {
        pub fn swap_ctx(current: Ctx, next: Ctx);
    }

    core::arch::global_asm!(
        "
.global {0}
{0}:
    // 获取 swap_ctx 的第一个参数 char **current
    mov %rcx, %rax

    // 依次将各个寄存器的值存储到 current
    mov %rbx, -16(%rax)
    mov %rbp, -24(%rax)
    mov %rdi, -32(%rax)
    mov %rsi, -40(%rax)
    mov %r12, -48(%rax)
    mov %r13, -56(%rax)
    mov %r14, -64(%rax)
    mov %r15, -72(%rax)
    mov %rsp, -80(%rax)

    mov (%rsp), %r8
    mov %r8,  -8(%rax) // save return address

    // 获取 swap_ctx 的第二个参数 char **next
    mov %rdx, %rax

    // 依次将 next 存储的值写入各个寄存器
    mov -16(%rax), %rbx
    mov -24(%rax), %rbp
    mov -32(%rax), %rdi
    mov -40(%rax), %rsi
    mov -48(%rax), %r12
    mov -56(%rax), %r13
    mov -64(%rax), %r14
    mov -72(%rax), %r15
    mov -80(%rax), %rsp

    mov -8(%rax), %r8
    mov %r8,    (%rsp) // restore return address

    ret",
        sym swap_ctx,
        options(att_syntax)
    );
}

static mut MAIN_CTX: Ctx = ptr::null_mut();
static mut NEST_CTX: Ctx = ptr::null_mut();
static mut FUNC_CTX_1: Ctx = ptr::null_mut();
static mut FUNC_CTX_2: Ctx = ptr::null_mut();

// 用于模拟切换协程的上下文
static mut YIELD_COUNT: usize = 0;

const CTX_LAYOUT: std::alloc::Layout =
    unsafe { std::alloc::Layout::from_size_align_unchecked(8 * CTX_SIZE, 16) };

// 注意 x86 的栈增长方向是从高位向低位增长的，所以寻址是向下偏移的
unsafe fn init_ctx(func: fn()) -> Ctx {
    use platform::SAVED_REG_COUNT;
    // 动态申请 CTX_SIZE 内存用于存储协程上下文
    let ctx = std::alloc::alloc_zeroed(CTX_LAYOUT) as Ctx;

    // 将 func 的地址作为其栈帧 return address 的初始值，
    // 当 func 第一次被调度时，将从其入口处开始执行
    *ctx.add(CTX_SIZE - 1) = func as _;

    // 需要预留 SAVED_REG_COUNT + 1 个寄存器内容的存储空间，
    // 余下的内存空间均可以作为 func 的栈帧空间
    // 注意栈帧地址要 16 字节对齐, 从 swap_ctx 执行 ret 时会退 8 字节栈, 函数体调整 rsp 时会认为 rsp 在上一层
    // 调用时已对齐, 加上执行 call 时压栈 8 字节. 因此此处地址需要 16 字节对齐.
    static_assertions::const_assert!(SAVED_REG_COUNT + 2 < 16);
    *ctx.add(CTX_SIZE - SAVED_REG_COUNT - 1) = ctx.add(CTX_SIZE - 16) as _;
    // *ctx.add(CTX_SIZE - 2) = ctx.add(CTX_SIZE - 9) as _;
    return ctx.add(CTX_SIZE);
}

// 因为我们只有 4 个协程（其中一个是主协程），
// 所以这里简单用 switch 来模拟调度器切换上下文了
unsafe fn r#yield() {
    use platform::swap_ctx;
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
    if !cfg!(all(target_arch = "x86_64", not(windows))) {
        eprintln!("this example only support x86_64 target and sysv64 calling convention!");
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
