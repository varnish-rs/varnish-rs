#include <sys/socket.h>
#include <sys/types.h>
#include <string.h>

#include "cache/cache_varnishd.h"
#include "cache/cache_backend.h"
#include "cache/cache_director.h"
#include "vrt_obj.h"
#include "cache/cache_filter.h"
#include "vrt_obj.h"
#include "vmod_abi.h"
#include "vsb.h"
#include "vsa.h"
#include "vapi/vsm.h"
#include "vapi/vsc.h"

#include "vcl.h"

struct vfp_entry *VFP_Push(struct vfp_ctx *, const struct vfp *);

/** from vcc_interface.h */
typedef int acl_match_f(VRT_CTX, const VCL_IP);

struct vrt_acl {
    unsigned        magic;
#define VRT_ACL_MAGIC   0x78329d96
    acl_match_f     *match;
    const char    *name;
};
