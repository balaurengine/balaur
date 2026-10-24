/* Balaur Script modules translated to C by balaur_bs::c. Every module in this
 * file shares what follows; the runtime finds them through balaur_bs_modules.
 * It includes no header and calls no library, so clang and lld build it for
 * any target. Build with -ffp-contract=off, so floats round as they do on
 * every engine, and -ffreestanding -fno-math-errno -fno-stack-protector. */
typedef __INT8_TYPE__ int8_t;
typedef __INT16_TYPE__ int16_t;
typedef __INT32_TYPE__ int32_t;
typedef __INT64_TYPE__ int64_t;
typedef __UINT8_TYPE__ uint8_t;
typedef __UINT16_TYPE__ uint16_t;
typedef __UINT32_TYPE__ uint32_t;
typedef __UINT64_TYPE__ uint64_t;
typedef __UINTPTR_TYPE__ uintptr_t;
typedef __SIZE_TYPE__ size_t;

#define BS_ABI 2u
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
  /* The host's allocator: a null block allocates, a new size of 0 frees, and
   * every byte a block gains reads as zero. */
  void *(*resize)(void *block, uint64_t old_size, uint64_t new_size);
  /* The lowest address this thread's stack reaches, or 0 when the host cannot tell. */
  uintptr_t (*stack_bottom)(void);
} bs_host;

/* A trap jumps back to the host's call: by hand under clang, which has no
 * builtin for it on arm64, and through the C library under GCC. */
#if defined(__clang__)
typedef uint64_t bs_jmp[32];
#if defined(__aarch64__)
__attribute__((naked, noinline, returns_twice)) static int bs_setjmp(uint64_t *b) {
  __asm__ volatile(
      "mov x16, sp\n"
      "stp x19, x20, [x0, #0]\n"
      "stp x21, x22, [x0, #16]\n"
      "stp x23, x24, [x0, #32]\n"
      "stp x25, x26, [x0, #48]\n"
      "stp x27, x28, [x0, #64]\n"
      "stp x29, x30, [x0, #80]\n"
      "str x16, [x0, #96]\n"
      "stp d8, d9, [x0, #104]\n"
      "stp d10, d11, [x0, #120]\n"
      "stp d12, d13, [x0, #136]\n"
      "stp d14, d15, [x0, #152]\n"
      "mov w0, #0\n"
      "ret\n");
}
__attribute__((naked, noinline, noreturn)) static void bs_longjmp(uint64_t *b) {
  __asm__ volatile(
      "ldp x19, x20, [x0, #0]\n"
      "ldp x21, x22, [x0, #16]\n"
      "ldp x23, x24, [x0, #32]\n"
      "ldp x25, x26, [x0, #48]\n"
      "ldp x27, x28, [x0, #64]\n"
      "ldp x29, x30, [x0, #80]\n"
      "ldr x16, [x0, #96]\n"
      "mov sp, x16\n"
      "ldp d8, d9, [x0, #104]\n"
      "ldp d10, d11, [x0, #120]\n"
      "ldp d12, d13, [x0, #136]\n"
      "ldp d14, d15, [x0, #152]\n"
      "mov w0, #1\n"
      "ret\n");
}
#elif defined(__x86_64__) && defined(_WIN32)
__attribute__((naked, noinline, returns_twice)) static int bs_setjmp(uint64_t *b) {
  __asm__ volatile(
      "movq %rbx, 0(%rcx)\n"
      "movq %rbp, 8(%rcx)\n"
      "movq %rdi, 16(%rcx)\n"
      "movq %rsi, 24(%rcx)\n"
      "movq %r12, 32(%rcx)\n"
      "movq %r13, 40(%rcx)\n"
      "movq %r14, 48(%rcx)\n"
      "movq %r15, 56(%rcx)\n"
      "leaq 8(%rsp), %rdx\n"
      "movq %rdx, 64(%rcx)\n"
      "movq (%rsp), %rdx\n"
      "movq %rdx, 72(%rcx)\n"
      "movdqu %xmm6, 80(%rcx)\n"
      "movdqu %xmm7, 96(%rcx)\n"
      "movdqu %xmm8, 112(%rcx)\n"
      "movdqu %xmm9, 128(%rcx)\n"
      "movdqu %xmm10, 144(%rcx)\n"
      "movdqu %xmm11, 160(%rcx)\n"
      "movdqu %xmm12, 176(%rcx)\n"
      "movdqu %xmm13, 192(%rcx)\n"
      "movdqu %xmm14, 208(%rcx)\n"
      "movdqu %xmm15, 224(%rcx)\n"
      "xorl %eax, %eax\n"
      "ret\n");
}
__attribute__((naked, noinline, noreturn)) static void bs_longjmp(uint64_t *b) {
  __asm__ volatile(
      "movq 0(%rcx), %rbx\n"
      "movq 8(%rcx), %rbp\n"
      "movq 16(%rcx), %rdi\n"
      "movq 24(%rcx), %rsi\n"
      "movq 32(%rcx), %r12\n"
      "movq 40(%rcx), %r13\n"
      "movq 48(%rcx), %r14\n"
      "movq 56(%rcx), %r15\n"
      "movdqu 80(%rcx), %xmm6\n"
      "movdqu 96(%rcx), %xmm7\n"
      "movdqu 112(%rcx), %xmm8\n"
      "movdqu 128(%rcx), %xmm9\n"
      "movdqu 144(%rcx), %xmm10\n"
      "movdqu 160(%rcx), %xmm11\n"
      "movdqu 176(%rcx), %xmm12\n"
      "movdqu 192(%rcx), %xmm13\n"
      "movdqu 208(%rcx), %xmm14\n"
      "movdqu 224(%rcx), %xmm15\n"
      "movq 64(%rcx), %rsp\n"
      "movl $1, %eax\n"
      "jmpq *72(%rcx)\n");
}
#elif defined(__x86_64__)
__attribute__((naked, noinline, returns_twice)) static int bs_setjmp(uint64_t *b) {
  __asm__ volatile(
      "movq %rbx, 0(%rdi)\n"
      "movq %rbp, 8(%rdi)\n"
      "movq %r12, 16(%rdi)\n"
      "movq %r13, 24(%rdi)\n"
      "movq %r14, 32(%rdi)\n"
      "movq %r15, 40(%rdi)\n"
      "leaq 8(%rsp), %rdx\n"
      "movq %rdx, 48(%rdi)\n"
      "movq (%rsp), %rdx\n"
      "movq %rdx, 56(%rdi)\n"
      "xorl %eax, %eax\n"
      "ret\n");
}
__attribute__((naked, noinline, noreturn)) static void bs_longjmp(uint64_t *b) {
  __asm__ volatile(
      "movq 0(%rdi), %rbx\n"
      "movq 8(%rdi), %rbp\n"
      "movq 16(%rdi), %r12\n"
      "movq 24(%rdi), %r13\n"
      "movq 32(%rdi), %r14\n"
      "movq 40(%rdi), %r15\n"
      "movq 48(%rdi), %rsp\n"
      "movl $1, %eax\n"
      "jmpq *56(%rdi)\n");
}
#else
#error "balaur_bs::c has no trap jump for this architecture"
#endif
#define BS_SETJMP(b) bs_setjmp(b)
#define BS_LONGJMP(b) bs_longjmp(b)
#else
#include <setjmp.h>
typedef jmp_buf bs_jmp;
#define BS_SETJMP(b) setjmp(b)
#define BS_LONGJMP(b) longjmp(b, 1)
#endif

typedef struct bs_inst {
  uint8_t *mem;
  uint64_t mem_len;
  uint64_t mem_max;
  bs_jmp *trap;
  int32_t code;
  uintptr_t floor;
  const bs_host *host;
  void *host_ctx;
  uint64_t size;
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

#if defined(_WIN32)
#define BS_EXPORT __declspec(dllexport)
#else
#define BS_EXPORT __attribute__((visibility("default")))
#endif

/* What a compiler may still call for a copy, a fill or a square root, so the
 * build links against nothing; hidden, so it never stands in for the host's. */
#if defined(__clang__)
#define BS_LIBCALL __attribute__((visibility("hidden"), used, no_builtin))
#else
#define BS_LIBCALL __attribute__((visibility("hidden"), used, optimize("no-tree-loop-distribute-patterns")))
#endif
BS_LIBCALL void *memcpy(void *d, const void *s, size_t n) {
  uint8_t *o = (uint8_t *)d;
  const uint8_t *i = (const uint8_t *)s;
  while (n--) *o++ = *i++;
  return d;
}
BS_LIBCALL void *memmove(void *d, const void *s, size_t n) {
  uint8_t *o = (uint8_t *)d;
  const uint8_t *i = (const uint8_t *)s;
  if (o < i) {
    while (n--) *o++ = *i++;
  } else {
    while (n--) o[n] = i[n];
  }
  return d;
}
BS_LIBCALL void *memset(void *d, int c, size_t n) {
  uint8_t *o = (uint8_t *)d;
  while (n--) *o++ = (uint8_t)c;
  return d;
}
#if defined(__APPLE__)
BS_LIBCALL void __bzero(void *d, size_t n) { memset(d, 0, n); }
#endif
BS_LIBCALL double sqrt(double x) { return x >= 0.0 ? __builtin_sqrt(x) : __builtin_nan(""); }
#if defined(_WIN32)
int _fltused = 0;
#if defined(__clang__) && defined(__x86_64__)
/* compiler-rt's probe: touch each page a frame of RAX bytes spans. */
__attribute__((naked, used)) void __chkstk(void) {
  __asm__ volatile(
      "pushq %rcx\n"
      "pushq %rax\n"
      "cmpq $0x1000, %rax\n"
      "leaq 24(%rsp), %rcx\n"
      "jb 1f\n"
      "2:\n"
      "subq $0x1000, %rcx\n"
      "testq %rcx, (%rcx)\n"
      "subq $0x1000, %rax\n"
      "cmpq $0x1000, %rax\n"
      "ja 2b\n"
      "1:\n"
      "subq %rax, %rcx\n"
      "testq %rcx, (%rcx)\n"
      "popq %rax\n"
      "popq %rcx\n"
      "ret\n");
}
#elif defined(__clang__) && defined(__aarch64__)
/* compiler-rt's probe: touch each page a frame of X15 * 16 bytes spans. */
__attribute__((naked, used)) void __chkstk(void) {
  __asm__ volatile(
      "lsl x16, x15, #4\n"
      "mov x17, sp\n"
      "1:\n"
      "sub x17, x17, #4096\n"
      "subs x16, x16, #4096\n"
      "ldr xzr, [x17]\n"
      "b.gt 1b\n"
      "ret\n");
}
#endif
#endif

__attribute__((noreturn)) static void bs_trap(bs_inst *I, int32_t code) {
  I->code = code;
  BS_LONGJMP(*I->trap);
}

static uintptr_t bs_stack_floor(const bs_host *host) {
  uintptr_t bottom = host->stack_bottom ? host->stack_bottom() : 0;
  if (bottom) return bottom + BS_STACK_MARGIN;
  return (uintptr_t)__builtin_frame_address(0) - (512u << 10);
}

/* Every function, kept from turning a recursion into a loop: a script that
 * recurses without end runs out of stack, as it does on every engine. GCC
 * has no `disable_tail_calls`, and `-w` hides that it ignores it: there the
 * same pass is switched off by name. */
#if defined(__clang__)
#define BS_FN static __attribute__((disable_tail_calls))
#else
#define BS_FN static __attribute__((optimize("no-optimize-sibling-calls")))
#endif
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

/* Rounding as wasm defines it: one instruction on arm64 or with SSE4.1, and
 * exact on the bits on an x86-64 without it, where C would call libm. */
#if defined(__aarch64__) || defined(__SSE4_1__)
static inline double bs_trunc(double x) { return __builtin_trunc(x); }
static inline double bs_floor(double x) { return __builtin_floor(x); }
static inline double bs_ceil(double x) { return __builtin_ceil(x); }
static inline double bs_nearest(double x) { return __builtin_rint(x); }
#else
static inline double bs_trunc(double x) {
  uint64_t b = bs_bits(x);
  int32_t e = (int32_t)((b >> 52) & 0x7FF) - 1023;
  if (e >= 52) return x;
  if (e < 0) return bs_f64(b & 0x8000000000000000ull);
  return bs_f64(b & ~(0x000FFFFFFFFFFFFFull >> e));
}
static inline double bs_floor(double x) {
  double t = bs_trunc(x);
  return x < t ? t - 1.0 : t;
}
static inline double bs_ceil(double x) {
  double t = bs_trunc(x);
  return x > t ? t + 1.0 : t;
}
/* Adding and taking back 2^52 rounds a smaller magnitude to the nearest
 * whole number, ties to even, as the default rounding mode does. */
static inline double bs_nearest(double x) {
  double a = __builtin_fabs(x);
  if (!(a < 4503599627370496.0)) return x;
  return __builtin_copysign((a + 4503599627370496.0) - 4503599627370496.0, x);
}
#endif

static uint32_t bs_grow(bs_inst *I, uint32_t pages) {
  uint64_t old = I->mem_len, add = (uint64_t)pages << 16;
  if (add > I->mem_max - old) return 0xFFFFFFFFu;
  if (add) {
    uint8_t *m = (uint8_t *)I->host->resize(I->mem, old, old + add);
    if (!m) return 0xFFFFFFFFu;
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
  bs_jmp here;
  bs_jmp *outer = I->trap;
  uintptr_t floor = I->floor;
  int32_t code = 0;
  if (!outer) I->floor = bs_stack_floor(I->host);
  I->trap = &here;
  if (BS_SETJMP(here) == 0) {
    dispatch(I, f, a, r);
  } else {
    code = I->code;
  }
  I->trap = outer;
  I->floor = floor;
  return code;
}

static bs_inst *bs_new(const bs_host *host, void *ctx, uint32_t nglobals, uint64_t initial, uint64_t max) {
  uint64_t size = sizeof(bs_inst) + nglobals * sizeof(uint64_t);
  bs_inst *I = (bs_inst *)host->resize(0, 0, size);
  if (!I) return 0;
  I->mem = (uint8_t *)host->resize(0, 0, initial);
  if (!I->mem) {
    host->resize(I, size, 0);
    return 0;
  }
  I->mem_len = initial;
  I->mem_max = max < initial ? initial : max;
  I->host = host;
  I->host_ctx = ctx;
  I->size = size;
  return I;
}

static void bs_destroy(bs_inst *I) {
  const bs_host *host = I->host;
  host->resize(I->mem, I->mem_len, 0);
  host->resize(I, I->size, 0);
}
static uint8_t *bs_memory(bs_inst *I, uint64_t *len) {
  *len = I->mem_len;
  return I->mem;
}
static uint64_t *bs_globals(bs_inst *I) { return I->g; }
