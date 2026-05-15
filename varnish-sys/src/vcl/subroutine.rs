pub mod bitmask {
    pub use crate::ffi::{
        VCL_MET_BACKEND_ERROR as BACKEND_ERROR, VCL_MET_BACKEND_FETCH as BACKEND_FETCH,
        VCL_MET_BACKEND_REFRESH as BACKEND_REFRESH, VCL_MET_BACKEND_RESPONSE as BACKEND_RESPONSE,
        VCL_MET_DELIVER as DELIVER, VCL_MET_FINI as FINI, VCL_MET_HASH as HASH, VCL_MET_HIT as HIT,
        VCL_MET_INIT as INIT, VCL_MET_MISS as MISS, VCL_MET_PASS as PASS, VCL_MET_PIPE as PIPE,
        VCL_MET_PURGE as PURGE, VCL_MET_RECV as RECV, VCL_MET_SYNTH as SYNTH,
        VCL_MET_TASK_B as BACKEND, VCL_MET_TASK_C as CLIENT, VCL_MET_TASK_H as HOUSEKEEPING,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id {
    Recv,
    Pipe,
    Pass,
    Hash,
    Purge,
    Miss,
    Hit,
    Deliver,
    Synth,
    BackendFetch,
    BackendRefresh,
    BackendResponse,
    BackendError,
    Init,
    Fini,
}

impl Id {
    pub fn to_bitfield(self) -> u32 {
        use bitmask::*;
        match self {
            Id::Recv => RECV,
            Id::Pipe => PIPE,
            Id::Pass => PASS,
            Id::Hash => HASH,
            Id::Purge => PURGE,
            Id::Miss => MISS,
            Id::Hit => HIT,
            Id::Deliver => DELIVER,
            Id::Synth => SYNTH,
            Id::BackendFetch => BACKEND_FETCH,
            Id::BackendRefresh => BACKEND_REFRESH,
            Id::BackendResponse => BACKEND_RESPONSE,
            Id::BackendError => BACKEND_ERROR,
            Id::Init => INIT,
            Id::Fini => FINI,
        }
    }

    pub fn from_bitfield(val: u32) -> Option<Self> {
        use Id::*;
        [
            Recv,
            Pipe,
            Pass,
            Hash,
            Purge,
            Miss,
            Hit,
            Deliver,
            Synth,
            BackendFetch,
            BackendRefresh,
            BackendResponse,
            BackendError,
            Init,
            Fini,
        ]
        .into_iter()
        .find(|id| id.to_bitfield() == val)
    }

    pub fn is_client(self) -> bool {
        self.to_bitfield() & bitmask::CLIENT != 0
    }

    pub fn is_backend(self) -> bool {
        self.to_bitfield() & bitmask::BACKEND != 0
    }

    pub fn is_housekeeping(self) -> bool {
        self.to_bitfield() & bitmask::HOUSEKEEPING != 0
    }

    pub fn matches(self, mask: u32) -> bool {
        self.to_bitfield() & mask != 0
    }
}
