use core::arch::global_asm;

extern crate alloc;

use super::Coroutine;

type Address = usize;

struct StackSpace {
    pub address: *mut u8,
    layout: core::alloc::Layout,
}

impl StackSpace {
    // size: szie in byte
    pub unsafe fn new(size: usize) -> Self {
        let layout = core::alloc::Layout::from_size_align(size, 16).unwrap();
        let address = alloc::alloc::alloc(layout);
        StackSpace { address, layout }
    }
}

impl Drop for StackSpace {
    fn drop(&mut self) {
        unsafe {
            alloc::alloc::dealloc(self.address, self.layout);
        }
    }
}

#[repr(C)]
pub struct Context {
    resume_addr: Address,
    resume_rsp: Address,
    stack_space: Option<StackSpace>,
}

impl Context {
    pub fn new(func: impl FnOnce()) -> Context {
        assert!(cfg!(all(target_arch = "x86_64", not(windows))));
        const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 8;
        let size = DEFAULT_STACK_SIZE;
        let func = Box::new(Box::new(func) as Box<dyn FnOnce()>);
        let func = Box::into_raw(func);
        let stack_space = unsafe { StackSpace::new(size) };
        // we use jmp to goto coro_stub
        let stack_top = unsafe { stack_space.address.offset(size as isize - 8) };
        unsafe {
            (stack_top as *mut usize).write(func as _);
        }
        Context {
            resume_addr: coro_stub as _,
            resume_rsp: stack_top as _,
            stack_space: Some(stack_space),
        }
    }
}

// context of main thread
static mut MAIN_CTX: Context = Context {
    resume_addr: 0,
    resume_rsp: 0,
    stack_space: None,
};

static mut CURRENT_CORO_CTX: *mut Context = core::ptr::null_mut();

pub unsafe fn resume_coroutine(coro: &mut Coroutine, val: usize) -> usize {
    CURRENT_CORO_CTX = &mut coro.context;
    swap_context(&mut MAIN_CTX, CURRENT_CORO_CTX, val)
}

pub unsafe fn return_from_coroutine(ret: usize) -> usize {
    swap_context(CURRENT_CORO_CTX, &mut MAIN_CTX, ret)
}

#[allow(improper_ctypes)]
extern "sysv64" {
    fn coro_stub();
    fn swap_context(current: *mut Context, next: *mut Context, val: usize) -> usize;
}

unsafe extern "C" fn call_rust_fn(func: *mut Box<dyn FnOnce()>) {
    let func = Box::from_raw(func);
    func()
}

// coro_stub
// assume when start, function ptr is in (%rsp)
global_asm!(
    ".global {0}",
    "{0}:",
    "mov rdi, [rsp]",
    "add rsp, 8",
    "call {call_rust_fn}", // call_rust_fn(*%rsp)
    "lea rdi, [rip + {CURRENT_CORO_CTX}]",
    "mov rdi, [rdi]",
    "lea rsi, [rip + {MAIN_CTX}]",
    "mov rdx, 1",
    "call {swap_context}", // swap_context(CURRENT_CORO_CTX, &mut MAIN_CTX, 1)
    sym coro_stub,
    call_rust_fn = sym call_rust_fn,
    swap_context = sym swap_context,
    MAIN_CTX = sym MAIN_CTX,
    CURRENT_CORO_CTX = sym CURRENT_CORO_CTX,
);

// swap_context
// current: %rdi
// next: %rsi
// val: %rdx
// -> %rax
global_asm!(
    ".global {0}",
    "{0}:",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "push rbx",
    "push rbp",
    "mov [rdi + 8], rsp",               // current.resume_rsp = %rsp
    "lea rax, [rip + co_ret_addr]",
    "mov [rdi], rax",                   // current.resume_addr = &&co_ret_addr
    "mov rsp, [rsi + 8]",               // %rsp = next.resume_rsp
    "mov rax, rdx",                     // %rax = val
    "jmp [rsi]",                          // goto next.resume_addr
    "co_ret_addr:",
    "pop rbp",
    "pop rbx",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "ret",                              // return %rax
    sym swap_context,
);
