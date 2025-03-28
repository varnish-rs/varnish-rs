#include "wrapper.h"

#ifdef VARNISH_RS_ALLOC_VARIADIC
void *
VRT_VSC_AllocVariadic(struct vsmw_cluster *vc, struct vsc_seg **sg,
    const char *nm, size_t sd,
    const unsigned char *jp, size_t sj, const char *fmt, ...)
{
	va_list ap;
	void *retval;

	va_start(ap, fmt);
	retval = VRT_VSC_Alloc(vc, sg, nm, sd, jp, sj, fmt, ap);
	va_end(ap);
	return(retval);
}
#endif
