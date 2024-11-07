//! Run queue and task scheduling implementation
//!
//! This module implements the run queue mechanism for task scheduling, including:
//! - Task run queue management
//! - Scheduling primitives 
//! - Task state transitions
//! - CPU scheduling decisions
//!
//! # Core Features
//! - Task enqueuing and dequeuing
//! - Priority-based scheduling
//! - Task yielding and preemption
//! - Timer-based scheduling events

#![no_std]

use alloc::boxed::Box;
use alloc::sync::Arc;
use taskctx::CtxRef;
use crate::run_queue::RUN_QUEUE;
use spinbase::SpinNoIrq;
use taskctx::{Tid, SchedInfo};

#[macro_use]
extern crate log;
extern crate alloc;

mod run_queue;
pub use run_queue::AxRunQueue;

/// Initializes the run queue and scheduling system
pub fn init(cpu_id: usize, dtb_pa: usize) {
    axconfig::init_once!();

    taskctx::init(cpu_id, dtb_pa);

    let idle = taskctx::init_thread();
    RUN_QUEUE.init_by(AxRunQueue::new(idle));
}

/// Returns the run queue associated with a task
pub fn task_rq(_task: &CtxRef) -> &SpinNoIrq<AxRunQueue> {
    &RUN_QUEUE
}

/// Voluntarily yields the current task's execution time
pub fn yield_now() {
    let ctx = taskctx::current_ctx();
    let rq = task_rq(&ctx);
    rq.lock().resched(false);
}

/// Forces unlock of the run queue lock
pub fn force_unlock() {
    unsafe { RUN_QUEUE.force_unlock() }
}

/// Creates and enqueues a new task with a closure
pub fn spawn_task_raw<F>(tid: Tid, f: F) -> Arc<SchedInfo>
where
    F: FnOnce() + 'static
{
    let entry: Option<*mut dyn FnOnce()> = Some(Box::into_raw(Box::new(f)));
    Arc::new(spawn_task(tid, entry))
}

/// Creates a new task with the specified entry point
pub fn spawn_task(tid: Tid, entry: Option<*mut dyn FnOnce()>) -> SchedInfo {
    let mut sched_info = SchedInfo::new();
    sched_info.init_tid(tid);
    sched_info.entry = entry;
    let sp = sched_info.pt_regs_addr();
    sched_info.thread.get_mut().init(task_entry as usize, sp.into(), 0.into());
    sched_info
}

/// Handles periodic timer ticks for the task manager.
///
/// For example, advance scheduler states, checks timed events, etc.
pub fn on_timer_tick() {
    debug!("timer tick ...");
    RUN_QUEUE.lock().scheduler_timer_tick();
}

// Todo: We should move task_entry to taskctx.
// Now schedule_tail: 'run_queue::force_unlock();` hinders us.
// Consider to move it to sched first!
pub extern "C" fn task_entry() -> ! {
    info!("################ task_entry ...");
    // schedule_tail
    // unlock runqueue for freshly created task
    force_unlock();

    let ctx = taskctx::current_ctx();
    if ctx.set_child_tid != 0 {
        let ctid_ptr = ctx.set_child_tid as *mut usize;
        unsafe { (*ctid_ptr) = ctx.tid(); }
    }

    if let Some(entry) = ctx.entry {
        unsafe { Box::from_raw(entry)() };
    }

    let sp = taskctx::current_ctx().pt_regs_addr();
    axhal::arch::ret_from_fork(sp);
    unimplemented!("task_entry!");
}
