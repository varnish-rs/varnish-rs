#include <sys/socket.h>
#include <sys/types.h>
#include <string.h>

#include "cache/cache_varnishd.h"
#include "cache/cache_backend.h"
#include "cache/cache_director.h"
#include "cache/cache_filter.h"
#include "vmod_abi.h"
#include "vsb.h"
#include "vsa.h"
#include "vapi/vsm.h"
#include "vapi/vsc.h"


struct vfp_entry *VFP_Push(struct vfp_ctx *, const struct vfp *);
