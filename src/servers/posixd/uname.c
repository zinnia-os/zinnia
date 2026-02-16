#include <config.h>
#include <uapi/uname.h>

static struct utsname uname_buf = {
    .sysname = "Zinnia",
    .release = ZINNIA_VERSION,
    .version = ZINNIA_COMPILER_ID " " ZINNIA_LINKER_ID,
    .domainname = "(none)",
    .nodename = "localhost",
    .machine = ZINNIA_ARCH,
};
