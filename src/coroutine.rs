#[cfg_attr(target_arch = "x86", path = "coroutine/platform/i386.rs")]
#[cfg_attr(
    all(target_arch = "x86_64", windows),
    path = "coroutine/platform/win64.rs"
)]
#[cfg_attr(
    all(target_arch = "x86_64", not(windows)),
    path = "coroutine/platform/sysv64.rs"
)]
mod platform;

use std::marker::PhantomData;

use platform::{resume_coroutine, return_from_coroutine, Context};

pub struct Coroutine<'a> {
    context: Context,
    finished: bool,
    _phantom: PhantomData<&'a dyn FnOnce()>,
}

impl<'a> Coroutine<'a> {
    pub fn new(func: impl FnOnce() + 'a) -> Self {
        Coroutine {
            context: Context::new(func),
            finished: false,
            _phantom: PhantomData,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn resume(&mut self) {
        if self.is_finished() {
            return;
        }

        unsafe {
            self.finished = resume_coroutine(self) != 0;
        }
    }
}

pub fn yield_now() {
    unsafe { return_from_coroutine(0) }
}

pub fn schedule(coros: &mut [Coroutine]) {
    let mut all_finished = false;
    while !all_finished {
        all_finished = true;
        for co in coros.iter_mut() {
            if !co.is_finished() {
                all_finished = false;
                co.resume();
            }
        }
    }
}
