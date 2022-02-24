use backtrace::Frame;
use findshlibs::SharedLibrary;
use frames::UnresolvedFrames;
use libc::c_int;
use nix::sys::signal;
use parking_lot::RwLock;
use smallvec::SmallVec;

mod frames;

/// Define the MAX supported stack depth. TODO: make this variable mutable.
pub const MAX_DEPTH: usize = 32;

/// Define the MAX supported thread name length. TODO: make this variable mutable.
pub const MAX_THREAD_NAME: usize = 16;

lazy_static::lazy_static! {
    pub(crate) static ref PROFILER: RwLock<Result<Profiler, Error>> = RwLock::new(Profiler::new());
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NixError(#[from] nix::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("create profiler error")]
    CreatingError,
    #[error("start running cpu profiler error")]
    Running,
    #[error("stop running cpu profiler error")]
    NotRunning,
}

pub struct Profiler {
    sample_counter: i32,

    running: bool,

    #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
    blocklist_segments: Vec<(usize, usize)>,
}

impl Profiler {
    fn new() -> Result<Self, Error> {
        Ok(Profiler {
            sample_counter: 0,
            running: false,

            #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
            blocklist_segments: Vec::new(),
        })
    }

    #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn is_blocklisted(&self, addr: usize) -> bool {
        for libs in &self.blocklist_segments {
            if addr > libs.0 && addr < libs.1 {
                return true;
            }
        }
        false
    }
}

impl Profiler {
    pub fn start(&mut self) -> Result<(), Error> {
        log::info!("starting cpu profiler");
        if self.running {
            Err(Error::Running)
        } else {
            self.register_signal_handler()?;
            self.running = true;

            Ok(())
        }
    }

    fn init(&mut self) -> Result<(), Error> {
        self.sample_counter = 0;
        self.running = false;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        log::info!("stopping cpu profiler");
        if self.running {
            self.unregister_signal_handler()?;
            self.init()?;

            Ok(())
        } else {
            Err(Error::NotRunning)
        }
    }

    fn register_signal_handler(&self) -> Result<(), Error> {
        dbg!();
        let handler = signal::SigHandler::SigAction(perf_signal_handler);
        let sigaction = signal::SigAction::new(
            handler,
            signal::SaFlags::SA_SIGINFO,
            signal::SigSet::empty(),
        );
        unsafe { signal::sigaction(signal::SIGPROF, &sigaction) }?;

        Ok(())
    }

    fn unregister_signal_handler(&self) -> Result<(), Error> {
        let handler = signal::SigHandler::SigIgn;
        unsafe { signal::signal(signal::SIGPROF, handler) }?;

        Ok(())
    }

    // This function has to be AS-safe
    pub fn sample(
        &mut self,
        backtrace: SmallVec<[Frame; MAX_DEPTH]>,
        thread_name: &[u8],
        thread_id: u64,
    ) {
        dbg!();
        let frames = UnresolvedFrames::new(backtrace, thread_name, thread_id);
        self.sample_counter += 1;

        println!("{frames:?}")
        // if let Ok(()) = self.data.add(frames, 1) {}
    }
}

#[derive(Clone, Debug)]
pub struct ProfilerGuardBuilder {
    #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
    blocklist_segments: Vec<(usize, usize)>,
}

impl Default for ProfilerGuardBuilder {
    fn default() -> ProfilerGuardBuilder {
        ProfilerGuardBuilder {
            #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
            blocklist_segments: Vec::new(),
        }
    }
}

impl ProfilerGuardBuilder {
    #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn blocklist<T: AsRef<str>>(self, blocklist: &[T]) -> Self {
        use findshlibs::{Segment, TargetSharedLibrary};

        let blocklist_segments = {
            let mut segments = Vec::new();
            TargetSharedLibrary::each(|shlib| {
                let in_blocklist = match shlib.name().to_str() {
                    Some(name) => {
                        let mut in_blocklist = false;
                        for blocked_name in blocklist.iter() {
                            if name.contains(blocked_name.as_ref()) {
                                in_blocklist = true;
                            }
                        }

                        in_blocklist
                    }

                    None => false,
                };
                if in_blocklist {
                    for seg in shlib.segments() {
                        let avam = seg.actual_virtual_memory_address(shlib);
                        let start = avam.0;
                        let end = start + seg.len();
                        segments.push((start, end));
                    }
                }
            });
            segments
        };

        Self {
            blocklist_segments,
            ..self
        }
    }

    pub fn start(self) -> Result<(), Error> {
        trigger_lazy();

        match PROFILER.write().as_mut() {
            Err(err) => {
                log::error!("Error in creating profiler: {}", err);
                Err(Error::CreatingError)
            }
            Ok(profiler) => {
                #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
                {
                    profiler.blocklist_segments = self.blocklist_segments;
                }

                profiler.start()?;
                Ok(())
            }
        }
    }
}

#[no_mangle]
#[cfg_attr(
    not(all(any(target_arch = "x86_64", target_arch = "aarch64"))),
    allow(unused_variables)
)]
extern "C" fn perf_signal_handler(
    _signal: c_int,
    _siginfo: *mut libc::siginfo_t,
    ucontext: *mut libc::c_void,
) {
    dbg!();
    if let Some(mut guard) = PROFILER.try_write() {
        if let Ok(profiler) = guard.as_mut() {
            #[cfg(all(any(target_arch = "x86_64", target_arch = "aarch64")))]
            if !ucontext.is_null() {
                let ucontext: *mut libc::ucontext_t = ucontext as *mut libc::ucontext_t;

                #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
                let addr =
                    unsafe { (*ucontext).uc_mcontext.gregs[libc::REG_RIP as usize] as usize };

                if profiler.is_blocklisted(addr) {
                    return;
                }
            }

            let mut bt: SmallVec<[Frame; MAX_DEPTH]> = SmallVec::with_capacity(MAX_DEPTH);
            let mut index = 0;

            unsafe {
                backtrace::trace_unsynchronized(|frame| {
                    if index < MAX_DEPTH {
                        bt.push(frame.clone());
                        index += 1;
                        true
                    } else {
                        false
                    }
                });
            }

            let current_thread = unsafe { libc::pthread_self() };
            let mut name = [0; MAX_THREAD_NAME];
            let name_ptr = &mut name as *mut [libc::c_char] as *mut libc::c_char;

            write_thread_name(current_thread, &mut name);

            let name = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
            println!("***********");
            profiler.sample(bt, name.to_bytes(), current_thread as u64);
        }
    }
}

fn trigger_lazy() {
    let _ = backtrace::Backtrace::new();
    let _ = PROFILER.read();
}

fn write_thread_name_fallback(current_thread: libc::pthread_t, name: &mut [libc::c_char]) {
    let mut len = 0;
    let mut base = 1;

    while current_thread as u128 > base && len < MAX_THREAD_NAME {
        base *= 10;
        len += 1;
    }

    let mut index = 0;
    while index < len && base > 1 {
        base /= 10;

        name[index] = match (48 + (current_thread as u128 / base) % 10).try_into() {
            Ok(digit) => digit,
            Err(_) => {
                log::error!("fail to convert thread_id to string");
                0
            }
        };

        index += 1;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_thread_name(current_thread: libc::pthread_t, name: &mut [libc::c_char]) {
    let name_ptr = name as *mut [libc::c_char] as *mut libc::c_char;
    let ret = unsafe { libc::pthread_getname_np(current_thread, name_ptr, MAX_THREAD_NAME) };

    if ret != 0 {
        write_thread_name_fallback(current_thread, name);
    }
}
