/* Balaur Script modules translated to C by balaur_bs::c. Every module in this
 * file shares what follows; the runtime finds them through balaur_bs_modules.
 * Build with -ffp-contract=off, so floats round as they do on every engine. */
#include <setjmp.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#if defined(__APPLE__)
#include <pthread.h>
#endif

#define BS_ABI 1u
#define BS_UNREACHABLE 1
#define BS_OUT_OF_BOUNDS 2
#define BS_DIVIDE_BY_ZERO 3
#define BS_OVERFLOW 4
#define BS_STACK 5
#define BS_HOST 6
#define BS_CONVERSION 7
#define BS_INDIRECT 8
/* What a host function may still need of the stack once a script ran out. */
#define BS_STACK_MARGIN (128u << 10)

typedef struct bs_host {
  int32_t (*import)(void *ctx, uint32_t index, const uint64_t *args, uint64_t *results);
} bs_host;

typedef struct bs_inst {
  uint8_t *mem;
  uint64_t mem_len;
  uint64_t mem_max;
  jmp_buf *trap;
  uintptr_t floor;
  const bs_host *host;
  void *host_ctx;
  uint64_t g[];
} bs_inst;

typedef struct bs_module {
  uint32_t abi;
  uint32_t nfuncs;
  const char *const *func_names;
  const char *const *func_results;
  uint32_t nglobals;
  const char *const *global_names;
  const uint32_t *global_index;
  uint32_t nimports;
  const char *const *import_params;
  const char *const *import_results;
  bs_inst *(*instantiate)(const bs_host *host, void *ctx, uint64_t max_bytes);
  int32_t (*call)(bs_inst *inst, uint32_t func, const uint64_t *args, uint64_t *results);
  void (*destroy)(bs_inst *inst);
  uint8_t *(*memory)(bs_inst *inst, uint64_t *len);
  uint64_t *(*globals)(bs_inst *inst);
} bs_module;

typedef struct bs_entry {
  const char *hash;
  const bs_module *module;
} bs_entry;

__attribute__((noreturn)) static void bs_trap(bs_inst *I, int32_t code) { longjmp(*I->trap, code); }

static uintptr_t bs_stack_floor(void) {
#if defined(__APPLE__)
  pthread_t t = pthread_self();
  uintptr_t top = (uintptr_t)pthread_get_stackaddr_np(t);
  return top - pthread_get_stacksize_np(t) + BS_STACK_MARGIN;
#else
  return (uintptr_t)__builtin_frame_address(0) - (512u << 10);
#endif
}

/* Every function, kept from turning a recursion into a loop: a script that
 * recurses without end runs out of stack, as it does on every engine. */
#define BS_FN static __attribute__((disable_tail_calls))
#define BS_ENTER(I) \
  if (__builtin_expect((uintptr_t)__builtin_frame_address(0) < (I)->floor, 0)) bs_trap(I, BS_STACK)
#define BS_RELOAD() (M = I->mem, L = I->mem_len)
#define BS_CHECK(a, n) \
  if (__builtin_expect((a) + (n) > L, 0)) bs_trap(I, BS_OUT_OF_BOUNDS)

#define BS_LOAD(name, T, n) \
  static inline T name(bs_inst *I, const uint8_t *M, uint64_t L, uint64_t a) { \
    T v; \
    BS_CHECK(a, n); \
    memcpy(&v, M + a, n); \
    return v; \
  }
#define BS_STORE(name, T, n) \
  static inline void name(bs_inst *I, uint8_t *M, uint64_t L, uint64_t a, T v) { \
    BS_CHECK(a, n); \
    memcpy(M + a, &v, n); \
  }
BS_LOAD(bs_ld8, uint8_t, 1)
BS_LOAD(bs_ld16, uint16_t, 2)
BS_LOAD(bs_ld32, uint32_t, 4)
BS_LOAD(bs_ld64, uint64_t, 8)
BS_LOAD(bs_ldf64, double, 8)
BS_STORE(bs_st8, uint8_t, 1)
BS_STORE(bs_st16, uint16_t, 2)
BS_STORE(bs_st32, uint32_t, 4)
BS_STORE(bs_st64, uint64_t, 8)
BS_STORE(bs_stf64, double, 8)

static inline uint64_t bs_bits(double x) {
  uint64_t b;
  memcpy(&b, &x, 8);
  return b;
}
static inline double bs_f64(uint64_t b) {
  double x;
  memcpy(&x, &b, 8);
  return x;
}

static uint32_t bs_grow(bs_inst *I, uint32_t pages) {
  uint64_t old = I->mem_len, add = (uint64_t)pages << 16;
  if (add > I->mem_max - old) return 0xFFFFFFFFu;
  if (add) {
    uint8_t *m = (uint8_t *)realloc(I->mem, old + add);
    if (!m) return 0xFFFFFFFFu;
    memset(m + old, 0, add);
    I->mem = m;
    I->mem_len = old + add;
  }
  return (uint32_t)(old >> 16);
}

static inline void bs_copy(bs_inst *I, uint8_t *M, uint64_t L, uint64_t d, uint64_t s, uint64_t n) {
  BS_CHECK(d, n);
  BS_CHECK(s, n);
  memmove(M + d, M + s, n);
}
static inline void bs_fill(bs_inst *I, uint8_t *M, uint64_t L, uint64_t d, uint32_t v, uint64_t n) {
  BS_CHECK(d, n);
  memset(M + d, (int)(v & 0xFF), n);
}

static inline uint32_t bs_div_s32(bs_inst *I, uint32_t a, uint32_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  if (a == 0x80000000u && b == 0xFFFFFFFFu) bs_trap(I, BS_OVERFLOW);
  return (uint32_t)((int32_t)a / (int32_t)b);
}
static inline uint32_t bs_rem_s32(bs_inst *I, uint32_t a, uint32_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  if (b == 0xFFFFFFFFu) return 0;
  return (uint32_t)((int32_t)a % (int32_t)b);
}
static inline uint32_t bs_div_u32(bs_inst *I, uint32_t a, uint32_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  return a / b;
}
static inline uint32_t bs_rem_u32(bs_inst *I, uint32_t a, uint32_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  return a % b;
}
static inline uint64_t bs_div_s64(bs_inst *I, uint64_t a, uint64_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  if (a == 0x8000000000000000ull && b == 0xFFFFFFFFFFFFFFFFull) bs_trap(I, BS_OVERFLOW);
  return (uint64_t)((int64_t)a / (int64_t)b);
}
static inline uint64_t bs_rem_s64(bs_inst *I, uint64_t a, uint64_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  if (b == 0xFFFFFFFFFFFFFFFFull) return 0;
  return (uint64_t)((int64_t)a % (int64_t)b);
}
static inline uint64_t bs_div_u64(bs_inst *I, uint64_t a, uint64_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  return a / b;
}
static inline uint64_t bs_rem_u64(bs_inst *I, uint64_t a, uint64_t b) {
  if (b == 0) bs_trap(I, BS_DIVIDE_BY_ZERO);
  return a % b;
}

/* wasm's min and max: a NaN wins, and -0 is below +0. */
static inline double bs_min(double a, double b) {
  if (a != a || b != b) return a + b;
  if (a == b) return __builtin_signbit(a) ? a : b;
  return a < b ? a : b;
}
static inline double bs_max(double a, double b) {
  if (a != a || b != b) return a + b;
  if (a == b) return __builtin_signbit(a) ? b : a;
  return a > b ? a : b;
}

static inline uint64_t bs_trunc_sat_s64(double x) {
  if (x != x) return 0;
  if (x >= 9223372036854775808.0) return 0x7FFFFFFFFFFFFFFFull;
  if (x < -9223372036854775808.0) return 0x8000000000000000ull;
  return (uint64_t)(int64_t)x;
}
static inline uint64_t bs_trunc_s64(bs_inst *I, double x) {
  if (x != x || x >= 9223372036854775808.0 || x < -9223372036854775808.0) bs_trap(I, BS_CONVERSION);
  return (uint64_t)(int64_t)x;
}
static inline uint32_t bs_trunc_sat_s32(double x) {
  if (x != x) return 0;
  if (x >= 2147483648.0) return 0x7FFFFFFFu;
  if (x < -2147483648.0) return 0x80000000u;
  return (uint32_t)(int32_t)x;
}
static inline uint32_t bs_trunc_s32(bs_inst *I, double x) {
  if (x != x || x >= 2147483648.0 || x <= -2147483649.0) bs_trap(I, BS_CONVERSION);
  return (uint32_t)(int32_t)x;
}

/* One call into a module from the host. A trap unwinds to here, never past a
 * host frame: a call a host function makes back in has its own. */
static int32_t bs_run(bs_inst *I, void (*dispatch)(bs_inst *, uint32_t, const uint64_t *, uint64_t *),
                      uint32_t f, const uint64_t *a, uint64_t *r) {
  jmp_buf here;
  jmp_buf *outer = I->trap;
  uintptr_t floor = I->floor;
  if (!outer) I->floor = bs_stack_floor();
  I->trap = &here;
  int32_t code = setjmp(here);
  if (code == 0) dispatch(I, f, a, r);
  I->trap = outer;
  I->floor = floor;
  return code;
}

static bs_inst *bs_new(const bs_host *host, void *ctx, uint32_t nglobals, uint64_t initial, uint64_t max) {
  bs_inst *I = (bs_inst *)calloc(1, sizeof(bs_inst) + nglobals * sizeof(uint64_t));
  if (!I) return 0;
  I->mem = (uint8_t *)calloc(initial ? initial : 1, 1);
  if (!I->mem) {
    free(I);
    return 0;
  }
  I->mem_len = initial;
  I->mem_max = max < initial ? initial : max;
  I->host = host;
  I->host_ctx = ctx;
  return I;
}

static void bs_destroy(bs_inst *I) {
  free(I->mem);
  free(I);
}
static uint8_t *bs_memory(bs_inst *I, uint64_t *len) {
  *len = I->mem_len;
  return I->mem;
}
static uint64_t *bs_globals(bs_inst *I) { return I->g; }
