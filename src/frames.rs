use backtrace::Frame;
use smallvec::SmallVec;

use crate::{MAX_DEPTH, MAX_THREAD_NAME};

#[derive(Clone, Debug)]
pub struct UnresolvedFrames {
    pub frames: SmallVec<[Frame; MAX_DEPTH]>,
    pub thread_name: [u8; MAX_THREAD_NAME],
    pub thread_name_length: usize,
    pub thread_id: u64,
}

impl UnresolvedFrames {
    pub fn new(frames: SmallVec<[Frame; MAX_DEPTH]>, tn: &[u8], thread_id: u64) -> Self {
        let thread_name_length = tn.len();
        let mut thread_name = [0; MAX_THREAD_NAME];
        thread_name[0..thread_name_length].clone_from_slice(tn);

        Self {
            frames,
            thread_name,
            thread_name_length,
            thread_id,
        }
    }
}
