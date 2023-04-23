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
    resume_esp: Address,
    stack_space: Option<StackSpace>,
}

impl Context {
    pub fn new(func: impl FnOnce()) -> Context {
        assert!(cfg!(all(target_arch = "x86", not(windows))));
        const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 8;
        let size = DEFAULT_STACK_SIZE;
        let func = Box::new(Box::new(func) as Box<dyn FnOnce()>);
        let func = Box::into_raw(func);
        let stack_space = unsafe { StackSpace::new(size) };
        // we use jmp to goto coro_stub
        let stack_top = unsafe { stack_space.address.offset(size as isize - 4) };
        unsafe {
            (stack_top as *mut usize).write(func as _);
        }
        Context {
            resume_addr: coro_stub as _,
            resume_esp: stack_top as _,
            stack_space: Some(stack_space),
        }
    }
}

// context of main thread
static mut MAIN_CTX: Context = Context {
    resume_addr: 0,
    resume_esp: 0,
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
extern "cdecl" {
    fn coro_stub();
    fn swap_context(current: *mut Context, next: *mut Context, val: usize) -> usize;
}

unsafe extern "C" fn call_rust_fn(func: *mut Box<dyn FnOnce()>) {
    let func = Box::from_raw(func);
    func()
}

// coro_stub
// assume when start, function ptr is in (%esp)
global_asm!(
    ".global {0}",
    "{0}:",
    "pop eax",
    "sub esp, 16",
    "mov [esp], eax",
    "call {call_rust_fn}", // call_rust_fn(...)
    "push 1",
    "lea ecx, {MAIN_CTX}",
    "push ecx",
    "lea ecx, {CURRENT_CORO_CTX}",
    "push [ecx]",
    "call {swap_context}", // swap_context(CURRENT_CORO_CTX, &mut MAIN_CTX, 1)
    sym coro_stub,
    call_rust_fn = sym call_rust_fn,
    swap_context = sym swap_context,
    MAIN_CTX = sym MAIN_CTX,
    CURRENT_CORO_CTX = sym CURRENT_CORO_CTX,
);

// swap_context
// current: 4(%esp)
// next: 8(%esp)
// val: 12(%esp)
// -> %eax
global_asm!(
    ".global {0}",
    "{0}:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push edi",
    "push esi",
    "mov eax, [ebp + 8]",               // current
    "mov [eax + 4], esp",               // current.resume_esp = %esp
    "lea ecx, co_ret_addr",
    "mov [eax], ecx",                   // current.resume_addr = &&co_ret_addr
    "mov ecx, [ebp + 12]",              // next
    "mov esp, [ecx + 4]",               // %esp = next.resume_esp
    "mov eax, [ebp + 16]",              // %eax = val
    "jmp [ecx]",                        // goto next.resume_addr
    "co_ret_addr:",
    "pop esi",
    "pop edi",
    "pop ebx",
    "pop ebp",
    "ret",                              // return %rax
    sym swap_context,
);
