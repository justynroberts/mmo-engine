/* A console client: connect, walk east, print what is around, say hello.
 *
 *   cargo build --release -p mmo-client
 *   cc sdk/c/example.c -Isdk/c -Ltarget/release -lmmo_client -Wl,-rpath,target/release -o example
 *   curl -s http://127.0.0.1:8080/play/info > info.json     # server run with --web 127.0.0.1:8080
 *   ./example 127.0.0.1 7777 info.json [identity-hex]
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "mmo_client.h"

static char *slurp(const char *path) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  rewind(f);
  char *b = malloc(n + 1);
  if (fread(b, 1, n, f) != (size_t)n) { fclose(f); free(b); return NULL; }
  b[n] = 0;
  fclose(f);
  return b;
}

int main(int argc, char **argv) {
  if (argc < 4) { fprintf(stderr, "usage: %s host port info.json [identity-hex]\n", argv[0]); return 2; }
  if (sizeof(MmoEntity) != 28 || sizeof(MmoChat) != 264) { fprintf(stderr, "struct layout mismatch\n"); return 3; }
  char *info = slurp(argv[3]);
  if (!info) { fprintf(stderr, "cannot read %s\n", argv[3]); return 2; }
  MmoClient *c = mmo_connect(argv[1], (uint16_t)atoi(argv[2]), info, argc > 4 ? argv[4] : NULL);
  if (!c) { fprintf(stderr, "connect: %s\n", mmo_last_error()); return 1; }

  int said = 0;
  struct timespec frame = {0, 16 * 1000 * 1000};
  for (int i = 0; i < 400; i++) { /* about 6 seconds */
    int st = mmo_poll(c);
    if (st == MMO_REJECTED || st == MMO_CLOSED) {
      char why[128];
      mmo_status_text(c, why, sizeof why);
      fprintf(stderr, "stopped: %s\n", why);
      mmo_free(c);
      return 1;
    }
    if (st == MMO_PLAYING) {
      mmo_set_move(c, 1, 0);
      if (!said) { mmo_say(c, 1, "hello from C"); said = 1; }
      MmoChat m;
      while (mmo_next_chat(c, &m)) printf("chat from #%u: %s\n", m.from, m.text);
      if (i % 60 == 0) {
        MmoEntity e;
        size_t n = mmo_entity_count(c);
        for (size_t k = 0; k < n; k++)
          if (mmo_entity(c, k, &e) && e.id == mmo_self_id(c))
            printf("me #%u at (%.1f, %.1f) %s=%.0f, %zu entities in view, rtt %.1f ms\n", e.id, e.x, e.y, mmo_attr_name(c, 0), e.attrs[0], n, mmo_rtt_ms(c));
      }
    }
    nanosleep(&frame, NULL);
  }
  mmo_free(c);
  return 0;
}
