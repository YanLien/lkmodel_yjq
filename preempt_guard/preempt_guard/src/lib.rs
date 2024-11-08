//! RAII wrappers to create a critical section with local IRQs or preemption
//! disabled, used to implement spin locks in kernel.
//!
//! The critical section is created after the guard struct is created, and is
//! ended when the guard falls out of scope.
//!
//! The crate user must implement the [`KernelGuardIf`] trait using
//! [`crate_interface::impl_interface`] to provide the low-level implementantion
//! of how to enable/disable kernel preemption, if the feature `preempt` is
//! enabled.
//!
//! Available guards:
//!
//! - [`NoOp`]: Does nothing around the critical section.
//! - [`IrqSave`]: Disables/enables local IRQs around the critical section.
//! - [`NoPreempt`]: Disables/enables kernel preemption around the critical
//!   section.
//! - [`NoPreemptIrqSave`]: Disables/enables both kernel preemption and local
//!   IRQs around the critical section.
//!
//! # Crate features
//!
//! - `preempt`: Use in the preemptive system. If this feature is enabled, you
//!    need to implement the [`KernelGuardIf`] trait in other crates. Otherwise
//!    the preemption enable/disable operations will be no-ops. This feature is
//!    disabled by default.
//!
//! # Examples
//!
//! ```
//! use kernel_guard::{KernelGuardIf, NoPreempt};
//!
//! struct KernelGuardIfImpl;
//!
//! #[crate_interface::impl_interface]
//! impl KernelGuardIf for KernelGuardIfImpl {
//!     fn enable_preempt() {
//!         // Your implementation here
//!     }
//!     fn disable_preempt() {
//!         // Your implementation here
//!     }
//! }
//!
//! let guard = NoPreempt::new();
//! /* The critical section starts here
//!
//! Do something that requires preemption to be disabled
//!
//! The critical section ends here */
//! drop(guard);
//! ```

#![no_std]
#![feature(asm_const)]

pub use kernel_guard_base::BaseGuard;
use kernel_guard_base::NoOp;

mod arch;

cfg_if::cfg_if! {
    // For user-mode std apps, we use the alias of [`NoOp`] for all guards,
    // since we can not disable IRQs or preemption in user-mode.
    if #[cfg(any(target_os = "none", doc))] {
        /// A guard that disables/enables kernel preemption around the critical
        /// section.
        pub struct NoPreempt;

        /// A guard that disables/enables both kernel preemption and local IRQs
        /// around the critical section.
        ///
        /// When entering the critical section, it disables kernel preemption
        /// first, followed by local IRQs. When leaving the critical section, it
        /// re-enables local IRQs first, followed by kernel preemption.
        pub struct NoPreemptIrqSave(usize);
    } else {
        /// Alias of [`NoOp`].
        pub type NoPreempt = NoOp;

        /// Alias of [`NoOp`].
        pub type NoPreemptIrqSave = NoOp;
    }
}

#[cfg(any(target_os = "none", doc))]
mod imp {
    use super::*;

    fn current_check_preempt_pending() {
        let curr = taskctx::current_ctx();
        if curr.get_preempt_pending() && curr.can_preempt(0) {
            let mut rq = run_queue::task_rq(&curr).lock();
            if curr.get_preempt_pending() {
                rq.preempt_resched();
            }
        }
    }

    impl BaseGuard for NoPreempt {
        type State = ();
        fn acquire() -> Self::State {
            // disable preempt
            if let Some(ctx) = taskctx::CurrentCtx::try_get() {
                ctx.disable_preempt();
            }
        }
        fn release(_state: Self::State) {
            // enable preempt
            if let Some(ctx) = taskctx::CurrentCtx::try_get() {
                if ctx.enable_preempt() {
                    // If current task is pending to be preempted, do rescheduling.
                    current_check_preempt_pending();
                }
            }
        }
    }

    impl BaseGuard for NoPreemptIrqSave {
        type State = usize;
        fn acquire() -> Self::State {
            // disable preempt
            if let Some(ctx) = taskctx::CurrentCtx::try_get() {
                ctx.disable_preempt();
            }
            // disable IRQs and save IRQ states
            super::arch::local_irq_save_and_disable()
        }
        fn release(state: Self::State) {
            // restore IRQ states
            super::arch::local_irq_restore(state);
            // enable preempt
            if let Some(ctx) = taskctx::CurrentCtx::try_get() {
                if ctx.enable_preempt() {
                    // If current task is pending to be preempted, do rescheduling.
                    current_check_preempt_pending();
                }
            }
        }
    }

    impl NoPreempt {
        /// Creates a new [`NoPreempt`] guard.
        pub fn new() -> Self {
            Self::acquire();
            Self
        }
    }

    impl Drop for NoPreempt {
        fn drop(&mut self) {
            Self::release(())
        }
    }

    impl Default for NoPreempt {
        fn default() -> Self {
            Self::new()
        }
    }

    impl NoPreemptIrqSave {
        /// Creates a new [`NoPreemptIrqSave`] guard.
        pub fn new() -> Self {
            Self(Self::acquire())
        }
    }

    impl Drop for NoPreemptIrqSave {
        fn drop(&mut self) {
            Self::release(self.0)
        }
    }

    impl Default for NoPreemptIrqSave {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub fn init(cpu_id: usize, dtb_pa: usize) {
    axconfig::init_once!();
    run_queue::init(cpu_id, dtb_pa);
}
