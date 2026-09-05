/* A Balaur extension written in C.
 *
 * Not a cargo package and not linked against the engine: one .c file, one
 * header, compiled to a shared library by whatever compiler you like. That is
 * the point of it -- the Rust extension next door needs the identical rustc on
 * both sides, and this needs nothing.
 *
 *   cc -shared -fPIC -I../../crates/balaur_plugin/include \
 *      -o libcounter.so counter.c
 *
 * Drop the result in a project's extensions/ directory and scripts can call
 * counter.add(2, 3).
 */

#include "balaur_extension.h"

#include <string.h>

/* State the extension owns. Handed back to every function as `user`, which is
 * how a C extension keeps state without any Rust type being involved. */
typedef struct Counter {
    int64_t calls;
} Counter;

static Counter COUNTER = {0};

/* A script hands whole numbers over as BALAUR_INT and fractions as BALAUR_NUM, so
 * anything taking a number should accept both. */
static bool as_number(const BalaurValue *value, double *out) {
    if (value->kind == BALAUR_NUM) {
        *out = value->payload.number;
        return true;
    }
    if (value->kind == BALAUR_INT) {
        *out = (double)value->payload.integer;
        return true;
    }
    return false;
}

static int32_t add(void *user, const BalaurValue *args, size_t argc,
                   BalaurValue *out) {
    (void)user;
    double a = 0.0;
    double b = 0.0;
    if (argc != 2 || !as_number(&args[0], &a) || !as_number(&args[1], &b)) {
        /* Non-zero return plus a string is how an error reaches the script. */
        *out = balaur_string("add expects two numbers");
        return 1;
    }
    *out = balaur_num(a + b);
    return 0;
}

static int32_t bump(void *user, const BalaurValue *args, size_t argc,
                    BalaurValue *out) {
    (void)args;
    (void)argc;
    Counter *counter = (Counter *)user;
    counter->calls += 1;
    *out = balaur_int(counter->calls);
    return 0;
}

/* Returned strings must outlive the call, so the buffer is static. The host
 * copies out of it before this function's caller returns. */
static char GREETING[128];

static int32_t greet(void *user, const BalaurValue *args, size_t argc,
                     BalaurValue *out) {
    (void)user;
    if (argc != 1 || args[0].kind != BALAUR_STR) {
        *out = balaur_string("greet expects one string");
        return 1;
    }
    /* BalaurStr is not NUL-terminated -- always use the length. */
    size_t len = args[0].payload.string.len;
    if (len > sizeof(GREETING) - 16) {
        len = sizeof(GREETING) - 16;
    }
    memcpy(GREETING, "hello, ", 7);
    memcpy(GREETING + 7, args[0].payload.string.ptr, len);
    GREETING[7 + len] = '\0';
    *out = balaur_string(GREETING);
    return 0;
}

/* Same rule for lists: static storage, copied by the host on return. */
static BalaurValue TRIPLE[3];

static int32_t triple(void *user, const BalaurValue *args, size_t argc,
                      BalaurValue *out) {
    (void)user;
    double n = 0.0;
    if (argc != 1 || !as_number(&args[0], &n)) {
        *out = balaur_string("triple expects one number");
        return 1;
    }
    TRIPLE[0] = balaur_num(n);
    TRIPLE[1] = balaur_num(n * 2.0);
    TRIPLE[2] = balaur_num(n * 3.0);
    *out = balaur_list(TRIPLE, 3);
    return 0;
}

static int32_t sum_list(void *user, const BalaurValue *args, size_t argc,
                        BalaurValue *out) {
    (void)user;
    if (argc != 1 || args[0].kind != BALAUR_LIST) {
        *out = balaur_string("sum_list expects one list");
        return 1;
    }
    double total = 0.0;
    const BalaurSlice *list = &args[0].payload.list;
    for (size_t i = 0; i < list->len; i++) {
        double item = 0.0;
        if (as_number(&list->items[i], &item)) {
            total += item;
        }
    }
    *out = balaur_num(total);
    return 0;
}

static int32_t always_fails(void *user, const BalaurValue *args, size_t argc,
                            BalaurValue *out) {
    (void)user;
    (void)args;
    (void)argc;
    *out = balaur_string("this function fails on purpose");
    return 1;
}

uint32_t balaur_extension_abi(void) { return BALAUR_ABI_VERSION; }

const char *balaur_extension_name(void) { return "counter"; }

const char *balaur_extension_version(void) { return "0.1.0"; }

int32_t balaur_extension_declare(const BalaurApi *api,
                                 BalaurRegistry *registry) {
    if (api == NULL || api->abi_version != BALAUR_ABI_VERSION) {
        return 1;
    }

    BalaurModule *m = api->module_open(registry, balaur_str("counter"));
    if (m == NULL) {
        return 2;
    }

    api->module_function(m, balaur_str("add"), add, NULL);
    api->module_function(m, balaur_str("bump"), bump, &COUNTER);
    api->module_function(m, balaur_str("greet"), greet, NULL);
    api->module_function(m, balaur_str("triple"), triple, NULL);
    api->module_function(m, balaur_str("sum_list"), sum_list, NULL);
    api->module_function(m, balaur_str("always_fails"), always_fails, NULL);

    BalaurValue version = balaur_string("0.1.0");
    api->module_constant(m, balaur_str("VERSION"), &version);

    api->module_close(m);
    api->log(2, balaur_str("counter extension registered"));
    return 0;
}
