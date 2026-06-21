#import <Foundation/Foundation.h>
#import <stdlib.h>
#import <string.h>

static BOOL NexShellImeDebugEnabled(void) {
    static BOOL initialized = NO;
    static BOOL enabled = NO;

    if (!initialized) {
        const char *value = getenv("NEXSHELL_IME_SHIM_LOG");
        enabled = value != NULL && value[0] != '\0' && strcmp(value, "0") != 0;
        initialized = YES;
    }

    return enabled;
}

#define NEXSHELL_IME_LOG(fmt, ...)                                             \
    do {                                                                       \
        if (NexShellImeDebugEnabled()) {                                       \
            NSLog(@"[nexshell-ime-shim][objc] " fmt, ##__VA_ARGS__);          \
        }                                                                      \
    } while (0)

void nexshell_install_warp_ime_shims(void) {
    static BOOL installed = NO;
    if (installed) return;

    installed = YES;
    NEXSHELL_IME_LOG(@"using upstream WarpHostView key/IME handling");
}
