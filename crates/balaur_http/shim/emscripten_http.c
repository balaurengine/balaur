// The emscripten fetch backend, in C so emscripten_fetch_attr_t
// comes from the real headers instead of a hand-copied Rust layout that
// silently breaks when emscripten's changes.
//
// Exposes one function with a plain-C surface; the Rust side in
// src/emscripten.rs owns all allocation and event routing.
#include <emscripten/fetch.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef void (*balaur_fetch_callback)(void *user, int status,
                                      const char *body, int body_len,
                                      const char *error);

typedef struct {
  void *user;
  balaur_fetch_callback callback;
  // The header array, its strings, and the body copy: all alive until the
  // fetch settles, because emscripten references them asynchronously.
  char **headers;
  char *body;
} balaur_fetch_state;

static void balaur_fetch_free(balaur_fetch_state *state) {
  if (state->headers) {
    for (char **h = state->headers; *h; h++) free(*h);
    free(state->headers);
  }
  free(state->body);
  free(state);
}

static void balaur_fetch_success(emscripten_fetch_t *fetch) {
  balaur_fetch_state *state = (balaur_fetch_state *)fetch->userData;
  state->callback(state->user, (int)fetch->status, fetch->data,
                  (int)fetch->numBytes, NULL);
  balaur_fetch_free(state);
  emscripten_fetch_close(fetch);
}

static void balaur_fetch_error(emscripten_fetch_t *fetch) {
  balaur_fetch_state *state = (balaur_fetch_state *)fetch->userData;
  // A reachable server answering 4xx/5xx still lands here in fetch's model;
  // report it as a response so the caller sees the status, matching the
  // native backend's "an HTTP error status is a response" rule.
  if (fetch->status != 0) {
    state->callback(state->user, (int)fetch->status, fetch->data,
                    (int)fetch->numBytes, NULL);
  } else {
    state->callback(state->user, 0, NULL, 0, "the request failed");
  }
  balaur_fetch_free(state);
  emscripten_fetch_close(fetch);
}

// `headers_joined`: "name\nvalue\nname\nvalue" or NULL. `body` may be NULL.
void balaur_fetch(const char *method, const char *url,
                  const char *headers_joined, const char *body, int body_len,
                  int timeout_ms, void *user, balaur_fetch_callback callback) {
  emscripten_fetch_attr_t attr;
  emscripten_fetch_attr_init(&attr);
  strncpy(attr.requestMethod, method, sizeof(attr.requestMethod) - 1);
  attr.attributes = EMSCRIPTEN_FETCH_LOAD_TO_MEMORY;
  attr.timeoutMSecs = (unsigned long)timeout_ms;
  attr.onsuccess = balaur_fetch_success;
  attr.onerror = balaur_fetch_error;

  balaur_fetch_state *state = malloc(sizeof(balaur_fetch_state));
  state->user = user;
  state->callback = callback;
  state->headers = NULL;
  state->body = NULL;

  if (headers_joined && *headers_joined) {
    // Count entries, then build the NULL-terminated pair array fetch wants.
    int parts = 1;
    for (const char *c = headers_joined; *c; c++)
      if (*c == '\n') parts++;
    state->headers = malloc(sizeof(char *) * (parts + 1));
    int at = 0;
    const char *cursor = headers_joined;
    while (*cursor) {
      const char *end = strchr(cursor, '\n');
      size_t len = end ? (size_t)(end - cursor) : strlen(cursor);
      char *part = malloc(len + 1);
      memcpy(part, cursor, len);
      part[len] = 0;
      state->headers[at++] = part;
      cursor += len + (end ? 1 : 0);
    }
    state->headers[at] = NULL;
    attr.requestHeaders = (const char *const *)state->headers;
  }

  if (body && body_len > 0) {
    state->body = malloc((size_t)body_len);
    memcpy(state->body, body, (size_t)body_len);
    attr.requestData = state->body;
    attr.requestDataSize = (size_t)body_len;
  }

  attr.userData = state;
  emscripten_fetch(&attr, url);
}
