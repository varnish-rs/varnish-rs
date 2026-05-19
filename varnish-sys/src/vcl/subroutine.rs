macro_rules! define_subroutines {
    (
        subs {
            $( ($sub_name:literal, $enum_id:ident, $bitmask_name:ident) )*
        }
        groups {
            $( ($group_name:literal, $group_bitmask:ident, $group_ffi:ident, $is_method:ident) )*
        }
    ) => {
        /// Valid scope names for the `#[restrict(...)]` attribute. Used by varnish-macros to validate inputs.
        pub const VALID_RESTRICT_SCOPES: &[&str] = &[
            $( $sub_name, )*
            $( $group_name, )*
        ];

        pub mod bitmask {
            ::paste::paste! {
                pub use crate::ffi::{
                    $( [<VCL_MET_ $bitmask_name>] as $bitmask_name, )*
                    $( $group_ffi as $group_bitmask, )*
                };
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Id {
            $( $enum_id, )*
        }

        /// Maps a `#[restrict(...)]` scope name to the exact bitmask constant identifier.
        /// Used by varnish-macros codegen to avoid fragile string-manipulation heuristics.
        pub fn bitmask_const_name(scope: &str) -> Option<&'static str> {
            match scope {
                $( $sub_name => Some(stringify!($bitmask_name)), )*
                $( $group_name => Some(stringify!($group_bitmask)), )*
                _ => None,
            }
        }

        impl Id {
            pub fn to_bitfield(self) -> u32 {
                use bitmask::*;
                match self {
                    $( Id::$enum_id => $bitmask_name, )*
                }
            }

            pub fn from_bitfield(val: u32) -> Option<Self> {
                [$( Id::$enum_id, )*]
                    .into_iter()
                    .find(|id| id.to_bitfield() == val)
            }

            $(
                pub fn $is_method(self) -> bool {
                    self.to_bitfield() & bitmask::$group_bitmask != 0
                }
            )*

            pub fn matches(self, mask: u32) -> bool {
                self.to_bitfield() & mask != 0
            }
        }
    };
}

define_subroutines! {
    subs {
        ("vcl_recv",             Recv,            RECV)
        ("vcl_pipe",             Pipe,            PIPE)
        ("vcl_pass",             Pass,            PASS)
        ("vcl_hash",             Hash,            HASH)
        ("vcl_purge",            Purge,           PURGE)
        ("vcl_miss",             Miss,            MISS)
        ("vcl_hit",              Hit,             HIT)
        ("vcl_deliver",          Deliver,         DELIVER)
        ("vcl_synth",            Synth,           SYNTH)
        ("vcl_backend_fetch",    BackendFetch,    BACKEND_FETCH)
        ("vcl_backend_refresh",  BackendRefresh,  BACKEND_REFRESH)
        ("vcl_backend_response", BackendResponse, BACKEND_RESPONSE)
        ("vcl_backend_error",    BackendError,    BACKEND_ERROR)
        ("vcl_init",             Init,            INIT)
        ("vcl_fini",             Fini,            FINI)
    }
    groups {
        ("client",       CLIENT,       VCL_MET_TASK_C, is_client)
        ("backend",      BACKEND,      VCL_MET_TASK_B, is_backend)
        ("housekeeping", HOUSEKEEPING, VCL_MET_TASK_H, is_housekeeping)
    }
}
