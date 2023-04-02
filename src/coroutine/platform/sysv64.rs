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
    resume_rsp: Address,
    stack_space: StackSpace,
}

impl Context {
    pub fn new(func: impl FnOnce()) -> Context {
        assert_eq!(core::mem::size_of::<usize>(), 8);
        const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 4;
        let size = DEFAULT_STACK_SIZE;
        let func = Box::new(Box::new(func) as Box<dyn FnOnce()>);
        let func = Box::into_raw(func);
        let stack_space = unsafe { StackSpace::new(size) };
        // we use `ret` to branch to `coro_stub`
        let stack_top = unsafe { stack_space.address.offset(size as isize) as *mut usize };
        unsafe {
            stack_top.offset(-1).write(coro_stub as _);
            stack_top.offset(-2).write(func as _);
            Context {
                resume_rsp: stack_top.offset(-8) as _,
                stack_space,
            }
        }
    }
}

static mut MAIN_RSP: Address = 0;
static mut CURRENT_CTX: *mut Context = core::ptr::null_mut();

pub unsafe fn resume_coroutine(coro: &mut Coroutine) -> usize {
    CURRENT_CTX = &mut coro.context;
    swap_ctx(&mut MAIN_RSP, coro.context.resume_rsp, 0)
}

pub unsafe extern "C" fn return_from_coroutine(value: usize) {
    swap_ctx(&mut (*CURRENT_CTX).resume_rsp, MAIN_RSP, value);
}

extern "C" {
    fn coro_stub();

    fn swap_ctx(current_rsp: *mut Address, next_rsp: Address, value: usize) -> usize;
}

unsafe extern "C" fn call_rust_fn(func: *mut Box<dyn FnOnce()>) {
    let func = Box::from_raw(func);
    func()
}

// coro_stub
// assume when start, function ptr is in rbp
global_asm!(
    ".global {0}",
    "{0}:",
    "mov rdi, rbp",
    "mov rbp, rsp",
    "call {call_rust_fn}",
    "mov rdi, 1",
    "call {return_from_coroutine}", // return_from_coroutine(1)
    sym coro_stub,
    call_rust_fn = sym call_rust_fn,
    return_from_coroutine = sym return_from_coroutine,
);

// swap_ctx
// %rdi: current_rsp
// %rsi: next_rsp
// %rdx: value
global_asm!(
    ".global {0}",
    "{0}:",
    "push rbp",
    "mov rbp, rsp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "push rbx",
    "push rdx",
    "mov [rdi], rsp",                   // *current_rsp = %rsp
    "mov rsp, rsi",                     // %rsp = next_rsp
    "pop rdx",
    "pop rbx",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "ret",                         // return %rax
    sym swap_ctx,
);
