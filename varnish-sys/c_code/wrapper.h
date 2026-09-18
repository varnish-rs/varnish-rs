// VARNISH_RS_FULL_ABI is passed by build.rs (as a clang -D flag) exactly when the `full`
// Cargo feature is on, so this file's branch always matches the active feature.
#ifdef VARNISH_RS_FULL_ABI

#include <sys/socket.h>
#include <sys/types.h>
#include <string.h>

#include "cache/cache_varnishd.h"
#include "cache/cache_backend.h"
#include "cache/cache_director.h"
#include "cache/cache_filter.h"
#include "vsb.h"
#include "vsa.h"
#include "vapi/vsm.h"
#include "vapi/vsc.h"

struct vfp_entry *VFP_Push(struct vfp_ctx *, const struct vfp *);

#else

// vrt.h's VRT_VSC_Alloc/Allocv prototypes are only visible under #ifdef va_start, so
// stdarg.h must be included first even though it's not itself part of the vrt header set.
// (Under VARNISH_RS_FULL_ABI, cache.h already includes stdarg.h itself.)
#include <stdarg.h>

#include "vdef.h"
#include "vrt.h"

#endif

// vrt_obj.h has no include guard of its own and doesn't include vrt.h, so it must come after
// vrt.h is visible (guaranteed by both branches above) — but it's otherwise identical either
// way, so it only needs to be listed once here. vcl.h similarly needs vrt.h first, and *does*
// have a guard (so it must not be included twice) — same reasoning applies.
#include "vrt_obj.h"
#include "vcl.h"
#include "vmod_abi.h"
