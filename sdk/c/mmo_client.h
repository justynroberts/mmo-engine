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
  MMO_CLOSED = 3,   /* the server never answered */
  MMO_MOVED = 4     /* handed to another zone with following off; see mmo_moved */
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
  uint8_t channel;   /* 0 = say (nearby), 1 = shout (everyone), 3 = banner (from the server: show it big) */
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
/* Zones. By default a handoff is followed by reconnecting (strips of one world share the map). With following off
   (zone lines: every zone a map of its own) the client stops with MMO_MOVED instead, and mmo_moved says where: the
   zone's index (its web address is /play/info "zones"[index]), its IPv4 address and UDP port. Load that zone's
   /play/info and world, then connect again with the same identity. mmo_moved returns 1 once moved, else 0. */
void mmo_follow_redirects(MmoClient *c, int on);
int mmo_moved(const MmoClient *c, uint8_t *zone, uint8_t ip[4], uint16_t *port);

/* Intent. Each axis is -1, 0 or 1; the server decides the speed. Sticks until changed. */
void mmo_set_move(MmoClient *c, int dx, int dy);
/* A built block column, as /play/blocks reports it: keeps prediction in step with the server (climbing, falling,
 * walking under). cx, cy are block cells (multiply by /play/info build.block for world units); bit k of mask is
 * a block in layer base + k, and mask 0 clears the column. */
void mmo_set_column(MmoClient *c, uint32_t cx, uint32_t cy, int base, uint32_t mask);
/* Walls, for a world that streams them (/play/info walls.inline false; small worlds send them inline and these do
 * nothing). As the player moves, ask which /play/tiles request to make for them at (x, y): written to out as a
 * path with query, the return is its length, 0 for nothing missing, -1 if cap is too small. GET it from the web
 * gateway and pass the reply's bytes to mmo_put_tiles, or call mmo_tiles_failed if the fetch failed. mmo_put_tiles
 * returns 1 when the walls are in prediction, 0 when they have changed on the server (a 409 reply means the same):
 * fetch /play/info again and pass it to mmo_set_walls, which also adopts walls edited in the console. */
int mmo_tiles_wanted(MmoClient *c, float x, float y, char *out, size_t cap);
int mmo_put_tiles(MmoClient *c, const uint8_t *bytes, size_t len);
void mmo_tiles_failed(MmoClient *c);
/* 1 once every wall near the player is in (always, for inline walls). */
int mmo_walls_ready(const MmoClient *c);
int mmo_set_walls(MmoClient *c, const char *info_json);
/* The owner tag /play/blocks gives the blocks you build (out holds 17 bytes); empty for a guest. */
void mmo_account_tag(const MmoClient *c, char *out, size_t cap);
/* Your light on (1) or off (0). With a `lit` attribute in the game, others hide your name in the dark while it is out
 * and bots see you less far at night. Sent in an input that has no other action, so it never displaces one. */
void mmo_set_light(MmoClient *c, int on);
/* One action request (ids are in /play/info "actions") against a target entity id (0 = none). */
void mmo_act(MmoClient *c, uint8_t action, uint32_t target);
/* An action with its extra argument: for build and break (/play/info build), (layer & 0xff) | material << 8, with
 * the layer -128..127 and the material 0..build.kinds-1. */
void mmo_act_arg(MmoClient *c, uint8_t action, uint32_t target, int arg);
/* Say (0) or shout (1). Queued and resent until the server has it; the server rate-limits and trims. */
void mmo_say(MmoClient *c, uint8_t channel, const char *text);
/* Ask to be known by a name (3 to 16 letters; see PROTOCOL.md "Names"). The server answers with a chat line from id
 * 0. Everyone's names: GET /play/names?ids=1,2,3 on the web gateway. */
void mmo_set_name(MmoClient *c, const char *name);
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

/* Actions used near you (spells, blows), each reported once. target is 0 when aimed at the caster. */
typedef struct {
    uint32_t caster;
    uint32_t target;
    uint8_t action;
} MmoCast;
size_t mmo_take_casts(MmoClient *c, MmoCast *out, size_t cap); /* oldest first; returns how many */
/* Level and experience: at = this level's start, next = the next level's (0 at the cap); level 0 = not known. */
void mmo_progress(const MmoClient *c, uint8_t *level, uint32_t *xp, uint32_t *at, uint32_t *next);
/* Quests: state 0 in hand, 1 ready to hand in, 2 done. Returns how many; offer = quest just put to us (255 none).
   Act 232 talks to a target, 233 takes quest `arg`, 234 gives quest `arg` up (mmo_act_arg). */
typedef struct {
    uint8_t quest;
    uint8_t step;
    uint16_t count;
    uint8_t state;
} MmoQuest;
size_t mmo_quests(const MmoClient *c, MmoQuest *out, size_t cap, uint8_t *offer);
/* Party ids (us among them; 0 = in none); gear item+1 per slot (weapon, armour, trinket) and riding.
   Act 235 buys shop item `arg` from merchant `target`, 236 sells bag slot `arg` to them, 237 takes off gear slot `arg`. */
size_t mmo_party(const MmoClient *c, uint32_t *out, size_t cap);
/* The crowd past our snapshot (up to 480, twice the interest radius, a few times a second): side is the faction
   (index + 1, 0 none), player 1 for a player. Returns how many there are; copies up to cap. */
typedef struct {
    uint32_t id;
    float x, y;
    uint8_t side, player;
} MmoCrowd;
size_t mmo_crowd(const MmoClient *c, MmoCrowd *out, size_t cap);
void mmo_gear(const MmoClient *c, uint8_t *out3, uint8_t *riding);

#ifdef __cplusplus
}
#endif
#endif
