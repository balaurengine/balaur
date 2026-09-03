// The HTTP half of the emscripten backend, in C so emscripten_fetch_attr_t
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

// ---- The websocket half. Same reasoning: EmscriptenWebSocket* event
// structs contain EM_BOOL, whose size changed across emscripten versions,
// so the layouts stay in C where the headers define them.
#include <emscripten/websocket.h>

typedef void (*balaur_ws_open_callback)(void *user);
typedef void (*balaur_ws_message_callback)(void *user, const char *data,
                                           int len, int is_text);
typedef void (*balaur_ws_close_callback)(void *user, int code,
                                         const char *reason);
typedef void (*balaur_ws_error_callback)(void *user);

typedef struct {
  void *user;
  balaur_ws_open_callback on_open;
  balaur_ws_message_callback on_message;
  balaur_ws_close_callback on_close;
  balaur_ws_error_callback on_error;
} balaur_ws_state;

static EM_BOOL balaur_ws_opened(int type,
                                const EmscriptenWebSocketOpenEvent *event,
                                void *data) {
  (void)type;
  (void)event;
  balaur_ws_state *state = (balaur_ws_state *)data;
  state->on_open(state->user);
  return EM_TRUE;
}

static EM_BOOL balaur_ws_received(int type,
                                  const EmscriptenWebSocketMessageEvent *event,
                                  void *data) {
  (void)type;
  balaur_ws_state *state = (balaur_ws_state *)data;
  state->on_message(state->user, (const char *)event->data,
                    (int)event->numBytes, event->isText ? 1 : 0);
  return EM_TRUE;
}

static EM_BOOL balaur_ws_closed(int type,
                                const EmscriptenWebSocketCloseEvent *event,
                                void *data) {
  (void)type;
  balaur_ws_state *state = (balaur_ws_state *)data;
  state->on_close(state->user, (int)event->code, event->reason);
  free(state);
  return EM_TRUE;
}

static EM_BOOL balaur_ws_failed(int type,
                                const EmscriptenWebSocketErrorEvent *event,
                                void *data) {
  (void)type;
  (void)event;
  balaur_ws_state *state = (balaur_ws_state *)data;
  state->on_error(state->user);
  return EM_TRUE;
}

// Returns the socket handle, or 0 when websockets are unsupported or the
// url is refused. The callbacks receive `user`; after on_close fires the
// state is freed and no further callbacks come.
int balaur_ws_connect(const char *url, void *user,
                      balaur_ws_open_callback on_open,
                      balaur_ws_message_callback on_message,
                      balaur_ws_close_callback on_close,
                      balaur_ws_error_callback on_error) {
  if (!emscripten_websocket_is_supported()) return 0;
  EmscriptenWebSocketCreateAttributes attributes;
  emscripten_websocket_init_create_attributes(&attributes);
  attributes.url = url;
  EMSCRIPTEN_WEBSOCKET_T socket = emscripten_websocket_new(&attributes);
  if (socket <= 0) return 0;

  balaur_ws_state *state = malloc(sizeof(balaur_ws_state));
  state->user = user;
  state->on_open = on_open;
  state->on_message = on_message;
  state->on_close = on_close;
  state->on_error = on_error;
  emscripten_websocket_set_onopen_callback(socket, state, balaur_ws_opened);
  emscripten_websocket_set_onmessage_callback(socket, state,
                                              balaur_ws_received);
  emscripten_websocket_set_onclose_callback(socket, state, balaur_ws_closed);
  emscripten_websocket_set_onerror_callback(socket, state, balaur_ws_failed);
  return (int)socket;
}

void balaur_ws_send_text(int socket, const char *text) {
  emscripten_websocket_send_utf8_text((EMSCRIPTEN_WEBSOCKET_T)socket, text);
}

void balaur_ws_send_binary(int socket, const void *data, int len) {
  emscripten_websocket_send_binary((EMSCRIPTEN_WEBSOCKET_T)socket, (void *)data,
                                   (uint32_t)len);
}

void balaur_ws_close(int socket, int code, const char *reason) {
  emscripten_websocket_close((EMSCRIPTEN_WEBSOCKET_T)socket,
                             (unsigned short)code, reason);
}
