#ifndef CODEX_BRIDGE_H
#define CODEX_BRIDGE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
uint32_t codex_abi_version(void);
uint64_t codex_initialize(const char *config);
int32_t codex_command(uint64_t handle, const char *command);
char *codex_poll_event(uint64_t handle);
void codex_string_free(char *string);
void codex_shutdown(uint64_t handle);
#ifdef __cplusplus
}
#endif
#endif
