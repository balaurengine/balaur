// The emscripten websocket backend, in C because EmscriptenWebSocket* event
// structs contain EM_BOOL, whose size changed across emscripten versions, so
// the layouts stay where the headers define them.
//
// Exposes a plain-C surface; the Rust side in src/emscripten.rs owns all
// allocation and event routing.
#include <emscripten/websocket.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>


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
