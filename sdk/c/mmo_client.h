/* mmo_client.h: C interface to the mmo-engine game client library.
 *
 * Build the library with `cargo build --release -p mmo-client`; you get
 * target/release/libmmo_client.so (.dylib / .dll on other platforms) and a static
 * libmmo_client.a. The library owns the UDP socket and the whole protocol, so an
 * engine integration is: connect, call mmo_poll() once per frame, send intent, read entities.
 *
 * Threading: use one client from one thread. Strings are UTF-8, NUL-terminated.
 */
#ifndef MMO_CLIENT_H
#define MMO_CLIENT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MmoClient MmoClient; /* opaque */

enum {
  MMO_CONNECTING = 0,
  MMO_PLAYING = 1,
  MMO_REJECTED = 2, /* wrong server key, version or content; see mmo_status_text */
  MMO_CLOSED = 3    /* the server never answered */
};

#define MMO_MAX_REPL 4 /* replicated attributes per entity */
#define MMO_GROUND (-128) /* MmoEntity.z for something on the bare ground */

typedef struct {
  uint32_t id;
  float x, y;                  /* world units */
  int32_t z;                   /* block layer its feet are in: MMO_GROUND, or it stands z * build.block up */
  float attrs[MMO_MAX_REPL];   /* in mmo_attr_name order; unused slots are 0 */
} MmoEntity;

typedef struct {
  uint32_t from;     /* entity id of the speaker */
  uint8_t channel;   /* 0 = say (nearby), 1 = shout (everyone) */
  char text[256];    /* NUL-terminated UTF-8 */
} MmoChat;

/* Protocol version this library speaks (the server's /play/info "proto" must match). */
uint16_t mmo_protocol_version(void);

/* Why the last call that returned NULL failed (this thread). */
const char *mmo_last_error(void);

/* Identities. All "out" buffers for identities are 65 bytes (64 hex characters plus NUL).
 * An identity is the player's account: keep it secret, store it, or derive it again from a passphrase. */
void mmo_new_identity(char *out);
int mmo_identity_from_passphrase(const char *account, const char *passphrase, char *out); /* ~20 MiB, a few ms */
int mmo_recovery_code(const char *identity, char *out, size_t cap); /* cap >= 80; returns length or -1 */

/* Start connecting to host:port (the game's UDP port).
 * info_json: the body of GET /play/info from the server's web gateway. It carries the pinned
 *            server key, content hash and attribute schema, so a client can only talk to the
 *            server it was given and refuses a game whose content differs.
 * identity:  64 hex characters, or NULL to play as a guest (nothing is saved).
 * Returns NULL on bad arguments. Free with mmo_free. */
MmoClient *mmo_connect(const char *host, uint16_t port, const char *info_json, const char *identity);
void mmo_free(MmoClient *c);

/* Run one frame: read packets, retry the handshake, send input at the server's tick rate,
 * reconnect if the server goes quiet, follow zone handoffs. Returns the status. */
int mmo_poll(MmoClient *c);
int mmo_status(const MmoClient *c);
void mmo_status_text(const MmoClient *c, char *out, size_t cap);

/* Intent. Each axis is -1, 0 or 1; the server decides the speed. Sticks until changed. */
void mmo_set_move(MmoClient *c, int dx, int dy);
/* A built block column, as /play/blocks reports it: keeps prediction in step with the server (climbing, falling,
 * walking under). cx, cy are block cells (multiply by /play/info build.block for world units); bit k of mask is
 * a block in layer base + k, and mask 0 clears the column. */
void mmo_set_column(MmoClient *c, uint32_t cx, uint32_t cy, int base, uint32_t mask);
/* The owner tag /play/blocks gives the blocks you build (out holds 17 bytes); empty for a guest. */
void mmo_account_tag(const MmoClient *c, char *out, size_t cap);
/* One action request (ids are in /play/info "actions") against a target entity id (0 = none). */
void mmo_act(MmoClient *c, uint8_t action, uint32_t target);
/* An action with its extra argument: for build and break (/play/info build), (layer & 0xff) | material << 8, with
 * the layer -128..127 and the material 0..build.kinds-1. */
void mmo_act_arg(MmoClient *c, uint8_t action, uint32_t target, int arg);
/* Say (0) or shout (1). Queued and resent until the server has it; the server rate-limits and trims. */
void mmo_say(MmoClient *c, uint8_t channel, const char *text);
/* Pop the next line heard. Returns 1 and fills out, or 0. */
int mmo_next_chat(MmoClient *c, MmoChat *out);

/* World state, as of the newest snapshot (about 20 per second). Valid until the next mmo_poll.
 * Your own entity is in the list: find it with mmo_self_id (the id changes after a zone handoff). */
uint32_t mmo_self_id(const MmoClient *c);
size_t mmo_entity_count(const MmoClient *c);
int mmo_entity(const MmoClient *c, size_t i, MmoEntity *out);
size_t mmo_attr_count(const MmoClient *c);
const char *mmo_attr_name(const MmoClient *c, size_t i); /* valid while the client lives */

float mmo_rtt_ms(const MmoClient *c);

#ifdef __cplusplus
}
#endif
#endif
