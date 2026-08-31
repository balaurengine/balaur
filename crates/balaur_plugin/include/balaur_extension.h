/* The C ABI for a Balaur extension.
 *
 * Written by hand and committed so that any drift from the Rust definitions
 * in crates/balaur_plugin/src/capi.rs is visible in review. The assertions at
 * the bottom fail the compile if the layouts stop agreeing.
 *
 * An extension is a shared library exporting the four balaur_extension_*
 * symbols at the bottom of this file. Drop it in a project's extensions/
 * directory and the engine loads it.
 *
 * THE ONE RULE: no allocation crosses this boundary. Every BalaurValue is a
 * borrowed view, valid only for the duration of the call it appears in.
 * Arguments point at memory the host owns. A value you write to `out` must
 * point at memory that stays alive until your function returns; the host
 * copies it before doing anything else. There is therefore no free function,
 * and nothing to get wrong about which allocator owns what.
 *
 * Static storage is the easy way to honour that for returned strings and
 * lists. Returning a pointer to a local is a use-after-return, exactly as it
 * would be anywhere else in C.
 */

#ifndef BALAUR_EXTENSION_H
#define BALAUR_EXTENSION_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Bumped whenever anything in this file changes shape. The host checks this
 * before it calls anything else, so a stale extension is refused with a
 * message instead of misreading memory. */
#define BALAUR_ABI_VERSION 1u

/* Which arm of BalaurPayload is live. */
enum {
    BALAUR_NIL = 0,
    BALAUR_BOOL = 1,
    BALAUR_INT = 2,
    BALAUR_NUM = 3,
    BALAUR_STR = 4,
    BALAUR_VEC2 = 5,
    BALAUR_VEC3 = 6,
    BALAUR_COLOR = 7,
    BALAUR_NODE = 8,
    BALAUR_CALLBACK = 9,
    BALAUR_LIST = 10,
    BALAUR_MAP = 11,
    BALAUR_MANY = 12
};

/* UTF-8 bytes. NOT NUL-terminated: always pass a length. */
typedef struct BalaurStr {
    const uint8_t *ptr;
    size_t len;
} BalaurStr;

struct BalaurValue;
struct BalaurEntry;

typedef struct BalaurSlice {
    const struct BalaurValue *items;
    size_t len;
} BalaurSlice;

typedef struct BalaurMapRef {
    const struct BalaurEntry *items;
    size_t len;
} BalaurMapRef;

typedef union BalaurPayload {
    bool boolean;
    int64_t integer;
    double number;
    /* Node entity bits, or a callback id. */
    uint64_t bits;
    /* vec2 uses the first two lanes, vec3 the first three, color all four. */
    float vector[4];
    BalaurStr string;
    BalaurSlice list;
    BalaurMapRef map;
} BalaurPayload;

typedef struct BalaurValue {
    uint32_t kind;
    uint32_t reserved;
    BalaurPayload payload;
} BalaurValue;

typedef struct BalaurEntry {
    BalaurStr key;
    BalaurValue value;
} BalaurEntry;

/* Opaque host handles. */
typedef struct BalaurRegistry BalaurRegistry;
typedef struct BalaurModule BalaurModule;

/* What you implement. Return 0 for success.
 *
 * On any other return the host raises an error to the calling script. If you
 * also wrote a BALAUR_STR into `out`, that string becomes the message. */
typedef int32_t (*BalaurFn)(void *user, const BalaurValue *args, size_t argc,
                            BalaurValue *out);

/* The host functions you may call, handed to balaur_extension_declare.
 *
 * A table rather than symbols to link against: resolving symbols back into the
 * host executable requires it to have been linked with its dynamic symbols
 * exported, which is neither the default nor portable. */
typedef struct BalaurApi {
    uint32_t abi_version;
    uint32_t reserved;
    /* Open a named binding group -- "physics", "render", your own. NULL on
     * failure. */
    BalaurModule *(*module_open)(BalaurRegistry *registry, BalaurStr name);
    void (*module_function)(BalaurModule *module, BalaurStr name, BalaurFn fn,
                            void *user);
    void (*module_constant)(BalaurModule *module, BalaurStr name,
                            const BalaurValue *value);
    /* Every opened module must be closed exactly once. */
    void (*module_close)(BalaurModule *module);
    /* Levels: 0 trace, 1 debug, 2 info, 3 warn, 4 error. */
    void (*log)(uint32_t level, BalaurStr message);
} BalaurApi;

/* ---- convenience, header-only -------------------------------------------- */

static inline BalaurStr balaur_str(const char *text) {
    BalaurStr out;
    out.ptr = (const uint8_t *)text;
    out.len = 0;
    if (text != NULL) {
        while (text[out.len] != '\0') {
            out.len++;
        }
    }
    return out;
}

static inline BalaurValue balaur_nil(void) {
    BalaurValue out;
    out.kind = BALAUR_NIL;
    out.reserved = 0;
    out.payload.bits = 0;
    return out;
}

static inline BalaurValue balaur_int(int64_t value) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_INT;
    out.payload.integer = value;
    return out;
}

static inline BalaurValue balaur_num(double value) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_NUM;
    out.payload.number = value;
    return out;
}

static inline BalaurValue balaur_bool(bool value) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_BOOL;
    out.payload.boolean = value;
    return out;
}

/* The pointed-at bytes must outlive the call. Static storage is the safe
 * choice; a local is a use-after-return. */
static inline BalaurValue balaur_string(const char *text) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_STR;
    out.payload.string = balaur_str(text);
    return out;
}

static inline BalaurValue balaur_vec3(float x, float y, float z) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_VEC3;
    out.payload.vector[0] = x;
    out.payload.vector[1] = y;
    out.payload.vector[2] = z;
    out.payload.vector[3] = 0.0f;
    return out;
}

/* `items` must outlive the call, as above. */
static inline BalaurValue balaur_list(const BalaurValue *items, size_t len) {
    BalaurValue out = balaur_nil();
    out.kind = BALAUR_LIST;
    out.payload.list.items = items;
    out.payload.list.len = len;
    return out;
}

/* ---- what your library must export --------------------------------------- */

/* Must return BALAUR_ABI_VERSION. Called before anything else. */
uint32_t balaur_extension_abi(void);
/* Static, NUL-terminated. This, not the file name, is the plugin's name. */
const char *balaur_extension_name(void);
const char *balaur_extension_version(void);
/* Register everything here. Return 0 for success. */
int32_t balaur_extension_declare(const BalaurApi *api, BalaurRegistry *registry);

/* ---- layout, asserted rather than assumed -------------------------------- */

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(void *) == 8, "the balaur abi is defined for 64-bit builds");
_Static_assert(sizeof(BalaurStr) == 16, "BalaurStr disagrees with the rust definition");
_Static_assert(sizeof(BalaurSlice) == 16, "BalaurSlice disagrees with the rust definition");
_Static_assert(sizeof(BalaurMapRef) == 16, "BalaurMapRef disagrees with the rust definition");
_Static_assert(sizeof(BalaurPayload) == 16, "BalaurPayload disagrees with the rust definition");
_Static_assert(sizeof(BalaurValue) == 24, "BalaurValue disagrees with the rust definition");
_Static_assert(sizeof(BalaurEntry) == 40, "BalaurEntry disagrees with the rust definition");
_Static_assert(offsetof(BalaurValue, payload) == 8, "BalaurValue payload moved");
_Static_assert(offsetof(BalaurEntry, value) == 16, "BalaurEntry value moved");
#endif

#ifdef __cplusplus
}
#endif

#endif /* BALAUR_EXTENSION_H */
