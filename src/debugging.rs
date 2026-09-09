use crate::prelude::*;

use std::{
	future::{poll_fn, Future},
	pin::pin,
	time::{Duration, Instant},
	ffi::CStr,
	ptr::null_mut,
	collections::HashMap,
	sync::{RwLock}
};

// I don't know why my IDE doesn't break this into multiple lines
use esp_idf_svc::sys::{eTaskState, eTaskState_eBlocked, eTaskState_eDeleted, eTaskState_eInvalid, eTaskState_eReady, eTaskState_eRunning, eTaskState_eSuspended, uxTaskGetNumberOfTasks, uxTaskGetStackHighWaterMark, uxTaskGetSystemState, xTASK_STATUS, xTaskGetCurrentTaskHandle, TaskHandle_t};

use serde::Serialize;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
pub struct TaskInfo {
	#[serde(skip_serializing)]
	pub handle: TaskHandle_t,
	pub name: String,
	pub task_number: u32,
	pub state: TaskState,
	pub priority: u32,
	pub run_time_counter: u32,
	pub stack_high_water_mark: u32,
}

impl From<xTASK_STATUS> for TaskInfo {
	fn from(task: xTASK_STATUS) -> Self {
		Self {
			handle: task.xHandle,
			name: unsafe { CStr::from_ptr(task.pcTaskName) }.to_str().map_or_default(String::from),
			task_number: task.xTaskNumber,
			state: task.eCurrentState.into(),
			priority: task.uxCurrentPriority,
			run_time_counter: task.ulRunTimeCounter,
			stack_high_water_mark: task.usStackHighWaterMark,
		}
	}
}

#[derive(Debug, Serialize)]
pub enum TaskState {
	Running,
	Ready,
	Blocked,
	Suspended,
	Deleted,
	Invalid
}

impl From<eTaskState> for TaskState {
	fn from(value: eTaskState) -> Self {
		#[allow(non_upper_case_globals)]
		match value {
			eTaskState_eRunning     => Self::Running,
			eTaskState_eReady       => Self::Ready,
			eTaskState_eBlocked     => Self::Blocked,
			eTaskState_eSuspended   => Self::Suspended,
			eTaskState_eDeleted     => Self::Deleted,
			eTaskState_eInvalid | _ => Self::Invalid,
		}
	}
}

////////////////////////////////////////////////////////////////////////////////

/// Return the minimum recorded amount of stack space (in bytes) remaining for the
/// provided task handle, or the caller thread if null.
#[inline] pub fn high_water_mark(handle: TaskHandle_t) -> u32 {
	unsafe { uxTaskGetStackHighWaterMark(handle) }
}

/// Return the minimum recorded amount of stack space (in bytes) remaining for the
/// caller thread. Don't call this from an interrupt.
#[inline] pub fn high_water_mark_caller() -> u32 { high_water_mark(null_mut()) }

/// Returns a list of [xTASK_STATUS] objects, one for each existing FreeRTOS task.
///
/// This function takes a long time to call.
///
/// Note that in the extremely unlikely circumstance that a task is created between
/// the time the number of tasks is measured and the vector is populated, the
/// underlying C function will error and this function will return an empty vector.
fn get_task_status_list() -> Vec<xTASK_STATUS> {
	let num_tasks = /* not */ unsafe { uxTaskGetNumberOfTasks() };

	let mut buf = Vec::with_capacity(num_tasks as _);

	unsafe {
		let populated = uxTaskGetSystemState(buf.as_mut_ptr(), num_tasks, null_mut());
		buf.set_len(populated as _);
	}
	buf
}

pub fn get_tasks() -> impl Iterator<Item = TaskInfo> {
	get_task_status_list().into_iter().map(TaskInfo::from)
}

fn print_task_info() {
	info!("{:#?}", get_tasks().collect::<Vec<_>>().as_slice());
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Default, Debug)]
pub struct FutureStats {
	polls: u32,
	total_time: Duration,
	longest_time: Duration,
}

pub type ProfilingData = HashMap<&'static str, RwLock<FutureStats>>;

static mut PROFILING_DATA: Option<ProfilingData> = None;

/// If profiling is enabled, return a reference to the profiling data map.
pub fn get_profiling_data() -> Option<&'static mut ProfilingData> {
	unsafe { (&raw mut PROFILING_DATA).as_mut_unchecked()}.as_mut()
}

/// Enable profiling by populating the profiling data hashmap.
pub fn enable_profiling() {
	unsafe {
		if let None = PROFILING_DATA {
			PROFILING_DATA = Some(Default::default());
		}
	}
}

/// Wrap polling calls to the passed future and profile how long they take.
///
/// If the provided stats struct is None, then the provided future is transparently
/// executed and no profiling will be performed.
pub async fn profile<F: Future>(future: F, stats: Option<&mut RwLock<FutureStats>>) -> F::Output {
	if let Some(stats) = stats {
		// Profiling enabled
		let mut future = pin!(future);

		poll_fn(|cx| {
			let start = Instant::now();

			let result = future.as_mut().poll(cx);

			let elapsed = start.elapsed();

			let current = stats.get_mut().expect("Profiling stats poisoned");

			current.polls = current.polls.saturating_add(1);
			current.total_time = current.total_time.saturating_add(elapsed);
			current.longest_time = current.longest_time.max(elapsed);

			result
		}).await
	} else {
		// Profiling disabled
		future.await
	}

}

/// Wrap the passed future in a profiler and return it. If profiling is not enabled,
/// then the future will be executed normally.
#[macro_export]
macro_rules! profile {
	($name:ident($($arg:tt)*)) => {{
		let stats = crate::debugging::get_profiling_data().and_then(|map|
			map.try_insert(stringify!($name), Default::default()).ok()
		);

		crate::debugging::profile($name($($arg)*), stats)
	}};
}
